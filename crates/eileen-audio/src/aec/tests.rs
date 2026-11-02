//! The canceller and the energy detector over `RoomMixer` (WP4 acceptance 1 short of its
//! transcript check, which needs the recognizer), and the reference switch on its own.
//!
//! The room runs at the speech pipe's 16 kHz mono, so each 10 ms buffer is one chunk: the
//! resamplers that bring a 48 kHz endpoint there are measured in `resample::tests`. The loopback
//! reaches the canceller two chunks after Eileen's own stream, as a real endpoint's lags it. The
//! voices, music and video are synthetic and made here from additions, multiplications,
//! divisions and square roots alone, whose results IEEE 754 fixes to the bit, so every run on
//! every platform hears the same audio; the platform's `sin` and `exp` differ between C runtimes.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::sleep;

use super::{
    CHUNK, ChunkReport, EchoCanceller, LEARNING_CHUNKS, MISSING_CHUNKS, RATE_HZ, Reference,
    SILENT_CHUNKS,
};
use crate::energy::{EnergyWindow, ONSET_WINDOWS, WindowAssembler};
use crate::pace::Pace;
use crate::room::{Bed, BedSource, RoomConfig, RoomMixer};
use crate::stream::{
    AudioSink as _, AudioSource as _, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, StreamFormat,
};
use crate::testing::ms;
use crate::wav::WavClip;

const MONO_16K: StreamFormat = StreamFormat { sample_rate_hz: RATE_HZ, channels: 1 };
const RATE: f32 = 16_000.0;

/// The room's echo: 30 ms from speaker to microphone, about 10 dB down.
const ECHO_DELAY: Duration = ms(30);
const ECHO_GAIN: f32 = 0.3;

/// Chunks the loopback reaches the canceller after Eileen's own stream.
const LOOPBACK_LAG: usize = 2;

/// The least echo return loss enhancement the canceller must reach once it has converged.
const ERLE_FLOOR_DB: f32 = 20.0;

/// Chunks of the first 3 s, in which AEC3 learns the room and the ERLE floors are not measured.
const CONVERGED: usize = 300;

// Generators.

/// A deterministic generator (xorshift64*), so every run makes the same audio.
struct Rng(u64);

impl Rng {
    const fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    const fn bits(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        let top = usize::try_from(self.bits() >> 40).unwrap();
        float(top) / 16_777_216.0
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        (high - low) * self.unit() + low
    }

    /// A number in `[-1, 1)`.
    fn signed(&mut self) -> f32 {
        2.0 * self.unit() - 1.0
    }

    fn index(&mut self, len: usize) -> usize {
        usize::try_from(self.bits() % u64::try_from(len).unwrap()).unwrap()
    }
}

#[expect(clippy::cast_precision_loss, reason = "test counts, far inside f32's exact range")]
const fn float(n: usize) -> f32 {
    n as f32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a short test duration, positive"
)]
const fn samples_in(seconds: f32) -> usize {
    (seconds * RATE + 0.5) as usize
}

/// `sin(x)` to about 1e-7 from IEEE 754's exactly rounded operations alone: `x` brought into
/// `[-pi/2, pi/2]`, then its Taylor series to the eleventh power.
fn sine(x: f32) -> f32 {
    const C3: f32 = -1.0 / 6.0;
    const C5: f32 = 1.0 / 120.0;
    const C7: f32 = -1.0 / 5_040.0;
    const C9: f32 = 1.0 / 362_880.0;
    const C11: f32 = -1.0 / 39_916_800.0;
    let turned = x - TAU * (x / TAU).round();
    let x = if turned > FRAC_PI_2 {
        PI - turned
    } else if turned < -FRAC_PI_2 {
        -PI - turned
    } else {
        turned
    };
    let x2 = x * x;
    x * (1.0 + x2 * (C3 + x2 * (C5 + x2 * (C7 + x2 * (C9 + x2 * C11)))))
}

/// Vowels' first two formants, Hz.
const VOWELS: [(f32, f32); 5] =
    [(730.0, 1_090.0), (270.0, 2_290.0), (530.0, 1_840.0), (570.0, 840.0), (300.0, 870.0)];

/// How strongly a resonance at `formant` passes a harmonic at `hz`.
const fn resonance(hz: f32, formant: f32) -> f32 {
    let x = (hz - formant) / 120.0;
    1.0 / (1.0 + x * x)
}

/// `seconds` of a synthetic voice, scaled to `rms`: words of one to four syllables with gaps of
/// 50 to 400 ms between them, each syllable 120 to 300 ms of a harmonic series gliding between
/// two pitches in `pitch`, shaped by a vowel's formants and rising and falling, two in five
/// opened by a 40 ms burst of noise. AEC3's delay estimate, noise suppression and the energy
/// detector all treat steady tones differently from speech, so the tests use this instead.
fn voice(seed: u64, pitch: (f32, f32), seconds: f32, rms: f32) -> Vec<f32> {
    let len = samples_in(seconds);
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(len + samples_in(2.0));
    while out.len() < len {
        for _ in 0..=rng.index(4) {
            syllable(&mut rng, pitch, &mut out);
        }
        let gap = samples_in(rng.between(0.05, 0.4));
        out.resize(out.len() + gap, 0.0);
    }
    out.truncate(len);
    scaled(out, rms)
}

fn syllable(rng: &mut Rng, pitch: (f32, f32), out: &mut Vec<f32>) {
    let len = samples_in(rng.between(0.12, 0.3));
    let (from, to) = (rng.between(pitch.0, pitch.1), rng.between(pitch.0, pitch.1));
    let (f1, f2) = VOWELS[rng.index(VOWELS.len())];
    let burst = if rng.unit() < 0.4 { samples_in(0.04) } else { 0 };
    let mut phase = 0.0_f32;
    for n in 0..len {
        let t = float(n) / float(len);
        let f0 = (to - from) * t + from;
        phase += TAU * f0 / RATE;
        if phase >= TAU {
            phase -= TAU;
        }
        let mut sample = 0.0;
        for k in 1..=24_u16 {
            let hz = f32::from(k) * f0;
            if hz > 3_800.0 {
                break;
            }
            let gain = resonance(hz, f1) + 0.5 * resonance(hz, f2) + 0.02;
            sample += gain * sine(f32::from(k) * phase);
        }
        sample *= sine(PI * t);
        if n < burst {
            sample += 0.5 * rng.signed() * (1.0 - float(n) / float(burst));
        }
        out.push(sample);
    }
}

/// `seconds` of synthetic music, scaled to `rms`: a chord of three notes every 500 ms from a
/// pentatonic scale in just intonation, each note six harmonics decaying like a plucked string,
/// and a burst of noise on every 250 ms beat, like a hi-hat. It never pauses.
fn music(seed: u64, seconds: f32, rms: f32) -> Vec<f32> {
    const SCALE: [(f32, f32); 7] =
        [(1.0, 1.0), (6.0, 5.0), (4.0, 3.0), (3.0, 2.0), (9.0, 5.0), (2.0, 1.0), (12.0, 5.0)];
    /// A plucked string's decay each sample: about `exp(-3 t)`.
    const DECAY: f32 = 1.0 - 3.0 / RATE;
    let (chord, beat, hat) = (samples_in(0.5), samples_in(0.25), samples_in(0.03));
    let mut rng = Rng::new(seed);
    let mut notes = [220.0_f32; 3];
    let mut decay = 1.0_f32;
    let mut out = Vec::with_capacity(samples_in(seconds));
    for n in 0..samples_in(seconds) {
        if n % chord == 0 {
            for note in &mut notes {
                let (over, under) = SCALE[rng.index(SCALE.len())];
                *note = 220.0 * over / under;
            }
            decay = 1.0;
        }
        let t = float(n % chord) / RATE;
        let mut sample = 0.0;
        for note in notes {
            for h in 1..=6_u16 {
                let h = f32::from(h);
                sample += sine(TAU * note * h * t) / h;
            }
        }
        sample *= decay;
        decay *= DECAY;
        if n % beat < hat {
            sample += 0.8 * rng.signed() * (1.0 - float(n % beat) / float(hat));
        }
        out.push(sample);
    }
    scaled(out, rms)
}

/// `samples` scaled to `rms`.
fn scaled(mut samples: Vec<f32>, rms: f32) -> Vec<f32> {
    let gain = rms / mean_square(&samples).sqrt();
    for sample in &mut samples {
        *sample *= gain;
    }
    samples
}

fn mean_square(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample * sample).sum::<f32>() / float(samples.len())
}

fn db(ratio: f32) -> f32 {
    10.0 * ratio.log10()
}

/// `samples` starting `seconds` in, looped.
fn starting_at(mut samples: Vec<f32>, seconds: u64) -> Vec<f32> {
    let start = usize::try_from(seconds).unwrap() * samples_in(1.0) % samples.len();
    samples.rotate_left(start);
    samples
}

/// Eileen's voice from the speakers: a minute, looped, at -26 dBFS, so that with a bed under it
/// the speakers' mix peaks near -6 dBFS and never clips, as a device's mix cannot.
fn eileen() -> Vec<f32> {
    voice(1, (170.0, 250.0), 60.0, 0.05)
}

/// A spoken command, two seconds, at -22 dBFS at the microphone.
fn command() -> Vec<f32> {
    voice(7, (110.0, 160.0), 2.0, 0.08)
}

/// The music the PC plays, 47 s looped: no length shares a factor with Eileen's minute, so ten
/// minutes of the two never repeat a stretch.
fn music_samples() -> Vec<f32> {
    music(3, 47.0, 0.05)
}

/// The speech video the PC plays, 53 s looped: a man talking over quiet music.
fn video_samples() -> Vec<f32> {
    let talk = voice(11, (90.0, 150.0), 53.0, 0.045);
    let under = music(12, 53.0, 0.015);
    talk.iter().zip(&under).map(|(a, b)| a + b).collect()
}

/// A bed the speakers play.
fn played(samples: Vec<f32>) -> Bed {
    Bed { clip: WavClip::new(MONO_16K, samples).unwrap(), gain: 1.0, from: BedSource::Speakers }
}

/// `clip` after `seconds` of silence.
fn after(seconds: f32, clip: &[f32]) -> Vec<f32> {
    let mut samples = vec![0.0; samples_in(seconds)];
    samples.extend_from_slice(clip);
    samples
}

// The room.

/// What one chunk measured.
#[derive(Debug, Clone, Copy)]
struct Measured {
    /// The microphone's mean square, and the canceller's.
    mic: f32,
    out: f32,
    report: ChunkReport,
}

/// The capture side: the canceller and the energy fallback, fed as the component will feed
/// them, from the loopback's and the speaker's buffers of the same time.
struct Listener {
    canceller: EchoCanceller,
    assembler: WindowAssembler,
    own: [f32; CHUNK],
    loopback: Option<[f32; CHUNK]>,
    /// The loopback's last chunks, which reach the canceller [`LOOPBACK_LAG`] chunks late.
    lag: [[f32; CHUNK]; LOOPBACK_LAG],
    next_lag: usize,
    /// The loopback runs but carries silence, as when the default endpoint changes under it.
    loopback_silent: bool,
    /// The chunk whose microphone, loopback and own PCM each get a NaN or infinite sample.
    poison_at: Option<usize>,
    chunks: Vec<Measured>,
    windows: Vec<EnergyWindow>,
    /// When each onset's window ended, and its index into `windows`.
    onsets: Vec<(Duration, usize)>,
    /// The loudest sample the loopback carried: what the speakers played.
    loudest: f32,
}

impl Listener {
    fn hear(&mut self, buffer: &[f32], at: Duration) {
        let mut mic: [f32; CHUNK] = buffer.try_into().unwrap();
        let mut loopback = self.loopback.take();
        let mut own = std::mem::replace(&mut self.own, [0.0; CHUNK]);
        if self.poison_at == Some(self.chunks.len()) {
            mic[0] = f32::NAN;
            own[7] = f32::NAN;
            if let Some(chunk) = &mut loopback {
                chunk[5] = f32::INFINITY;
            }
        }
        let mut out = [0.0; CHUNK];
        let report = self.canceller.process(&mic, loopback.as_ref(), &own, &mut out).unwrap();
        self.chunks.push(Measured { mic: mean_square(&mic), out: mean_square(&out), report });
        if let Some(window) = self.assembler.push(&out, &mic, report) {
            self.windows.push(window);
            if window.onset {
                self.onsets.push((at + ms(10), self.windows.len() - 1));
            }
        }
    }
}

type Shared = Arc<Mutex<Listener>>;

/// What a room run heard.
struct Heard {
    chunks: Vec<Measured>,
    windows: Vec<EnergyWindow>,
    onsets: Vec<(Duration, usize)>,
    loudest: f32,
}

impl Heard {
    /// Each onset's time with the levels, at the microphone and after the canceller, of the
    /// windows that made it, for a failure's message.
    fn onset_levels(&self) -> String {
        let onsets = self.onsets.iter().map(|&(at, window)| {
            let run = usize::try_from(ONSET_WINDOWS).unwrap();
            let levels: Vec<String> = self.windows[window + 1 - run..=window]
                .iter()
                .map(|w| format!("{:.1}/{:.1}", w.heard_dbfs, w.level_dbfs))
                .collect();
            format!("{at:?}: {} dBFS", levels.join(" "))
        });
        onsets.collect::<Vec<_>>().join("; ")
    }

    /// The first onset's time.
    fn first_onset(&self) -> Option<Duration> {
        self.onsets.first().map(|&(at, _)| at)
    }

    /// How close the detector came to an onset where it judged energy: over each run of
    /// [`ONSET_WINDOWS`] windows in a row, the most any of them fell short of speech, and the
    /// least of that over the run, in dB. Under 0 means it fired.
    fn closest_approach(&self) -> f32 {
        self.windows
            .windows(usize::try_from(ONSET_WINDOWS).unwrap())
            .map(|run| run.iter().map(|w| w.short_db).fold(f32::NEG_INFINITY, f32::max))
            .fold(f32::INFINITY, f32::min)
    }

    /// The echo return loss enhancement over `chunks`, in dB.
    fn erle(&self, chunks: std::ops::Range<usize>) -> f32 {
        let chunks = &self.chunks[chunks];
        let mic: f32 = chunks.iter().map(|chunk| chunk.mic).sum();
        let out: f32 = chunks.iter().map(|chunk| chunk.out).sum();
        db(mic / out.max(1e-30))
    }

    /// The canceller's summed output over `chunks`.
    fn out(&self, chunks: std::ops::Range<usize>) -> f32 {
        self.chunks[chunks].iter().map(|chunk| chunk.out).sum()
    }

    fn references(&self, chunks: std::ops::Range<usize>) -> Vec<Reference> {
        self.chunks[chunks].iter().map(|chunk| chunk.report.reference).collect()
    }

    fn learning(&self) -> Vec<bool> {
        self.chunks.iter().map(|chunk| chunk.report.learning).collect()
    }

    fn other_audio(&self) -> Vec<bool> {
        self.chunks.iter().map(|chunk| chunk.report.flags.other_audio).collect()
    }

    fn playback(&self) -> Vec<bool> {
        self.chunks.iter().map(|chunk| chunk.report.flags.playback).collect()
    }
}

/// What a room plays and hears.
#[derive(Default)]
struct Scene {
    /// What Eileen says, looped, from the speakers.
    eileen: Option<Vec<f32>>,
    /// The near-end talker.
    near: Option<Vec<f32>>,
    bed: Option<Bed>,
    /// Whether the loopback runs.
    loopback: bool,
    poison_at: Option<usize>,
}

/// A room playing `scene`, its streams started, and the listener on its microphone. The first
/// stream is the loopback's when it runs.
fn start(scene: Scene) -> (Shared, Vec<Box<dyn AudioStream>>) {
    let config = RoomConfig {
        format: MONO_16K,
        pace: Pace::RealTime,
        echo_delay: ECHO_DELAY,
        echo_gain: ECHO_GAIN,
        near: scene.near.map(|samples| WavClip::new(MONO_16K, samples).unwrap()),
        bed: scene.bed,
    };
    let mixer = RoomMixer::new(config).unwrap();
    let shared = Arc::new(Mutex::new(Listener {
        canceller: EchoCanceller::new(),
        assembler: WindowAssembler::new(),
        own: [0.0; CHUNK],
        loopback: None,
        lag: [[0.0; CHUNK]; LOOPBACK_LAG],
        next_lag: 0,
        loopback_silent: false,
        poison_at: scene.poison_at,
        chunks: Vec::new(),
        windows: Vec::new(),
        onsets: Vec::new(),
        loudest: 0.0,
    }));
    let mut streams = Vec::new();
    if scene.loopback {
        streams.push(mixer.loopback().start(loopback(&shared)).unwrap());
    }
    if let Some(eileen) = scene.eileen {
        streams.push(mixer.sink().start(speaker(&shared, eileen)).unwrap());
    }
    streams.push(mixer.source().start(microphone(&shared)).unwrap());
    (shared, streams)
}

/// Runs `scene` for `seconds` and returns what the listener heard.
async fn listen(scene: Scene, seconds: u64) -> Heard {
    let (shared, streams) = start(scene);
    sleep(ms(seconds * 1_000 + 5)).await;
    let heard = take(&shared);
    drop(streams);
    heard
}

fn take(shared: &Shared) -> Heard {
    let mut listener = shared.lock().unwrap();
    Heard {
        chunks: std::mem::take(&mut listener.chunks),
        windows: std::mem::take(&mut listener.windows),
        onsets: std::mem::take(&mut listener.onsets),
        loudest: listener.loudest,
    }
}

fn speaker(shared: &Shared, voice: Vec<f32>) -> RenderCallback {
    let shared = Arc::clone(shared);
    let mut next = 0;
    Box::new(move |buffer: &mut [f32], _: RenderInfo| {
        for sample in &mut *buffer {
            *sample = voice[next % voice.len()];
            next += 1;
        }
        shared.lock().unwrap().own.copy_from_slice(buffer);
    })
}

fn loopback(shared: &Shared) -> CaptureCallback {
    let shared = Arc::clone(shared);
    Box::new(move |buffer: &[f32], _: CaptureInfo| {
        let mut listener = shared.lock().unwrap();
        let loudest = buffer.iter().fold(listener.loudest, |most, sample| most.max(sample.abs()));
        listener.loudest = loudest;
        let chunk: [f32; CHUNK] =
            if listener.loopback_silent { [0.0; CHUNK] } else { buffer.try_into().unwrap() };
        let slot = listener.next_lag;
        let lagged = std::mem::replace(&mut listener.lag[slot], chunk);
        listener.next_lag = (slot + 1) % LOOPBACK_LAG;
        listener.loopback = Some(lagged);
    })
}

fn microphone(shared: &Shared) -> CaptureCallback {
    let shared = Arc::clone(shared);
    Box::new(move |buffer: &[f32], info: CaptureInfo| {
        shared.lock().unwrap().hear(buffer, info.captured_at);
    })
}

/// Whether each of the first `chunks` of `voice` sounds: the playback flag it should carry.
fn sounding(voice: &[f32], chunks: usize) -> Vec<bool> {
    (0..chunks).map(|k| mean_square(&voice[k * CHUNK..(k + 1) * CHUNK]) >= 1e-6).collect()
}

/// The first chunk AEC3 had learned the room by.
fn learned_at(heard: &Heard) -> Option<usize> {
    heard.learning().iter().position(|&learning| !learning)
}

// AEC3 against the loopback.

#[tokio::test(start_paused = true)]
async fn eileen_speaking_is_cancelled_past_the_erle_floor_against_the_loopback() {
    let voice = eileen();
    let scene = Scene { eileen: Some(voice.clone()), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    // Her first two chunks reach the loopback after she sounds, inside the 50 ms allowance.
    let references = heard.references(0..1_200);
    assert!(references.iter().all(|&r| r == Reference::Loopback), "the loopback throughout");
    assert_eq!(heard.playback()[..1_200].to_vec(), sounding(&voice, 1_200), "while she sounds");
    assert!(heard.other_audio().iter().all(|&o| !o), "her own voice is not other audio");
    let learned = learned_at(&heard);
    let least = usize::try_from(LEARNING_CHUNKS).unwrap() + LOOPBACK_LAG - 1;
    assert!(
        learned.is_some_and(|at| (least..600).contains(&at)),
        "learning until 3 s of her voice reached AEC3: {learned:?}"
    );
    assert!(heard.learning()[learned.unwrap()..].iter().all(|&l| !l), "and not again");
}

#[tokio::test(start_paused = true)]
async fn a_music_bed_from_the_speakers_is_cancelled_and_sets_other_audio() {
    let scene = Scene { bed: Some(played(music_samples())), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    let references = heard.references(0..1_200);
    let (silent, arrived) = references.split_at(LOOPBACK_LAG);
    assert!(silent.iter().all(|&r| r == Reference::Bypassed), "bypassed till it arrives");
    assert!(arrived.iter().all(|&r| r == Reference::Loopback), "then the loopback throughout");
    // 200 ms of it from the first chunk that reaches the canceller.
    let other = heard.other_audio();
    let on = LOOPBACK_LAG + 19;
    assert!(other[..on].iter().all(|&o| !o), "not before 200 ms of it");
    assert!(other[on..].iter().all(|&o| o), "other audio from 200 ms on");
    assert!(heard.playback().iter().all(|&p| !p), "Eileen played nothing");
}

#[tokio::test(start_paused = true)]
async fn the_speech_video_bed_from_the_speakers_is_cancelled() {
    let scene = Scene { bed: Some(played(video_samples())), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    assert!(heard.other_audio()[100..].iter().all(|&o| o), "other audio, past its pauses");
}

#[tokio::test(start_paused = true)]
async fn a_bed_in_the_room_is_not_other_audio_and_leaves_aec3_bypassed() {
    let video = played(video_samples());
    let scene = Scene {
        bed: Some(Bed { from: BedSource::Room, ..video }),
        loopback: true,
        ..Scene::default()
    };
    let heard = listen(scene, 6).await;

    let references = heard.references(0..600);
    assert!(references.iter().all(|&r| r == Reference::Bypassed), "nothing played: bypassed");
    assert!(heard.other_audio().iter().all(|&o| !o), "the loopback never carried it");
}

// The near end.

#[tokio::test(start_paused = true)]
async fn a_command_alone_keeps_its_energy_and_the_detector_fires_on_its_onset() {
    let scene = Scene { near: Some(after(1.0, &command())), loopback: true, ..Scene::default() };
    let heard = listen(scene, 4).await;

    let command = &heard.chunks[100..300];
    let kept =
        db(command.iter().map(|c| c.out).sum::<f32>() / command.iter().map(|c| c.mic).sum::<f32>());
    assert!((-3.0..=0.5).contains(&kept), "the command kept {kept:.1} dB of its energy");
    let first = heard.first_onset();
    assert!(first.is_some_and(|at| (ms(1_000)..=ms(1_250)).contains(&at)), "onset at {first:?}");
}

/// What the canceller made of a command spoken at 6 s over what `scene` plays.
struct OverCommand {
    /// The command's energy kept, in dB: the canceller's output with the command less its
    /// output without it, over the command's two seconds, against the command's own energy at
    /// the microphone.
    kept: f32,
    /// The ERLE over those two seconds without the command: the echo under it was removed.
    erle: f32,
    /// The run with the command.
    heard: Heard,
}

/// Runs `scene` for 9 s with and without the command at 6 s, by when AEC3 has learned the room.
/// The room and the canceller are deterministic, so the two runs differ only by the command.
async fn command_over(scene: fn() -> Scene) -> OverCommand {
    let spoken = command();
    let near: f32 = spoken.chunks(CHUNK).map(mean_square).sum();
    let with = listen(Scene { near: Some(after(6.0, &spoken)), ..scene() }, 9).await;
    let without = listen(scene(), 9).await;
    let kept = db((with.out(600..800) - without.out(600..800)) / near);
    OverCommand { kept, erle: without.erle(600..800), heard: with }
}

/// Checks a command over the far end: its energy kept within -6 and +1 dB, the echo under it
/// removed past the floor, and an onset within 250 ms of its start and none before.
fn assert_command_heard(over: &OverCommand) {
    let OverCommand { kept, erle, heard } = over;
    assert!((-6.0..=1.0).contains(kept), "the command kept {kept:.1} dB of its energy");
    assert!(*erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB under the command, without it");
    let first = heard.first_onset();
    assert!(
        first.is_some_and(|at| (ms(6_000)..=ms(6_250)).contains(&at)),
        "no onset from the far end, then one at the command: {}",
        heard.onset_levels()
    );
}

#[tokio::test(start_paused = true)]
async fn a_command_over_eileen_keeps_its_energy_and_the_detector_fires_on_its_onset() {
    let over =
        command_over(|| Scene { eileen: Some(eileen()), loopback: true, ..Scene::default() }).await;
    assert_command_heard(&over);
}

/// A bed the speakers play 6 dB over Eileen's voice, so that its energy stands over hers
/// through her syllables and the other-audio flag stays on under the whole command.
fn louder(samples: Vec<f32>) -> Bed {
    Bed { gain: 2.0, ..played(samples) }
}

/// Checks that other audio played across the whole command, so that the windows that made its
/// onset were judged with the stricter margin.
fn assert_judged_as_other_audio(heard: &Heard) {
    let other = heard.other_audio()[600..800].iter().filter(|&&o| o).count();
    assert_eq!(other, 200, "other audio in {other} of the command's 200 chunks");
    let (_, onset) = heard.onsets[0];
    let run = usize::try_from(ONSET_WINDOWS).unwrap();
    let judged = &heard.windows[onset + 1 - run..=onset];
    assert!(judged.iter().all(|w| w.other_audio), "judged with the other-audio margin");
}

#[tokio::test(start_paused = true)]
async fn a_command_over_eileen_and_the_music_bed_is_kept_and_its_onset_detected() {
    let over = command_over(|| Scene {
        eileen: Some(eileen()),
        bed: Some(louder(music_samples())),
        loopback: true,
        ..Scene::default()
    })
    .await;
    assert_command_heard(&over);
    assert_judged_as_other_audio(&over.heard);
}

#[tokio::test(start_paused = true)]
async fn a_command_over_eileen_and_the_video_bed_is_kept_and_its_onset_detected() {
    let over = command_over(|| Scene {
        eileen: Some(eileen()),
        bed: Some(louder(video_samples())),
        loopback: true,
        ..Scene::default()
    })
    .await;
    assert_command_heard(&over);
    assert_judged_as_other_audio(&over.heard);
}

// The reference switch.

/// A chunk of noise at about -20 dBFS.
fn noise(rng: &mut Rng) -> [f32; CHUNK] {
    std::array::from_fn(|_| 0.17 * rng.signed())
}

/// Runs one chunk of silence at the microphone with this loopback and own PCM.
fn report_of(
    canceller: &mut EchoCanceller,
    loopback: Option<&[f32; CHUNK]>,
    own: &[f32; CHUNK],
) -> ChunkReport {
    let mut out = [0.0; CHUNK];
    canceller.process(&[0.0; CHUNK], loopback, own, &mut out).unwrap()
}

fn reference_of(
    canceller: &mut EchoCanceller,
    loopback: Option<&[f32; CHUNK]>,
    own: &[f32; CHUNK],
) -> Reference {
    report_of(canceller, loopback, own).reference
}

#[test]
fn aec3_is_bypassed_after_500_ms_of_loopback_silence_and_returns_at_once() {
    let mut rng = Rng::new(5);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    assert_eq!(reference_of(&mut canceller, Some(&silence), &silence), Reference::Bypassed);

    for _ in 0..100 {
        let chunk = noise(&mut rng);
        assert_eq!(reference_of(&mut canceller, Some(&chunk), &silence), Reference::Loopback);
    }
    // The chunk that completes 500 ms of silence is the first bypassed.
    let quiet: Vec<Reference> =
        (0..60).map(|_| reference_of(&mut canceller, Some(&silence), &silence)).collect();
    let mut expected = vec![Reference::Loopback; usize::try_from(SILENT_CHUNKS).unwrap() - 1];
    expected.resize(60, Reference::Bypassed);
    assert_eq!(quiet, expected, "49 chunks on the loopback, then bypassed");

    let chunk = noise(&mut rng);
    assert_eq!(reference_of(&mut canceller, Some(&chunk), &silence), Reference::Loopback);
}

#[test]
fn own_pcm_is_the_reference_once_she_sounds_for_50_ms_without_the_loopback_or_it_is_gone() {
    let mut rng = Rng::new(6);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    for _ in 0..100 {
        let (looped, own) = (noise(&mut rng), noise(&mut rng));
        assert_eq!(reference_of(&mut canceller, Some(&looped), &own), Reference::Loopback);
    }
    // A loopback that falls silent while Eileen speaks: own PCM from her fifth chunk without it.
    let switched: Vec<Reference> =
        (0..60).map(|_| reference_of(&mut canceller, Some(&silence), &noise(&mut rng))).collect();
    let mut expected = vec![Reference::Loopback; usize::try_from(MISSING_CHUNKS).unwrap() - 1];
    expected.resize(60, Reference::OwnPcm);
    assert_eq!(switched, expected, "four chunks on the silent loopback, then own PCM");
    // Her pauses do not bring the silent loopback back, and its first sound does.
    let paused: Vec<Reference> =
        (0..20).map(|_| reference_of(&mut canceller, Some(&silence), &silence)).collect();
    assert!(paused.iter().all(|&r| r == Reference::OwnPcm), "own PCM through her pause");
    let chunk = noise(&mut rng);
    assert_eq!(reference_of(&mut canceller, Some(&chunk), &silence), Reference::Loopback);

    // No loopback at all: own PCM at once, and bypassed 500 ms after she stops.
    let chunk = noise(&mut rng);
    assert_eq!(reference_of(&mut canceller, None, &chunk), Reference::OwnPcm);
    let stopped: Vec<Reference> =
        (0..60).map(|_| reference_of(&mut canceller, None, &silence)).collect();
    let mut expected = vec![Reference::OwnPcm; 49];
    expected.resize(60, Reference::Bypassed);
    assert_eq!(stopped, expected, "own PCM through its echo tail, then bypassed");
}

#[test]
fn a_loopback_two_chunks_behind_her_stream_stays_the_reference() {
    // Each time she starts after a pause, her first two chunks reach the loopback late.
    let mut rng = Rng::new(8);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    let mut lag = [[0.0; CHUNK]; 2];
    for k in 0..1_000 {
        let own = if k % 50 < 20 { noise(&mut rng) } else { silence };
        let looped = std::mem::replace(&mut lag[k % 2], own);
        let reference = reference_of(&mut canceller, Some(&looped), &own);
        assert_ne!(reference, Reference::OwnPcm, "chunk {k}: the loopback carries her");
    }
}

#[test]
fn aec3_learns_for_3_s_of_sounding_reference_and_again_when_its_reference_changes() {
    let mut rng = Rng::new(10);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    let learned = usize::try_from(LEARNING_CHUNKS).unwrap();
    let learning: Vec<bool> = (0..learned + 10)
        .map(|_| report_of(&mut canceller, Some(&noise(&mut rng)), &silence).learning)
        .collect();
    let mut expected = vec![true; learned - 1];
    expected.resize(learned + 10, false);
    assert_eq!(learning, expected, "learning through the 299th chunk of sound");

    // An idle spell keeps what it learned: the bypass leaves AEC3 alone.
    for _ in 0..100 {
        let report = report_of(&mut canceller, Some(&silence), &silence);
        assert!(!report.learning, "nothing to learn, or bypassed");
    }
    let report = report_of(&mut canceller, Some(&noise(&mut rng)), &silence);
    assert_eq!((report.reference, report.learning), (Reference::Loopback, false), "kept");

    // Her own PCM has another delay to the microphone: AEC3 learns it afresh.
    let report = report_of(&mut canceller, None, &noise(&mut rng));
    assert_eq!((report.reference, report.learning), (Reference::OwnPcm, true), "afresh");
}

#[test]
fn a_nan_or_infinite_input_sample_becomes_silence_and_the_report_says_so() {
    let mut rng = Rng::new(12);
    let mut canceller = EchoCanceller::new();
    let mut out = [0.0; CHUNK];
    let mut echo = [0.0; CHUNK];
    for _ in 0..300 {
        let far = noise(&mut rng);
        let report = canceller.process(&echo, Some(&far), &far, &mut out).unwrap();
        assert!(!report.repaired, "finite input is left alone");
        echo = far.map(|sample| 0.3 * sample);
    }
    for poisoned in 0..3 {
        let (mut mic, mut far, mut own) = (echo, noise(&mut rng), noise(&mut rng));
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][poisoned];
        match poisoned {
            0 => mic[17] = bad,
            1 => far[17] = bad,
            _ => own[17] = bad,
        }
        let report = canceller.process(&mic, Some(&far), &own, &mut out).unwrap();
        assert!(report.repaired, "input {poisoned} carried {bad}");
        assert!(out.iter().all(|sample| sample.is_finite()), "the output stays finite");
        echo = far.map(|sample| 0.3 * sample);
    }
}

#[tokio::test(start_paused = true)]
async fn a_nan_from_the_devices_leaves_the_echo_cancelled() {
    let scene =
        Scene { eileen: Some(eileen()), loopback: true, poison_at: Some(600), ..Scene::default() };
    let heard = listen(scene, 12).await;

    assert!(heard.chunks[600].report.repaired, "the poisoned chunk was repaired");
    assert!(heard.chunks.iter().all(|chunk| chunk.out.is_finite()), "every output finite");
    let erle = heard.erle(650..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB after the NaN");
    assert!(heard.onsets.is_empty(), "no onset: {}", heard.onset_levels());
}

#[test]
fn a_loopback_that_lags_and_scales_eileen_is_not_other_audio_and_a_bed_in_it_is() {
    // On a real endpoint the loopback lags her stream by a buffer or two and may carry it at
    // another volume: two chunks late at full and at half gain, her voice alone never sets the
    // flag, and music under it sets it in her pauses.
    let voice = eileen();
    let bed = music(4, 12.0, 0.025);
    let chunk = |samples: &[f32], k: usize| -> [f32; CHUNK] {
        samples[k * CHUNK..(k + 1) * CHUNK].try_into().unwrap()
    };
    for (gain, with_bed) in [(1.0, false), (0.5, false), (1.0, true)] {
        let mut canceller = EchoCanceller::new();
        let mut out = [0.0; CHUNK];
        let mut other = Vec::new();
        for k in 0..1_200 {
            let own = chunk(&voice, k);
            let lagged = if k < 2 { [0.0; CHUNK] } else { chunk(&voice, k - 2) };
            let under = if with_bed { chunk(&bed, k) } else { [0.0; CHUNK] };
            let looped: [f32; CHUNK] = std::array::from_fn(|n| gain * lagged[n] + under[n]);
            let report = canceller.process(&[0.0; CHUNK], Some(&looped), &own, &mut out);
            other.push(report.unwrap().flags.other_audio);
        }
        let set = other.iter().filter(|&&o| o).count();
        if with_bed {
            assert!(set > 100, "music under her voice set the flag in {set} chunks");
        } else {
            assert_eq!(set, 0, "her voice at gain {gain}, two chunks late, set the flag");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn without_a_loopback_eileen_is_cancelled_against_her_own_pcm() {
    let scene = Scene { eileen: Some(eileen()), ..Scene::default() };
    let heard = listen(scene, 12).await;

    let references = heard.references(0..1_200);
    assert!(references.iter().all(|&r| r == Reference::OwnPcm), "own PCM throughout");
    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    assert!(heard.onsets.is_empty(), "no onset: {}", heard.onset_levels());
}

#[tokio::test(start_paused = true)]
async fn a_loopback_that_stops_hands_over_to_own_pcm_and_the_echo_stays_cancelled() {
    // The loopback is two chunks behind her stream, so the switch moves AEC3's reference 20 ms.
    let scene = Scene { eileen: Some(eileen()), loopback: true, ..Scene::default() };
    let (shared, mut streams) = start(scene);
    sleep(ms(6_005)).await;
    drop(streams.remove(0));
    sleep(ms(6_000)).await;
    let heard = take(&shared);
    drop(streams);

    assert!(heard.references(0..600).iter().all(|&r| r == Reference::Loopback), "first");
    assert!(heard.references(602..1_200).iter().all(|&r| r == Reference::OwnPcm), "then own");
    let erle = heard.erle(650..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB after the switch");
    assert!(heard.onsets.is_empty(), "no onset across the switch: {}", heard.onset_levels());
}

#[tokio::test(start_paused = true)]
async fn a_loopback_that_falls_silent_under_her_voice_hands_over_within_50_ms() {
    // The loopback keeps running but carries silence from 6 s, as when the default endpoint
    // changes under it while Eileen speaks.
    let scene = Scene { eileen: Some(eileen()), loopback: true, ..Scene::default() };
    let (shared, streams) = start(scene);
    sleep(ms(6_005)).await;
    shared.lock().unwrap().loopback_silent = true;
    sleep(ms(6_000)).await;
    let heard = take(&shared);
    drop(streams);

    assert!(heard.references(0..600).iter().all(|&r| r == Reference::Loopback), "first");
    let switched = heard.references(600..1_200).iter().position(|&r| r == Reference::OwnPcm);
    assert!(switched.is_some_and(|at| at <= 60), "own PCM {switched:?} chunks after 6 s");
    assert!(heard.references(700..1_200).iter().all(|&r| r == Reference::OwnPcm), "and kept");
    let erle = heard.erle(650..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB after the switch");
    assert!(heard.onsets.is_empty(), "no onset across the switch: {}", heard.onset_levels());
}

#[tokio::test(start_paused = true)]
async fn an_idle_spell_keeps_aec3_converged_for_the_next_reply() {
    // Eileen speaks 6 s, is quiet 2 s (bypassed from 500 ms in), and speaks again: her second
    // reply is cancelled from its first second, as AEC3 kept what it learned.
    let voice = eileen();
    let mut replies = voice[..samples_in(6.0)].to_vec();
    replies.resize(samples_in(8.0), 0.0);
    replies.extend_from_slice(&voice[samples_in(20.0)..samples_in(24.0)]);
    let scene = Scene { eileen: Some(replies), loopback: true, ..Scene::default() };
    let heard = listen(scene, 11).await;

    assert!(heard.references(700..800).iter().all(|&r| r == Reference::Bypassed), "idle");
    let back = heard.references(800..900);
    assert!(back.iter().all(|&r| r != Reference::OwnPcm), "the loopback, never her PCM");
    assert!(heard.learning()[800..].iter().all(|&l| !l), "nothing to learn again");
    let erle = heard.erle(800..900);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB in the first second after the spell");
}

// The energy detector over the beds.

/// The seconds each run of a bed lasts. Four runs, starting 0, 150, 300 and 450 s into Eileen's
/// voice and the bed, cover ten minutes and 3 s of the two together, each from a fresh canceller
/// that has to learn the room as at the core's start; split this way, each run stays inside the
/// test profiles' time limit on a loaded PC.
const BED_RUN_SECONDS: u64 = 153;

/// Plays Eileen over `bed` from `start_s` seconds into both for [`BED_RUN_SECONDS`], and checks
/// that the energy detector never fires, from the first chunk on.
async fn assert_no_onset_over(bed: Vec<f32>, start_s: u64) {
    let scene = Scene {
        eileen: Some(starting_at(eileen(), start_s)),
        bed: Some(played(starting_at(bed, start_s))),
        loopback: true,
        ..Scene::default()
    };
    let heard = listen(scene, BED_RUN_SECONDS).await;
    assert!(heard.loudest < 1.0, "the speakers' mix clipped at {}", heard.loudest);
    let closest = heard.closest_approach();
    assert!(
        heard.onsets.is_empty(),
        "false onsets from {start_s} s: {} (closest approach {closest:.1} dB)",
        heard.onset_levels()
    );
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_music_bed_from_0_s() {
    assert_no_onset_over(music_samples(), 0).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_music_bed_from_150_s() {
    assert_no_onset_over(music_samples(), 150).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_music_bed_from_300_s() {
    assert_no_onset_over(music_samples(), 300).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_music_bed_from_450_s() {
    assert_no_onset_over(music_samples(), 450).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_video_bed_from_0_s() {
    assert_no_onset_over(video_samples(), 0).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_video_bed_from_150_s() {
    assert_no_onset_over(video_samples(), 150).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_video_bed_from_300_s() {
    assert_no_onset_over(video_samples(), 300).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_over_eileen_and_the_video_bed_from_450_s() {
    assert_no_onset_over(video_samples(), 450).await;
}

/// Plays `bed` alone, Eileen silent, for a minute from a fresh canceller, as while the core
/// starts and the speech process loads its models, and checks that the detector never fires.
async fn assert_no_onset_over_alone(bed: Vec<f32>) {
    let scene = Scene { bed: Some(played(bed)), loopback: true, ..Scene::default() };
    let heard = listen(scene, 60).await;
    let closest = heard.closest_approach();
    assert!(
        heard.onsets.is_empty(),
        "false onsets: {} (closest approach {closest:.1} dB)",
        heard.onset_levels()
    );
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_in_a_minute_of_the_music_bed_alone_from_a_fresh_canceller() {
    assert_no_onset_over_alone(music_samples()).await;
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_in_a_minute_of_the_video_bed_alone_from_a_fresh_canceller() {
    assert_no_onset_over_alone(video_samples()).await;
}

// The callback's budget.

/// sonora 0.2.0 allocates and frees as it runs AEC3, converged too, so the canceller must not
/// run inside an audio callback (the crate's invariant): the component part runs it on the
/// capture side's reader. This pins that down, so a sonora that stops allocating shows here, and
/// checks that each mode's heap use stays small and bounded.
#[test]
fn a_chunk_allocates_so_the_canceller_stays_out_of_the_callbacks() {
    let mut rng = Rng::new(9);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    let mut out = [0.0; CHUNK];
    let mut echo = [0.0; CHUNK];
    for _ in 0..300 {
        let far = noise(&mut rng);
        canceller.process(&echo, Some(&far), &silence, &mut out).unwrap();
        echo = far.map(|sample| 0.3 * sample);
    }
    let far = noise(&mut rng);
    let mic = far.map(|sample| 0.3 * sample);
    let mut run = |loopback: Option<&[f32; CHUNK]>, own: &[f32; CHUNK], chunks: usize| {
        let mut references = Vec::with_capacity(chunks);
        let counted = allocation_counter::measure(|| {
            for _ in 0..chunks {
                let report = canceller.process(&mic, loopback, own, &mut out);
                references.push(report.map(|report| report.reference));
            }
        });
        (counted, references.last().copied())
    };
    let (on_loopback, reference) = run(Some(&far), &far, 50);
    assert_eq!(reference, Some(Ok(Reference::Loopback)), "against the loopback");
    let (on_own, reference) = run(None, &far, 50);
    assert_eq!(reference, Some(Ok(Reference::OwnPcm)), "against own PCM");
    let (bypassed, reference) = run(None, &silence, 100);
    assert_eq!(reference, Some(Ok(Reference::Bypassed)), "bypassed");

    assert!(on_loopback.count_total >= 50, "more than once a chunk: {on_loopback:?}");
    for counted in [on_loopback, on_own, bypassed] {
        assert!(counted.bytes_max < 64 * 1024, "at most 64 KiB live at once: {counted:?}");
    }
}

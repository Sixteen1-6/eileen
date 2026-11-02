//! The canceller and the energy detector over `RoomMixer` (WP4 acceptance 1 short of its
//! transcript check, which needs the recognizer), and the reference switch on its own.
//!
//! The room runs at the speech pipe's 16 kHz mono, so each 10 ms buffer is one chunk: the
//! resamplers that bring a 48 kHz endpoint there are measured in `resample::tests`. The voices,
//! music and video are synthetic and made here, the same on every run.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::sleep;

use super::{CHUNK, ChunkReport, EchoCanceller, RATE_HZ, Reference, SILENT_CHUNKS};
use crate::energy::{EnergyDetector, WINDOW};
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

/// The least echo return loss enhancement the canceller must reach once it has converged.
const ERLE_FLOOR_DB: f32 = 20.0;

/// Chunks of the first 3 s, in which AEC3 learns the room and the tests do not measure it
/// ([`CONVERGED_AT`]).
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
        (high - low).mul_add(self.unit(), low)
    }

    /// A number in `[-1, 1)`.
    fn signed(&mut self) -> f32 {
        2.0_f32.mul_add(self.unit(), -1.0)
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
        let f0 = (to - from).mul_add(t, from);
        phase = (phase + TAU * f0 / RATE) % TAU;
        let mut sample = 0.0;
        for k in 1..=24_u16 {
            let hz = f32::from(k) * f0;
            if hz > 3_800.0 {
                break;
            }
            let gain = resonance(hz, f1) + 0.5 * resonance(hz, f2) + 0.02;
            sample += gain * (f32::from(k) * phase).sin();
        }
        sample *= (PI * t).sin();
        if n < burst {
            sample += 0.5 * rng.signed() * (1.0 - float(n) / float(burst));
        }
        out.push(sample);
    }
}

/// `seconds` of synthetic music, scaled to `rms`: a chord of three notes every 500 ms from a
/// pentatonic scale, each note six harmonics decaying like a plucked string, and a burst of
/// noise on every 250 ms beat, like a hi-hat. It never pauses.
fn music(seed: u64, seconds: f32, rms: f32) -> Vec<f32> {
    const SCALE: [f32; 7] = [0.0, 3.0, 5.0, 7.0, 10.0, 12.0, 15.0];
    let (chord, beat, hat) = (samples_in(0.5), samples_in(0.25), samples_in(0.03));
    let mut rng = Rng::new(seed);
    let mut notes = [220.0_f32; 3];
    let mut out = Vec::with_capacity(samples_in(seconds));
    for n in 0..samples_in(seconds) {
        if n % chord == 0 {
            for note in &mut notes {
                *note = 220.0 * 2.0_f32.powf(SCALE[rng.index(SCALE.len())] / 12.0);
            }
        }
        let t = float(n % chord) / RATE;
        let mut sample = 0.0;
        for note in notes {
            for h in 1..=6_u16 {
                let h = f32::from(h);
                sample += (TAU * note * h * t).sin() / h;
            }
        }
        sample *= (-3.0 * t).exp();
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

/// Eileen's voice from the speakers: a minute, looped.
fn eileen() -> Vec<f32> {
    voice(1, (170.0, 250.0), 60.0, 0.1)
}

/// A spoken command, two seconds, as loud at the microphone as Eileen's voice is at the speaker.
fn command() -> Vec<f32> {
    voice(7, (110.0, 160.0), 2.0, 0.08)
}

/// The music bed the PC plays.
fn music_bed() -> Bed {
    bed(music(3, 60.0, 0.1), BedSource::Speakers)
}

/// The speech-video bed the PC plays: a man talking over quiet music.
fn video_bed() -> Bed {
    let talk = voice(11, (90.0, 150.0), 60.0, 0.09);
    let under = music(12, 60.0, 0.03);
    bed(talk.iter().zip(&under).map(|(a, b)| a + b).collect(), BedSource::Speakers)
}

fn bed(samples: Vec<f32>, from: BedSource) -> Bed {
    Bed { clip: WavClip::new(MONO_16K, samples).unwrap(), gain: 1.0, from }
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

/// The capture side: the canceller and the energy detector, fed as the component will feed
/// them, from the loopback's and the speaker's buffers of the same time.
struct Listener {
    canceller: EchoCanceller,
    detector: EnergyDetector,
    own: [f32; CHUNK],
    loopback: Option<[f32; CHUNK]>,
    window: Vec<f32>,
    /// The microphone's samples of the same window.
    window_in: Vec<f32>,
    window_other: bool,
    /// Each window's level at the microphone and after the canceller, in dBFS.
    levels: Vec<(f32, f32)>,
    chunks: Vec<Measured>,
    /// When each onset's window ended.
    onsets: Vec<Duration>,
    /// The window of each onset, an index into `levels`.
    onset_windows: Vec<usize>,
}

impl Listener {
    fn hear(&mut self, buffer: &[f32], at: Duration) {
        let mic: &[f32; CHUNK] = buffer.try_into().unwrap();
        let loopback = self.loopback.take();
        let own = std::mem::replace(&mut self.own, [0.0; CHUNK]);
        let mut out = [0.0; CHUNK];
        let report = self.canceller.process(mic, loopback.as_ref(), &own, &mut out).unwrap();
        self.chunks.push(Measured { mic: mean_square(mic), out: mean_square(&out), report });

        self.window.extend_from_slice(&out);
        self.window_in.extend_from_slice(mic);
        self.window_other |= report.flags.other_audio;
        if self.window.len() >= WINDOW {
            let window: [f32; WINDOW] = self.window[..WINDOW].try_into().unwrap();
            let level_in = db(mean_square(&self.window_in[..WINDOW]));
            self.levels.push((level_in, db(mean_square(&window))));
            if self.detector.window(&window, self.window_other).onset {
                self.onsets.push(at + ms(10));
                self.onset_windows.push(self.levels.len() - 1);
            }
            self.window.drain(..WINDOW);
            self.window_in.drain(..WINDOW);
            self.window_other = report.flags.other_audio && !self.window.is_empty();
        }
    }
}

type Shared = Arc<Mutex<Listener>>;

/// What a room run heard.
struct Heard {
    chunks: Vec<Measured>,
    onsets: Vec<Duration>,
    levels: Vec<(f32, f32)>,
    onset_windows: Vec<usize>,
}

impl Heard {
    /// Each onset's time with the levels, at the microphone and after the canceller, of the
    /// windows that made it, for a failure's message.
    fn onset_levels(&self) -> String {
        let onsets = self.onsets.iter().zip(&self.onset_windows).map(|(at, &window)| {
            let levels: Vec<String> = self.levels[window.saturating_sub(3)..=window]
                .iter()
                .map(|(mic, out)| format!("{mic:.1}/{out:.1}"))
                .collect();
            format!("{at:?}: {} dBFS", levels.join(" "))
        });
        onsets.collect::<Vec<_>>().join("; ")
    }

    /// The onsets from `from` on.
    fn onsets_from(&self, from: Duration) -> Vec<Duration> {
        self.onsets.iter().copied().filter(|&at| at >= from).collect()
    }

    /// The echo return loss enhancement over `chunks`, in dB.
    fn erle(&self, chunks: std::ops::Range<usize>) -> f32 {
        let chunks = &self.chunks[chunks];
        let mic: f32 = chunks.iter().map(|chunk| chunk.mic).sum();
        let out: f32 = chunks.iter().map(|chunk| chunk.out).sum();
        db(mic / out.max(1e-30))
    }

    fn references(&self, chunks: std::ops::Range<usize>) -> Vec<Reference> {
        self.chunks[chunks].iter().map(|chunk| chunk.report.reference).collect()
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
        detector: EnergyDetector::new(),
        own: [0.0; CHUNK],
        loopback: None,
        window: Vec::with_capacity(2 * WINDOW),
        window_in: Vec::with_capacity(2 * WINDOW),
        window_other: false,
        levels: Vec::new(),
        chunks: Vec::new(),
        onsets: Vec::new(),
        onset_windows: Vec::new(),
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
        onsets: std::mem::take(&mut listener.onsets),
        levels: std::mem::take(&mut listener.levels),
        onset_windows: std::mem::take(&mut listener.onset_windows),
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
        shared.lock().unwrap().loopback = Some(buffer.try_into().unwrap());
    })
}

fn microphone(shared: &Shared) -> CaptureCallback {
    let shared = Arc::clone(shared);
    Box::new(move |buffer: &[f32], info: CaptureInfo| {
        shared.lock().unwrap().hear(buffer, info.captured_at);
    })
}

/// The playback flag each chunk of `voice` should carry: it sounded in the chunk or the 19
/// before it.
fn expected_playback(voice: &[f32], chunks: usize) -> Vec<bool> {
    let sounded: Vec<bool> =
        (0..chunks).map(|k| mean_square(&voice[k * CHUNK..(k + 1) * CHUNK]) >= 1e-6).collect();
    (0..chunks).map(|k| sounded[k.saturating_sub(19)..=k].iter().any(|&s| s)).collect()
}

// AEC3 against the loopback.

#[tokio::test(start_paused = true)]
async fn eileen_speaking_is_cancelled_past_the_erle_floor_against_the_loopback() {
    let voice = eileen();
    let scene = Scene { eileen: Some(voice.clone()), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    let references = heard.references(0..1_200);
    assert!(references.iter().all(|&r| r == Reference::Loopback), "the loopback throughout");
    assert_eq!(
        heard.playback()[..1_200].to_vec(),
        expected_playback(&voice, 1_200),
        "playback while she sounds"
    );
    assert!(heard.other_audio().iter().all(|&o| !o), "her own voice is not other audio");
}

#[tokio::test(start_paused = true)]
async fn a_music_bed_from_the_speakers_is_cancelled_and_sets_other_audio() {
    let scene = Scene { bed: Some(music_bed()), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    let references = heard.references(0..1_200);
    assert!(references.iter().all(|&r| r == Reference::Loopback), "the loopback throughout");
    let other = heard.other_audio();
    assert!(other[..19].iter().all(|&o| !o), "not before 200 ms of it");
    assert!(other[19..].iter().all(|&o| o), "other audio from 200 ms on");
    assert!(heard.playback().iter().all(|&p| !p), "Eileen played nothing");
}

#[tokio::test(start_paused = true)]
async fn the_speech_video_bed_from_the_speakers_is_cancelled() {
    let scene = Scene { bed: Some(video_bed()), loopback: true, ..Scene::default() };
    let heard = listen(scene, 12).await;

    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
    assert!(heard.other_audio()[100..].iter().all(|&o| o), "other audio, past its pauses");
}

#[tokio::test(start_paused = true)]
async fn a_bed_in_the_room_is_not_other_audio_and_leaves_aec3_bypassed() {
    let video = video_bed();
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
    assert!(kept >= -3.0, "the command kept {kept:.1} dB of its energy");
    let first = heard.onsets.first().copied();
    assert!(first.is_some_and(|at| (ms(1_000)..=ms(1_250)).contains(&at)), "onset at {first:?}");
}

#[tokio::test(start_paused = true)]
async fn a_command_over_eileen_keeps_its_energy_and_the_detector_fires_on_its_onset() {
    let spoken = command();
    let scene = Scene {
        eileen: Some(eileen()),
        near: Some(after(6.0, &spoken)),
        loopback: true,
        ..Scene::default()
    };
    let heard = listen(scene, 9).await;

    // The command's own energy at the microphone, apart from the echo under it.
    let near: f32 = spoken.chunks(CHUNK).map(mean_square).sum();
    let out: f32 = heard.chunks[600..800].iter().map(|chunk| chunk.out).sum();
    let kept = db(out / near);
    assert!(kept >= -6.0, "the command kept {kept:.1} dB of its energy over Eileen");
    let first = heard.onsets.first().copied();
    assert!(
        first.is_some_and(|at| (ms(6_000)..=ms(6_250)).contains(&at)),
        "no onset from Eileen's echo, then one at the command: {}",
        heard.onset_levels()
    );
}

// The reference switch.

/// A chunk of noise at about -20 dBFS.
fn noise(rng: &mut Rng) -> [f32; CHUNK] {
    std::array::from_fn(|_| 0.17 * rng.signed())
}

/// Runs one chunk of silence at the microphone with this loopback and own PCM.
fn reference_of(
    canceller: &mut EchoCanceller,
    loopback: Option<&[f32; CHUNK]>,
    own: &[f32; CHUNK],
) -> Reference {
    let mut out = [0.0; CHUNK];
    canceller.process(&[0.0; CHUNK], loopback, own, &mut out).unwrap().reference
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
fn own_pcm_is_the_reference_once_the_loopback_is_silent_for_500_ms_or_gone() {
    let mut rng = Rng::new(6);
    let silence = [0.0; CHUNK];
    let mut canceller = EchoCanceller::new();
    for _ in 0..100 {
        let (looped, own) = (noise(&mut rng), noise(&mut rng));
        assert_eq!(reference_of(&mut canceller, Some(&looped), &own), Reference::Loopback);
    }
    // A loopback that falls silent while Eileen speaks: own PCM from the 50th silent chunk.
    let switched: Vec<Reference> =
        (0..60).map(|_| reference_of(&mut canceller, Some(&silence), &noise(&mut rng))).collect();
    let mut expected = vec![Reference::Loopback; 49];
    expected.resize(60, Reference::OwnPcm);
    assert_eq!(switched, expected, "49 chunks on the silent loopback, then own PCM");

    // No loopback at all: own PCM at once, and bypassed 500 ms after she stops.
    let chunk = noise(&mut rng);
    assert_eq!(reference_of(&mut canceller, None, &chunk), Reference::OwnPcm);
    let stopped: Vec<Reference> =
        (0..60).map(|_| reference_of(&mut canceller, None, &silence)).collect();
    let mut expected = vec![Reference::OwnPcm; 49];
    expected.resize(60, Reference::Bypassed);
    assert_eq!(stopped, expected, "own PCM through its echo tail, then bypassed");
}

#[tokio::test(start_paused = true)]
async fn without_a_loopback_eileen_is_cancelled_against_her_own_pcm() {
    let scene = Scene { eileen: Some(eileen()), ..Scene::default() };
    let heard = listen(scene, 12).await;

    let references = heard.references(0..1_200);
    assert!(references.iter().all(|&r| r == Reference::OwnPcm), "own PCM throughout");
    let erle = heard.erle(CONVERGED..1_200);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB, under the {ERLE_FLOOR_DB} dB floor");
}

#[tokio::test(start_paused = true)]
async fn a_loopback_that_stops_hands_over_to_own_pcm_and_the_echo_stays_cancelled() {
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
    let erle = heard.erle(800..900);
    assert!(erle >= ERLE_FLOOR_DB, "ERLE {erle:.1} dB in the first second after the spell");
}

// The energy detector over the beds.

/// When AEC3 has converged: the ERLE floors are measured from here, and so are false onsets.
/// Before it, 2.2 s into Eileen over music, the canceller took only about 5 dB off her echo and
/// the bed, and the detector fired once; AEC3 learns the room once after the core starts, since
/// the bypass keeps its state.
const CONVERGED_AT: Duration = ms(3_000);

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_in_ten_minutes_of_eileen_over_the_music_bed() {
    let scene = Scene {
        eileen: Some(eileen()),
        bed: Some(music_bed()),
        loopback: true,
        ..Scene::default()
    };
    let heard = listen(scene, 603).await;
    let late = heard.onsets_from(CONVERGED_AT);
    assert!(late.is_empty(), "false onsets over music: {}", heard.onset_levels());
}

#[tokio::test(start_paused = true)]
async fn no_barge_in_onset_in_ten_minutes_of_eileen_over_the_speech_video_bed() {
    let scene = Scene {
        eileen: Some(eileen()),
        bed: Some(video_bed()),
        loopback: true,
        ..Scene::default()
    };
    let heard = listen(scene, 603).await;
    let late = heard.onsets_from(CONVERGED_AT);
    assert!(late.is_empty(), "false onsets over the video: {}", heard.onset_levels());
}

// The callback's budget.

/// sonora 0.2.0 allocates and frees as it processes, converged or not, so the canceller must not
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

    assert!(on_loopback.count_total > 0, "sonora allocates: {on_loopback:?}");
    for counted in [on_loopback, on_own, bypassed] {
        assert!(counted.bytes_max < 64 * 1024, "at most 64 KiB live at once: {counted:?}");
    }
}

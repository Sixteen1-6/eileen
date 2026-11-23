//! Echo cancellation and the capture flags (BUILD.md section 2.8), on the speech pipe's 16 kHz
//! mono, one 10 ms chunk at a time.
//!
//! [`EchoCanceller`] runs sonora 0.2.0's AEC3 with its noise suppression and high-pass filter.
//! Its far-end reference is the loopback, the mix the speakers play: Eileen's voice and any
//! video or music beside her, so whatever the PC plays is cancelled. Eileen's own PCM, what the
//! render callback handed the device, is the reference instead while no loopback stream runs, or
//! once her PCM has sounded for 50 ms (five chunks) since the loopback last did: a loopback that
//! runs but does not carry her (it broke, or the default endpoint changed under it) would leave
//! AEC3 cancelling against silence. The 50 ms covers the loopback lagging her stream by a
//! buffer or two. Once the reference and her PCM have both been silent for 500 ms, AEC3 is
//! bypassed and only the high-pass filter and noise suppression run, which keeps idle cost low;
//! a chunk in which either sounds again brings AEC3 back at once. "Silent" is a chunk under
//! -60 dBFS, and "sounding" a chunk at or over it.
//!
//! Two sonora pipelines carry this, one with AEC3 and one without. The bypass runs the second and
//! leaves the first untouched, so AEC3 keeps its estimate of the echo path through an idle spell
//! and cancels the first words Eileen speaks after one; switching AEC3 off and on in one pipeline
//! would reset it, and leak echo while it learned the room again.
//!
//! AEC3 learns the room from the reference it hears: until it has run against 3 s of sounding
//! reference, since it started or since its reference last changed between the loopback and her
//! PCM (whose delays to the microphone differ), the report says it is learning, and the energy
//! fallback counts no speech in that time, since what AEC3 leaves of the echo then is as loud as
//! a voice.
//!
//! Each chunk also yields the capture flags the speech frames carry (decision D81), each true of
//! the chunk's own time:
//!
//! - `playback` while Eileen's own PCM for the chunk sounds. Her echo still reaches the
//!   microphone for a while after, which the canceller handles itself: AEC3 keeps running
//!   against its reference for 500 ms after the reference and her PCM fall silent.
//! - `other_audio` while the loopback carries energy her own PCM does not explain: over the last
//!   100 ms, more than twice the energy she played (3 dB), a window long enough to cover the
//!   loopback lagging her stream by a buffer or two. It turns on after 200 ms of such energy and
//!   off after 1 s without, so a quiet bar of a song does not flip it. Other audio quieter than
//!   Eileen's own voice shows only in her pauses, which the hold bridges.
//!
//! The devices' samples are untrusted: a NaN or infinite sample in the microphone, the loopback or
//! her PCM, or one past 24 dB over full scale, which no device delivers as sound, is replaced
//! with silence before any of it reaches sonora or the flags, and the report says so. AEC3's
//! state is never reset, so one such sample reaching its filter would spoil it for the rest of
//! the process.
//!
//! The 50 ms allowance assumes the loopback, once the component has placed it on the
//! microphone's clock, runs at most 40 ms behind her PCM. Further behind, each reply after a
//! pause would hand the reference to her PCM and back, and each change restarts AEC3's learning
//! time, in which the energy fallback counts no speech; the device part measures the real lag.
//!
//! The capture side calls [`EchoCanceller::process`] once per 10 ms of microphone audio, with the
//! loopback and the own PCM of the same time, each placed by its buffer's time on its device's
//! clock. sonora allocates and frees as it runs AEC3, converged too (more than once a chunk on
//! average, `tests` pins it), so the canceller cannot run inside an audio callback, which
//! allocates nothing: the callbacks hand their 16 kHz chunks on, and the component part runs the
//! canceller where the ring's reader runs.

use sonora::config::{EchoCanceller as Aec3, HighPassFilter, NoiseSuppression};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::ring::CaptureFlags;

/// The rate the canceller runs at: the speech pipe's.
pub(crate) const RATE_HZ: u32 = 16_000;

/// Samples in one chunk: 10 ms at 16 kHz, sonora's unit of work.
pub(crate) const CHUNK: usize = 160;

/// [`CHUNK`] as a divisor.
#[expect(clippy::cast_precision_loss, reason = "160 is exact in f32")]
const CHUNK_LEN: f32 = CHUNK as f32;

/// Chunks of silence after which a reference counts as gone: 500 ms.
pub(crate) const SILENT_CHUNKS: u32 = 50;

/// Chunks in which Eileen's PCM sounds while the loopback does not, since it last did, after
/// which her PCM is the reference: 50 ms, past a loopback a buffer or two behind her stream.
pub(crate) const MISSING_CHUNKS: u32 = 5;

/// Chunks of sounding reference after which AEC3 has learned the room: 3 s.
pub(crate) const LEARNING_CHUNKS: u32 = 300;

/// The largest sample taken as sound: 24 dB over full scale. A float device stream can go a
/// little past 1.0; nothing near this is audio.
const MAX_SAMPLE: f32 = 16.0;

/// A chunk under this mean square is silent: -60 dBFS.
const SILENT_MEAN_SQUARE: f32 = 1e-6;

/// Chunks the other-audio comparison sums: 100 ms.
const OTHER_WINDOW_CHUNKS: usize = 10;

/// The least summed energy of a sounding loopback over that window: silent chunks' worth.
#[expect(clippy::cast_precision_loss, reason = "10 is exact in f32")]
const OTHER_WINDOW_SILENT: f32 = SILENT_MEAN_SQUARE * OTHER_WINDOW_CHUNKS as f32;

/// How much more energy than Eileen played the loopback must carry to count as other audio.
const OTHER_RATIO: f32 = 2.0;

/// Chunks of unexplained loopback energy in a row that turn the other-audio flag on: 200 ms.
const OTHER_ON_CHUNKS: u32 = 20;

/// Chunks without it in a row that turn the flag off: 1 s.
const OTHER_OFF_CHUNKS: u32 = 100;

/// The far-end reference a chunk was cancelled against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reference {
    /// The loopback: everything the speakers play.
    Loopback,
    /// Eileen's own PCM, while the loopback does not carry her or does not run.
    OwnPcm,
    /// The reference and her PCM were silent for 500 ms: AEC3 did not run, only the high-pass
    /// filter and noise suppression.
    Bypassed,
}

/// What one chunk was processed against, and the flags of its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkReport {
    /// The reference used.
    pub(crate) reference: Reference,
    /// AEC3 ran and has not yet heard 3 s of sounding reference since it started or since its
    /// reference changed: what it leaves of the echo can be as loud as a voice.
    pub(crate) learning: bool,
    /// A NaN, infinite or far too large input sample was replaced with silence.
    pub(crate) repaired: bool,
    /// The capture flags.
    pub(crate) flags: CaptureFlags,
}

/// A chunk sonora refused. The canceller's formats are fixed, so this never happens; the
/// output then holds the microphone's samples unprocessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("echo cancellation refused a chunk")]
pub(crate) struct AecError(#[source] sonora::Error);

/// The echo canceller and the capture flags. See the module doc.
pub(crate) struct EchoCanceller {
    /// High-pass filter, AEC3 and noise suppression.
    full: AudioProcessing,
    /// High-pass filter and noise suppression, for the bypass.
    light: AudioProcessing,
    /// Where sonora writes the render side's copy, which nothing reads.
    render_out: [f32; CHUNK],
    /// The inputs with their non-finite samples replaced, silence for a loopback that does not
    /// run.
    mic: [f32; CHUNK],
    loopback: [f32; CHUNK],
    own: [f32; CHUNK],
    /// Chunks since the loopback last sounded, counting a chunk with no loopback.
    loopback_quiet: u32,
    /// Chunks since Eileen's own PCM last sounded.
    own_quiet: u32,
    /// Chunks in which her PCM sounded since the loopback last did.
    missing: u32,
    /// The reference AEC3 last ran against.
    last_reference: Option<Reference>,
    /// Chunks of sounding reference AEC3 heard since it started or since its reference changed,
    /// up to [`LEARNING_CHUNKS`].
    learned: u32,
    other: OtherAudio,
}

impl EchoCanceller {
    /// A canceller that has heard nothing yet: bypassed until a reference sounds.
    pub(crate) fn new() -> Self {
        let mono = StreamConfig::new(RATE_HZ, 1);
        let pipeline = |aec3: Option<Aec3>| {
            let config = Config {
                high_pass_filter: Some(HighPassFilter::default()),
                echo_canceller: aec3,
                noise_suppression: Some(NoiseSuppression::default()),
                ..Config::default()
            };
            AudioProcessing::builder()
                .config(config)
                .capture_config(mono)
                .render_config(mono)
                .build()
        };
        Self {
            full: pipeline(Some(Aec3::default())),
            light: pipeline(None),
            render_out: [0.0; CHUNK],
            mic: [0.0; CHUNK],
            loopback: [0.0; CHUNK],
            own: [0.0; CHUNK],
            loopback_quiet: SILENT_CHUNKS,
            own_quiet: SILENT_CHUNKS,
            missing: 0,
            last_reference: None,
            learned: 0,
            other: OtherAudio::new(),
        }
    }

    /// Cancels the echo in one chunk of microphone audio into `out`. `loopback` is the loopback's
    /// chunk of the same time, and `own` what Eileen's playback handed the device for that time,
    /// silence while she is quiet. `None` means no loopback stream runs, and makes own PCM the
    /// reference at once: the caller lines the loopback's buffers up with the microphone's and
    /// passes `None` only while no loopback stream runs, never for a buffer that is late.
    pub(crate) fn process(
        &mut self,
        mic: &[f32; CHUNK],
        loopback: Option<&[f32; CHUNK]>,
        own: &[f32; CHUNK],
        out: &mut [f32; CHUNK],
    ) -> Result<ChunkReport, AecError> {
        let mut repaired = finite_into(mic, &mut self.mic);
        repaired |= finite_into(own, &mut self.own);
        let running = loopback.is_some();
        if let Some(chunk) = loopback {
            repaired |= finite_into(chunk, &mut self.loopback);
        } else {
            self.loopback = [0.0; CHUNK];
        }

        let loopback_energy = mean_square(&self.loopback);
        let own_energy = mean_square(&self.own);
        let loopback_sounds = loopback_energy >= SILENT_MEAN_SQUARE;
        let own_sounds = own_energy >= SILENT_MEAN_SQUARE;
        self.loopback_quiet = next_quiet(self.loopback_quiet, loopback_sounds);
        self.own_quiet = next_quiet(self.own_quiet, own_sounds);
        self.missing =
            if loopback_sounds { 0 } else { self.missing.saturating_add(u32::from(own_sounds)) };
        let flags = CaptureFlags {
            playback: own_sounds,
            other_audio: self.other.update(loopback_energy, own_energy),
        };

        let carries_her = running && self.missing < MISSING_CHUNKS;
        let reference = if carries_her
            && (self.loopback_quiet < SILENT_CHUNKS || self.own_quiet < SILENT_CHUNKS)
        {
            Reference::Loopback
        } else if self.own_quiet < SILENT_CHUNKS {
            Reference::OwnPcm
        } else {
            Reference::Bypassed
        };

        let processed = if reference == Reference::Bypassed {
            self.light.process_capture_f32(&[self.mic.as_slice()], &mut [out.as_mut_slice()])
        } else {
            let (far, sounds) = if reference == Reference::Loopback {
                (&self.loopback, loopback_sounds)
            } else {
                (&self.own, own_sounds)
            };
            if self.last_reference != Some(reference) {
                self.last_reference = Some(reference);
                self.learned = 0;
            }
            if sounds {
                self.learned = self.learned.saturating_add(1).min(LEARNING_CHUNKS);
            }
            self.full
                .process_render_f32(&[far.as_slice()], &mut [self.render_out.as_mut_slice()])
                .and_then(|()| {
                    self.full.process_capture_f32(&[self.mic.as_slice()], &mut [out.as_mut_slice()])
                })
        };
        if let Err(error) = processed {
            out.copy_from_slice(&self.mic);
            return Err(AecError(error));
        }
        let learning = reference != Reference::Bypassed && self.learned < LEARNING_CHUNKS;
        Ok(ChunkReport { reference, learning, repaired, flags })
    }
}

impl std::fmt::Debug for EchoCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoCanceller")
            .field("loopback_quiet", &self.loopback_quiet)
            .field("own_quiet", &self.own_quiet)
            .field("missing", &self.missing)
            .field("last_reference", &self.last_reference)
            .field("learned", &self.learned)
            .field("other", &self.other)
            .finish_non_exhaustive()
    }
}

/// The other-audio flag: loopback energy that Eileen's own PCM does not explain, with its
/// hysteresis. See the module doc.
#[derive(Debug, Clone)]
struct OtherAudio {
    /// The last chunks' energies, loopback and own, oldest overwritten first.
    window: [(f32, f32); OTHER_WINDOW_CHUNKS],
    next: usize,
    /// Chunks in a row with unexplained energy, and without it.
    unexplained: u32,
    explained: u32,
    playing: bool,
}

impl OtherAudio {
    const fn new() -> Self {
        Self {
            window: [(0.0, 0.0); OTHER_WINDOW_CHUNKS],
            next: 0,
            unexplained: 0,
            explained: 0,
            playing: false,
        }
    }

    /// Takes one chunk's energies and returns the flag for it.
    fn update(&mut self, loopback: f32, own: f32) -> bool {
        if let Some(slot) = self.window.get_mut(self.next) {
            *slot = (loopback, own);
        }
        self.next = (self.next + 1) % OTHER_WINDOW_CHUNKS;
        let (loopback, own) = self
            .window
            .iter()
            .fold((0.0, 0.0), |(l, o), &(chunk_l, chunk_o)| (l + chunk_l, o + chunk_o));
        let sounding = loopback >= OTHER_WINDOW_SILENT;
        if sounding && loopback > OTHER_RATIO * own {
            self.unexplained = self.unexplained.saturating_add(1);
            self.explained = 0;
        } else {
            self.explained = self.explained.saturating_add(1);
            self.unexplained = 0;
        }
        if self.unexplained >= OTHER_ON_CHUNKS {
            self.playing = true;
        } else if self.explained >= OTHER_OFF_CHUNKS {
            self.playing = false;
        }
        self.playing
    }
}

/// A chunk's mean square.
fn mean_square(chunk: &[f32; CHUNK]) -> f32 {
    chunk.iter().map(|sample| sample * sample).sum::<f32>() / CHUNK_LEN
}

/// The count of silent chunks after one that `sounds` or not.
const fn next_quiet(quiet: u32, sounds: bool) -> u32 {
    if sounds { 0 } else { quiet.saturating_add(1) }
}

/// Copies `from` into `to` with each NaN, infinite or far too large sample made silence, and
/// says whether there was one.
fn finite_into(from: &[f32; CHUNK], to: &mut [f32; CHUNK]) -> bool {
    let mut repaired = false;
    for (sample, &value) in to.iter_mut().zip(from) {
        if value.abs() <= MAX_SAMPLE {
            *sample = value;
        } else {
            *sample = 0.0;
            repaired = true;
        }
    }
    repaired
}

#[cfg(test)]
mod tests;

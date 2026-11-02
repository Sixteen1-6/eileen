//! Echo cancellation and the capture flags (BUILD.md section 2.8), on the speech pipe's 16 kHz
//! mono, one 10 ms chunk at a time.
//!
//! [`EchoCanceller`] runs sonora 0.2.0's AEC3 with its noise suppression and high-pass filter.
//! Its far-end reference is the loopback, the mix the speakers play: Eileen's voice and any
//! video or music beside her, so whatever the PC plays is cancelled. Once the loopback has been
//! silent for 500 ms, or is not running, Eileen's own PCM, what the render callback handed the
//! device, is the reference instead; and once both have been silent for 500 ms, AEC3 is bypassed
//! and only the high-pass filter and noise suppression run, which keeps idle cost low. A chunk in
//! which either sounds again brings its reference back at once. "Silent" is a chunk under
//! -60 dBFS.
//!
//! Two sonora pipelines carry this, one with AEC3 and one without. The bypass runs the second and
//! leaves the first untouched, so AEC3 keeps its estimate of the echo path through an idle spell
//! and cancels the first words Eileen speaks after one; switching AEC3 off and on in one pipeline
//! would reset it, and leak echo while it learned the room again.
//!
//! Each chunk also yields the capture flags the speech frames carry (decision D81):
//!
//! - `playback` while Eileen's own PCM sounded in this chunk or the 190 ms before it, since her
//!   echo still reaches the microphone after the speaker falls silent.
//! - `other_audio` while the loopback carries energy her own PCM does not explain: over the last
//!   100 ms, more than twice the energy she played (3 dB), a window long enough to cover the
//!   loopback lagging her stream by a buffer or two. It turns on after 200 ms of such energy and
//!   off after 1 s without, so a quiet bar of a song does not flip it. Other audio quieter than
//!   Eileen's own voice shows only in her pauses, which the hold bridges.
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

/// A chunk under this mean square is silent: -60 dBFS.
const SILENT_MEAN_SQUARE: f32 = 1e-6;

/// Chunks the playback flag outlasts Eileen's own PCM, this one included: 200 ms.
const PLAYBACK_HOLD_CHUNKS: u32 = 20;

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
    /// Eileen's own PCM, while the loopback is silent or not running.
    OwnPcm,
    /// Neither sounded for 500 ms: AEC3 did not run, only the high-pass filter and noise
    /// suppression.
    Bypassed,
}

/// What one chunk was processed against, and the flags of its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChunkReport {
    /// The reference used.
    pub(crate) reference: Reference,
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
    /// Chunks since the loopback last sounded, counting a chunk with no loopback.
    loopback_quiet: u32,
    /// Chunks since Eileen's own PCM last sounded.
    own_quiet: u32,
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
            loopback_quiet: SILENT_CHUNKS,
            own_quiet: SILENT_CHUNKS,
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
        let loopback_energy = loopback.map_or(0.0, mean_square);
        let own_energy = mean_square(own);
        self.loopback_quiet = next_quiet(self.loopback_quiet, loopback_energy);
        self.own_quiet = next_quiet(self.own_quiet, own_energy);
        let flags = CaptureFlags {
            playback: self.own_quiet < PLAYBACK_HOLD_CHUNKS,
            other_audio: self.other.update(loopback_energy, own_energy),
        };

        let (reference, far) = match loopback {
            Some(chunk) if self.loopback_quiet < SILENT_CHUNKS => (Reference::Loopback, chunk),
            _ if self.own_quiet < SILENT_CHUNKS => (Reference::OwnPcm, own),
            _ => (Reference::Bypassed, own),
        };
        let processed = if reference == Reference::Bypassed {
            self.light.process_capture_f32(&[mic.as_slice()], &mut [out.as_mut_slice()])
        } else {
            self.full
                .process_render_f32(&[far.as_slice()], &mut [self.render_out.as_mut_slice()])
                .and_then(|()| {
                    self.full.process_capture_f32(&[mic.as_slice()], &mut [out.as_mut_slice()])
                })
        };
        if let Err(error) = processed {
            out.copy_from_slice(mic);
            return Err(AecError(error));
        }
        Ok(ChunkReport { reference, flags })
    }
}

impl std::fmt::Debug for EchoCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EchoCanceller")
            .field("loopback_quiet", &self.loopback_quiet)
            .field("own_quiet", &self.own_quiet)
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

/// The count of silent chunks after one of `energy`.
const fn next_quiet(quiet: u32, energy: f32) -> u32 {
    if energy >= SILENT_MEAN_SQUARE { 0 } else { quiet.saturating_add(1) }
}

#[cfg(test)]
mod tests;

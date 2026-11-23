//! The energy barge-in fallback (BUILD.md section 2.8): while the speech process is not
//! connected, no VAD judges barge-in windows, so an energy detector on the echo-cancelled
//! microphone stands in for it. The component part feeds it and acts on what it reports.
//!
//! [`WindowAssembler`] gathers the canceller's 10 ms chunks into 32 ms windows, the VAD's and the
//! speech frames' 512 samples at 16 kHz, each paired with the microphone's samples it came from
//! and carrying the flags of every chunk it spans, a chunk split between two windows counting
//! in both; it hands each window to its [`EnergyDetector`]. The detector calls a window speech
//! when its level stands 12 dB over the room's noise floor and is at least -40 dBFS, the quietest
//! voice near the PC it is meant to hear, and, while AEC3 ran for it, lost at most 6 dB to the
//! canceller: a voice in the room passes AEC3 nearly whole, while a window it took more from was
//! mostly echo, whose remainder can still be loud when AEC3 briefly loses the room. Bypassed,
//! only noise suppression runs, which takes more than that from the first syllables after a
//! silence as it learns them, so the test does not apply. While AEC3 is still learning the room
//! (its first 3 s of sounding reference, `aec`) no window is speech: what it leaves of the echo
//! then is as loud as a voice, and no level tells the two apart. While other audio plays the
//! margin is 18 dB: section 2.8 makes barge-in stricter then (two VAD windows over its
//! threshold), and a bed's residual after AEC3 is less steady than a room's noise. The floor
//! drops to any quieter window at once and rises 5 dB a second otherwise, so a steady noise stops
//! counting within seconds while a spoken word, louder than the floor by far more than it rises
//! meanwhile, still counts. On noise-suppressed audio in a quiet room the floor sits low and the
//! -40 dBFS minimum decides; over a noisy room the margin does. It reports an onset at the third
//! speech window in a row (96 ms), the window at which section 2.8's barge-in pauses playback,
//! and once per run of speech windows.
//!
//! What remains after AEC3 of Eileen's voice and of a bed the PC plays stays under these
//! thresholds; `aec::tests` checks over ten minutes of each bed, in runs that each start from a
//! fresh canceller and overlap by its learning time, that it never fires, and that it fires on a
//! near-end command alone, over Eileen and over each bed.
//!
//! A detector's floor is the room's while it is fed, so the component makes a fresh assembler
//! each time the speech process disconnects: a floor kept from before would be stale. Nothing
//! here allocates.

use eileen_contracts::speech::AUDIO_FRAME_SAMPLES;

use crate::aec::{CHUNK, ChunkReport, Reference};

/// Samples in one window: 32 ms at 16 kHz, a speech frame.
pub(crate) const WINDOW: usize = AUDIO_FRAME_SAMPLES;

/// [`WINDOW`] as a divisor.
#[expect(clippy::cast_precision_loss, reason = "512 is exact in f32")]
const WINDOW_LEN: f32 = WINDOW as f32;

/// Speech windows in a row that make an onset: 96 ms, where barge-in pauses playback.
pub(crate) const ONSET_WINDOWS: u32 = 3;

/// How far over the floor a window must be to count as speech, in dB.
const MARGIN_DB: f32 = 12.0;

/// The margin while other audio plays.
const MARGIN_OTHER_AUDIO_DB: f32 = 18.0;

/// The quietest window that counts as speech, in dBFS.
const MIN_SPEECH_DBFS: f32 = -40.0;

/// The most the canceller may take from a window that counts as speech, in dB.
const MAX_CANCELLED_DB: f32 = 6.0;

/// How fast the floor rises, in dB per window: 5 dB a second.
const FLOOR_RISE_DB: f32 = 5.0 * 0.032;

/// The level of a window of digital silence, in dBFS, so the logarithm stays finite.
const SILENCE_DBFS: f32 = -120.0;

/// What the canceller did over a window, from the least cautious reading to the most: the
/// window takes the last of these that any of its chunks had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Canceller {
    /// AEC3 was bypassed: only the high-pass filter and noise suppression ran.
    Bypassed,
    /// AEC3 ran, and has learned the room.
    Converged,
    /// AEC3 ran, and is still learning the room.
    Learning,
}

impl Canceller {
    /// What the canceller did for one chunk.
    pub(crate) const fn of(report: ChunkReport) -> Self {
        match report.reference {
            Reference::Bypassed => Self::Bypassed,
            Reference::Loopback | Reference::OwnPcm if report.learning => Self::Learning,
            Reference::Loopback | Reference::OwnPcm => Self::Converged,
        }
    }
}

/// What the detector made of one window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EnergyWindow {
    /// The window counts as speech.
    pub(crate) speech: bool,
    /// Speech windows in a row, this one included; 0 when it is not speech.
    pub(crate) run: u32,
    /// This window is the run's [`ONSET_WINDOWS`]th: the user started to talk.
    pub(crate) onset: bool,
    /// The window's level after the canceller, and at the microphone, in dBFS.
    pub(crate) level_dbfs: f32,
    pub(crate) heard_dbfs: f32,
    /// How many dB the window's level fell short of the nearest threshold it missed, 0 or less
    /// when it is speech; infinite while AEC3 learns. Tests use it to show their margins.
    pub(crate) short_db: f32,
    /// The flags it was judged with.
    pub(crate) other_audio: bool,
    pub(crate) canceller: Canceller,
}

/// The energy barge-in detector. See the module doc.
#[derive(Debug, Clone, Default)]
pub(crate) struct EnergyDetector {
    /// The noise floor in dBFS, from the first window on.
    floor_db: Option<f32>,
    run: u32,
}

impl EnergyDetector {
    /// A detector that has heard nothing yet: its first window sets the floor.
    pub(crate) const fn new() -> Self {
        Self { floor_db: None, run: 0 }
    }

    /// Judges one window of echo-cancelled 16 kHz audio, `cancelled`, against the microphone's
    /// window it came from, `heard`. `other_audio` is the capture flag of the same time, and
    /// `aec3` what the canceller did over the window.
    pub(crate) fn window(
        &mut self,
        cancelled: &[f32; WINDOW],
        heard: &[f32; WINDOW],
        other_audio: bool,
        aec3: Canceller,
    ) -> EnergyWindow {
        let level = level_dbfs(cancelled);
        let heard_dbfs = level_dbfs(heard);
        let floor = match self.floor_db {
            Some(floor) => (floor + FLOOR_RISE_DB).min(level),
            None => level,
        };
        self.floor_db = Some(floor);
        let margin = if other_audio { MARGIN_OTHER_AUDIO_DB } else { MARGIN_DB };
        let loud_enough = MIN_SPEECH_DBFS.max(floor + margin);
        let needed = if aec3 == Canceller::Converged {
            loud_enough.max(heard_dbfs - MAX_CANCELLED_DB)
        } else {
            loud_enough
        };
        let short_db = if aec3 == Canceller::Learning { f32::INFINITY } else { needed - level };
        let speech = short_db <= 0.0;
        self.run = if speech { self.run.saturating_add(1) } else { 0 };
        EnergyWindow {
            speech,
            run: self.run,
            onset: self.run == ONSET_WINDOWS,
            level_dbfs: level,
            heard_dbfs,
            short_db,
            other_audio,
            canceller: aec3,
        }
    }
}

/// Gathers the canceller's chunks into windows and judges each. See the module doc.
#[derive(Debug, Clone)]
pub(crate) struct WindowAssembler {
    detector: EnergyDetector,
    /// The window being filled, after the canceller and at the microphone.
    cancelled: [f32; WINDOW],
    heard: [f32; WINDOW],
    filled: usize,
    /// The flags of the chunks it spans so far.
    other_audio: bool,
    canceller: Canceller,
}

impl WindowAssembler {
    /// An assembler with a fresh detector and an empty window.
    pub(crate) const fn new() -> Self {
        Self {
            detector: EnergyDetector::new(),
            cancelled: [0.0; WINDOW],
            heard: [0.0; WINDOW],
            filled: 0,
            other_audio: false,
            canceller: Canceller::Bypassed,
        }
    }

    /// Takes one chunk, `out` from the canceller and `mic` it came from, with the canceller's
    /// report for it, and returns the window it completes, if it completes one.
    pub(crate) fn push(
        &mut self,
        out: &[f32; CHUNK],
        mic: &[f32; CHUNK],
        report: ChunkReport,
    ) -> Option<EnergyWindow> {
        let canceller = Canceller::of(report);
        let other_audio = report.flags.other_audio;
        self.other_audio |= other_audio;
        self.canceller = self.canceller.max(canceller);

        let room = WINDOW - self.filled;
        let first = CHUNK.min(room);
        self.copy(out, mic, 0..first);
        if self.filled < WINDOW {
            return None;
        }
        let judged =
            self.detector.window(&self.cancelled, &self.heard, self.other_audio, self.canceller);
        self.filled = 0;
        self.other_audio = false;
        self.canceller = Canceller::Bypassed;
        if first < CHUNK {
            // The rest of the chunk opens the next window, which carries its flags too.
            self.other_audio = other_audio;
            self.canceller = canceller;
            self.copy(out, mic, first..CHUNK);
        }
        Some(judged)
    }

    /// Appends `range` of the chunk to the window.
    fn copy(&mut self, out: &[f32; CHUNK], mic: &[f32; CHUNK], range: std::ops::Range<usize>) {
        let len = range.len();
        let to = self.filled..self.filled + len;
        if let (Some(cancelled), Some(heard), Some(out), Some(mic)) = (
            self.cancelled.get_mut(to.clone()),
            self.heard.get_mut(to),
            out.get(range.clone()),
            mic.get(range),
        ) {
            cancelled.copy_from_slice(out);
            heard.copy_from_slice(mic);
        }
        self.filled += len;
    }
}

/// A window's level: its mean square in dB relative to full scale.
fn level_dbfs(samples: &[f32; WINDOW]) -> f32 {
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    let mean_square = sum / WINDOW_LEN;
    if mean_square > 0.0 { (10.0 * mean_square.log10()).max(SILENCE_DBFS) } else { SILENCE_DBFS }
}

#[cfg(test)]
mod tests;

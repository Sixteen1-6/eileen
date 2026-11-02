//! The energy barge-in fallback (BUILD.md section 2.8): while the speech process is not
//! connected, no VAD judges barge-in windows, so an energy detector on the echo-cancelled
//! microphone stands in for it. The component part feeds it and acts on what it reports.
//!
//! [`EnergyDetector`] takes 32 ms windows, the VAD's and the speech frames' 512 samples at
//! 16 kHz, and calls a window speech when its level stands 12 dB over the room's noise floor and
//! is at least -40 dBFS, the quietest voice near the PC it is meant to hear, and, while AEC3 ran
//! for it, lost at most 6 dB to the canceller: a voice in the room passes AEC3 nearly whole,
//! while a window it took more from was mostly echo, whose remainder can still be loud while
//! AEC3 converges or briefly loses the room. Bypassed, only noise suppression runs, which takes
//! more than that from the first syllables after a silence as it learns them, so the test does
//! not apply. While other audio plays the margin is 18 dB: section 2.8 makes
//! barge-in stricter then (two VAD windows over its threshold), and a bed's residual after AEC3
//! is less steady than a room's noise. The floor
//! drops to any quieter window at once and rises 5 dB a second otherwise, so a steady noise stops
//! counting within seconds while a spoken word, louder than the floor by far more than it rises
//! meanwhile, still counts. On noise-suppressed audio in a quiet room the floor sits low and the
//! -40 dBFS minimum decides; over a noisy room the margin does. It reports an onset at the third
//! speech window in a row (96 ms), the window at which section 2.8's barge-in pauses playback,
//! and once per run of speech windows.
//!
//! What remains after a converged AEC3 of Eileen's voice and of a bed the PC plays stays under
//! these thresholds; `aec::tests` checks over ten minutes of each bed, from 3 s on, that it never
//! fires, and that it fires on a near-end command. In AEC3's first seconds, before it has
//! learned the room, the echo can still reach them.
//!
//! A detector's floor is the room's while it is fed, so the component makes a fresh one each
//! time the speech process disconnects: a floor kept from before would be stale. Nothing here
//! allocates.

use eileen_contracts::speech::AUDIO_FRAME_SAMPLES;

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

/// What the detector made of one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnergyWindow {
    /// The window counts as speech.
    pub(crate) speech: bool,
    /// Speech windows in a row, this one included; 0 when it is not speech.
    pub(crate) run: u32,
    /// This window is the run's [`ONSET_WINDOWS`]th: the user started to talk.
    pub(crate) onset: bool,
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
    /// `aec3` whether AEC3 ran for any of the window's chunks, rather than the bypass.
    pub(crate) fn window(
        &mut self,
        cancelled: &[f32; WINDOW],
        heard: &[f32; WINDOW],
        other_audio: bool,
        aec3: bool,
    ) -> EnergyWindow {
        let level = level_dbfs(cancelled);
        let floor = match self.floor_db {
            Some(floor) => (floor + FLOOR_RISE_DB).min(level),
            None => level,
        };
        self.floor_db = Some(floor);
        let margin = if other_audio { MARGIN_OTHER_AUDIO_DB } else { MARGIN_DB };
        let speech = level >= MIN_SPEECH_DBFS
            && level >= floor + margin
            && (!aec3 || level >= level_dbfs(heard) - MAX_CANCELLED_DB);
        self.run = if speech { self.run.saturating_add(1) } else { 0 };
        EnergyWindow { speech, run: self.run, onset: self.run == ONSET_WINDOWS }
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

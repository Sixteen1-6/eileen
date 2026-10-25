//! Eileen's audio (BUILD.md sections 2.8 and 4.7): WASAPI capture, playback and loopback through
//! cpal at 48 kHz, echo cancellation with the loopback as the far-end reference, the 16 kHz frames
//! into the speech ring buffer, mute, duck, pause and playback accounting, and the test sources
//! and sinks. A stub until WP4 builds it.
//!
//! # Owns
//!
//! - The audio [`component`]: capture, render and loopback (cpal only under the Windows target
//!   table, the logic around it neutral), sonora's AEC, resampling and the ring buffer.
//! - `AudioSource` and `AudioSink` with `WavSource`, `CaptureSink` and `RoomMixer`, so the
//!   end-to-end tests run without a sound card.
//! - Samples-played accounting, the energy barge-in fallback and the audio heartbeat.
//!
//! # Does not own
//!
//! - The endpoints, MMCSS and the Communications category (`eileen-audio-win`), speech models
//!   (`eileen-speech`), or barge-in decisions (`eileen-core`).
//!
//! # Invariants
//!
//! - The capture callback allocates nothing and logs nothing (WP4 adds the counting allocator
//!   that checks it in debug builds).
//! - Mute stops the capture stream within one buffer (WP4 acceptance 9 tests it).
//! - The ring buffer is safe under concurrent use by the callback and its reader (WP4 adds its
//!   loom tests, under `cfg(loom)`).
//! - The stub accepts the audio family and does nothing (unit test
//!   `tests::the_stub_component_takes_the_audio_family`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (the component).
//! - May depend on `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`; `eileen-audio-win` and `cpal` under the Windows target table.
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-speech` (its own process) or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on any other `-win` or `-linux` crate, or
//!   on `eileen-audio-win` or `cpal` outside the Windows target table (check 15).
//! - Wire types: the speech pipe's PCM frames (`eileen-contracts::speech`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-audio` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-audio`).
//! - Layers: unit tests for the DSP, loom for the ring buffer, `RoomMixer` echo tests, and
//!   device tests on the owner's PC under `desktop.lock`.
//! - A bug here starts as Eileen hearing herself, or a word lost at a buffer boundary.
//!
//! # Traps
//!
//! - Echo cancellation is bypassed after 500 ms of loopback silence and Eileen's own PCM becomes
//!   the reference.
//!
//! # Logs and tracing
//!
//! - None yet, and never inside the audio callback; WP4 reports through the tick ring.
//!
//! # Do not change without a decision record
//!
//! - The sample rates and frame sizes of the speech pipe (48 kHz devices, 16 kHz 512-sample
//!   frames).

#![forbid(unsafe_code)]

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::ports::{Component, IdleComponent};

/// What the audio component reads at start. WP4 adds its fields (the source and sink, the
/// device choices).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioConfig;

/// The audio component. The stub accepts the audio family and does nothing.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "section 3.3.15's signature; AudioConfig has no fields until WP4"
)]
pub fn component(_cfg: &AudioConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("audio", &[EffectFamily::Audio]))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::EffectFamily;

    use super::{AudioConfig, component};

    #[test]
    fn the_stub_component_takes_the_audio_family() {
        let audio = component(&AudioConfig);
        assert_eq!(audio.name(), "audio", "its name");
        assert_eq!(audio.accepts(), [EffectFamily::Audio], "its family");
    }
}

//! Eileen's audio (BUILD.md sections 2.8 and 4.7): WASAPI capture, playback and loopback through
//! cpal at 48 kHz, echo cancellation with the loopback as the far-end reference, the 16 kHz frames
//! into the speech ring buffer and the core end of the speech pipe (decision D81), mute, duck,
//! pause and playback accounting, and the test sources and sinks. WP4 builds it in parts.
//!
//! # Owns
//!
//! - The audio [`component`]: capture, render and loopback (cpal only under the Windows target
//!   table, the logic around it neutral), sonora's AEC, resampling, the ring buffer, and every
//!   frame of the speech pipe, over connections taken from a [`SpeechTransport`].
//! - [`AudioSource`] and [`AudioSink`] with `WavSource`, `CaptureSink` and `RoomMixer`, and the
//!   in-memory speech transport (`fake`), so the end-to-end tests run without a sound card.
//! - Samples-played accounting, the energy barge-in fallback and the audio heartbeat.
//!
//! # Does not own
//!
//! - The endpoints, MMCSS and the Communications category (`eileen-audio-win`), speech models
//!   (`eileen-speech`), the speech pipe's transport (`eileen-runtime`), or barge-in decisions
//!   (`eileen-core`).
//!
//! # Invariants
//!
//! - The audio callbacks allocate, free and log nothing (WP4's tests check it in test builds with
//!   `allocation-counter`, a dev-dependency; product builds carry no check, decision D82).
//! - Mute stops the capture stream within one buffer (WP4 acceptance 9 tests it).
//! - Every accept, read and write on a speech connection has a deadline, and every queue of the
//!   memory transport a maximum (tests in `fake::tests`). A shipped binary never enables `fake`.
//! - The ring buffer is safe under concurrent use by the callback and its reader (WP4 adds its
//!   loom tests, under `cfg(loom)`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`, which builds the component and implements [`SpeechTransport`] over
//!   WP2's transport; WP2's tests use `fake`. Cross-crate items: [`SpeechTransport`],
//!   [`SpeechConnection`] and its halves and byte traits, [`AudioSource`], [`AudioSink`] and
//!   [`AudioConfig`]. The component never names the pipe, spawns a process or sees the token.
//! - May depend on `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx`,
//!   `eileen-trace`, `tokio` and `thiserror`; `eileen-audio-win` and `cpal` under the Windows
//!   target table. Today `eileen-rt` is used only by `fake`, so it is optional and the feature
//!   turns it on, and a dev-dependency for this crate's own tests.
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-speech` (its own process) or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on any other `-win` or `-linux` crate, or
//!   on `eileen-audio-win` or `cpal` outside the Windows target table (check 15).
//! - Wire types: the speech pipe's frames (`eileen-contracts::speech`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-audio`.
//! - Layers: unit tests for the DSP and the memory transport (paused tokio time), loom for the
//!   ring buffer, `RoomMixer` echo tests, and device tests on the owner's PC under `desktop.lock`.
//! - A bug here starts as Eileen hearing herself, or a word lost at a buffer boundary.
//!
//! # Traps
//!
//! - Echo cancellation is bypassed after 500 ms of loopback silence and Eileen's own PCM becomes
//!   the reference.
//! - A speech connection's halves need a tokio runtime with its timer; a failed write leaves the
//!   stream inside a frame, so the writer refuses every later write and the connection goes back.
//!
//! # Logs and tracing
//!
//! - None yet, and never inside the audio callback; WP4 reports through the tick ring.
//!
//! # Do not change without a decision record
//!
//! - The sample rates and frame sizes of the speech pipe (48 kHz devices, 16 kHz 512-sample
//!   frames), and who owns which part of the pipe (D81).

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::Arc;

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::ports::{Component, IdleComponent};

#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod stream;
pub mod transport;

pub use stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, StreamFormat,
};
pub use transport::{
    AcceptError, ByteRead, ByteWrite, ConnectionError, ConnectionId, SpeechConnection,
    SpeechReader, SpeechTransport, SpeechWriter,
};

/// What the audio component reads at start. Later parts of WP4's audio series add the source
/// and sink choices.
#[derive(Clone, Default)]
pub struct AudioConfig {
    /// The speech process's connections; `None` runs without the speech pipe, as the stub and
    /// tests that need no speech process do.
    pub speech: Option<Arc<dyn SpeechTransport>>,
}

impl fmt::Debug for AudioConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioConfig").field("speech", &self.speech.is_some()).finish()
    }
}

/// The audio component. The stub accepts the audio family and does nothing.
#[must_use]
pub fn component(_cfg: &AudioConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("audio", &[EffectFamily::Audio]))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::EffectFamily;

    use super::{AudioConfig, component};

    #[test]
    fn the_stub_component_takes_the_audio_family() {
        let audio = component(&AudioConfig::default());
        assert_eq!(audio.name(), "audio", "its name");
        assert_eq!(audio.accepts(), [EffectFamily::Audio], "its family");
    }
}

//! Eileen's audio (BUILD.md sections 2.8 and 4.7): WASAPI capture, playback and loopback through
//! cpal at the endpoint's own rate (see Traps), echo cancellation with the loopback as the far-end
//! reference, the 16 kHz frames into the speech ring buffer and the core end of the speech pipe
//! (decision D81), mute, duck, pause and playback accounting, and the test sources and sinks.
//! WP4 builds it in parts.
//!
//! # Owns
//!
//! - The audio [`component`]: capture, render and loopback (cpal only under the Windows target
//!   table, the logic around it neutral), sonora's AEC, resampling, the ring buffer, and every
//!   frame of the speech pipe, over connections taken from a [`SpeechTransport`].
//! - [`AudioSource`] and [`AudioSink`] with [`WavSource`], [`CaptureSink`] and [`RoomMixer`] on
//!   paced virtual time ([`Pace`]), and the in-memory speech transport (`fake`), so the end-to-end
//!   tests run without a sound card.
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
//! - The speech ring's capture callback and its reader share only an SPSC handoff (`rtrb`) and
//!   two atomics, the next `seq` and the count of frames lost. Loom models in `ring::concurrency`
//!   explore every order of those two under `cfg(loom)`; rtrb's own tests cover the handoff, and
//!   a test on real threads in the same module drives a 4-frame handoff through 10 000 frames.
//!   Its capture side allocates nothing (`allocation_counter::measure` in `ring::tests`).
//! - Every resampler conversion meets BUILD.md section 6.6's bar (capture flat within 0.1 dB to
//!   7 kHz and 60 dB down from 9 kHz; synthesis flat within 0.1 dB to 10 kHz, its images 60 dB
//!   down from 14 kHz), measured through `realfft` in `resample::tests`, which also check that
//!   buffers of any size lose no sample and that the downmix and conversions allocate nothing.
//! - The ring keeps every frame the speech process has not confirmed, up to 180 s, numbered from
//!   the core's start (decision D81); a mute empties it, so nothing captured before a mute is
//!   written after it (`ring::tests`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`, which builds the component and implements [`SpeechTransport`] over
//!   WP2's transport; WP2's tests use `fake`. Cross-crate items: [`SpeechTransport`],
//!   [`SpeechConnection`] and its halves and byte traits, [`AudioSource`], [`AudioSink`],
//!   [`AudioConfig`], and the test audio `eileen run --audio-source wav:<file> --audio-sink
//!   capture:<file>` selects and other crates' tests use: [`WavClip`], [`WavSource`],
//!   [`CaptureSink`], [`RoomMixer`] and its halves, and [`Pace`]. The component never names the
//!   pipe, spawns a process or sees the token.
//! - May depend on `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx`,
//!   `eileen-trace`, `tokio` and `thiserror`; `eileen-audio-win` and `cpal` under the Windows
//!   target table; `rtrb` for the ring's handoff, `hound` for WAV files, and `loom` only under
//!   `cfg(loom)`. `realfft` (the resamplers' measurements) and `allocation-counter` (decision
//!   D82) are dev-dependencies, never outside `[dev-dependencies]`.
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-speech` (its own process) or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on any other `-win` or `-linux` crate, or
//!   on `eileen-audio-win` or `cpal` outside the Windows target table (check 15).
//! - Wire types: the speech pipe's frames (`eileen-contracts::speech`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-audio`.
//! - Layers: unit tests for the DSP, the ring's rules and the memory transport (paused tokio
//!   time), loom for the ring's shared atomics (`RUSTFLAGS="--cfg loom"`, with `--lib
//!   ring::concurrency`; no CI lane runs it yet, section 4.6 plans one; without the flag the same
//!   models run once on a real thread), the test sources and sinks under paused tokio time
//!   (`wav::tests`, `room::tests`: a WAV round trip, the pacing, and the room's delay and
//!   attenuation on an impulse), and device tests on the owner's PC under `desktop.lock`.
//! - A bug here starts as Eileen hearing herself, or a word lost at a buffer boundary.
//!
//! # Traps
//!
//! - Echo cancellation is bypassed after 500 ms of loopback silence and Eileen's own PCM becomes
//!   the reference.
//! - The ring's break flag follows from the numbers: a frame carries it when its `seq` is not one
//!   more than the frame written before it on the connection, so a frame lost at the handoff, one
//!   dropped from a full store and one written to a connection that ended all show as gaps. Only a
//!   mute skips no number, so the first frame pulled after one is marked in the store, and keeps
//!   the flag when a later connection replays it.
//! - A frame's replayed flag is its age on the capture clock, which the ring estimates from the
//!   newest frame pulled plus 32 ms per number taken since; a mute forgets that clock, since the
//!   next source may start its own at zero.
//! - cpal opens WASAPI capture and loopback streams only at the endpoint's own rate, so a 44.1 kHz
//!   endpoint's microphone and echo reference both take Eileen's 160:441; only render streams get
//!   Windows' converter, and synthesis keeps its own 147:80. Other rates are unsupported for now.
//! - A speech connection's halves need a tokio runtime with its timer; a failed write leaves the
//!   stream inside a frame, so the writer refuses every later write and the connection goes back.
//! - The test sources and sinks pace their buffers on the tokio clock and take their buffers'
//!   times from sample positions, so under paused tokio time a test's audio moves only when the
//!   test waits. Their callbacks run under the device's lock: a callback that stops its own
//!   stream deadlocks.
//!
//! # Logs and tracing
//!
//! - None yet, and never inside the audio callback; WP4 reports through the tick ring.
//!
//! # Do not change without a decision record
//!
//! - The sample rates and frame sizes of the speech pipe (devices at 48 kHz or 44.1 kHz, 16 kHz
//!   512-sample frames), and who owns which part of the pipe (D81).

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::Arc;

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::ports::{Component, IdleComponent};

#[cfg(any(test, feature = "fake"))]
pub mod fake;
// The component takes the ring in a later part of the audio series; until then only its tests
// use it, and under loom only the concurrency models, which leave the rules' items unused.
#[cfg_attr(
    any(not(test), loom),
    expect(dead_code, reason = "the component wires the ring in a later part")
)]
mod ring;
// The component converts its capture and synthesis with these in a later part; until then only
// their tests use them.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the component wires the resamplers in a later part")
)]
mod resample;

pub mod pace;
pub mod room;
pub mod stream;
#[cfg(test)]
mod testing;
pub mod transport;
pub mod wav;

pub use pace::Pace;
pub use room::{Bed, RoomConfig, RoomMixer, RoomSink, RoomSource};
pub use stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, StreamFormat,
};
pub use transport::{
    AcceptError, ByteRead, ByteWrite, ConnectionError, ConnectionId, SpeechConnection,
    SpeechReader, SpeechTransport, SpeechWriter,
};
pub use wav::{CaptureSink, WavClip, WavSource};

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

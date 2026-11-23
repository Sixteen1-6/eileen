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
//! - Mute stops the capture stream within one buffer (WP4 acceptance 9 tests it), even while a
//!   write on the speech pipe waits: the component takes effects during every wait on the pipe,
//!   and after a mute writes no audio frame it had not begun (`tests/it/speech_pipe.rs`).
//! - Every accept, read and write on a speech connection has a deadline, and every queue of the
//!   memory transport a maximum (tests in `fake::tests`). A shipped binary never enables `fake`.
//! - On each new speech connection the component sends the latest `SetMode` and `SetHotwords`
//!   before any audio, then the commands held while none was up, then the ring's unconfirmed
//!   frames, then live ones; so a late connect, a restart and a reload each recognise a turn once,
//!   and an open turn again under a new turn ID (`tests/it/speech_pipe.rs`).
//! - The component never restarts anything: a broken connection, or one whose frame the codec
//!   refused, goes back to the transport, which only closes it (`tests/it/speech_pipe.rs`).
//! - Commands held while no connection is up are bounded (64), each one dropped past that logged
//!   (`tests/it/speech_pipe.rs`).
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
//! - Playback takes a duck or unduck in the next output callback (simulator invariant 2), stops a
//!   paused utterance at a sample and resumes it from exactly that sample, and never plays audio
//!   of a stopped utterance (`playback::tests`). Its queues are bounded: a PCM frame that does
//!   not fit the sample or marker queue is refused and logged, and so is a new utterance past
//!   the live limit, an early pause past the ones it remembers and an event the player reports
//!   itself past the event limit; its render side allocates nothing
//!   (`allocation_counter::measure` in the same tests).
//! - Echo cancellation runs on the speech pipe's 16 kHz mono in 10 ms chunks, against the
//!   loopback while it carries Eileen, her own PCM once she has sounded for 50 ms without it or
//!   while none runs, and bypassed (high-pass filter and noise suppression only) once the
//!   reference and her PCM have both been silent for 500 ms; the bypass leaves AEC3's state
//!   alone, so the next reply is cancelled from its first words (`aec::tests`). sonora allocates
//!   as it runs AEC3, so echo cancellation runs off the audio callbacks (`aec::tests` pins it).
//!   No NaN or infinite sample from a device reaches sonora (`aec::tests`).
//! - The energy barge-in fallback counts no speech while AEC3 learns the room (its first 3 s of
//!   sounding reference, and again after its reference changes), reports one onset per run of
//!   speech windows, at the third (`energy::tests`), and allocates nothing.
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`, which builds the component and implements [`SpeechTransport`] over
//!   WP2's transport; WP2's tests use `fake`. Cross-crate items: [`SpeechTransport`],
//!   [`SpeechConnection`] and its halves and byte traits, [`AudioSource`], [`AudioSink`],
//!   [`AudioConfig`] with its [`SharedSource`], [`SetupError`], and the test audio
//!   `eileen run --audio-source wav:<file> --audio-sink capture:<file>` selects and other crates'
//!   tests use: [`WavClip`], [`WavSource`], [`CaptureSink`], [`RoomMixer`] with its speaker,
//!   microphone and loopback, [`RoomConfig`], [`Bed`] and [`BedSource`], and [`Pace`]. The
//!   component never names the pipe, spawns a process or sees the token.
//! - May depend on `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx`,
//!   `eileen-trace`, `tracing`, `tokio` and `thiserror`; `eileen-audio-win` and `cpal` under the
//!   Windows target table; `rtrb` for the ring's and playback's handoffs, `hound` for WAV files,
//!   `sonora` for AEC3, noise suppression and the high-pass filter, and `loom` only under
//!   `cfg(loom)`. `realfft` (the resamplers' measurements) and
//!   `allocation-counter` (decision D82) are dev-dependencies, never outside
//!   `[dev-dependencies]`.
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-speech` (its own process) or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on any other `-win` or `-linux` crate, or
//!   on `eileen-audio-win` or `cpal` outside the Windows target table (check 15).
//! - Wire types: the speech pipe's frames (`eileen-contracts::speech`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-audio`.
//! - Layers: integration tests of the component's end of the speech pipe in
//!   `tests/it/speech_pipe.rs` (WP4 acceptance 15: a `WavSource` microphone and a scripted
//!   stand-in for the speech process over the memory transport, on paused tokio time: a late
//!   connect, a kill and a reload each recognising every command once, the latest mode and
//!   hotwords first, an overrun's break flag, mutes, the heartbeat and confirmation, a refused
//!   frame, the answers while no connection is up and the held commands' maximum, a write that
//!   misses its deadline, a command whose own write fails, the transport shutting down, a mute
//!   while a write waits, one `Pong` for a read's `Ping`s, and the logs of a clean stop, a
//!   dropped command, refused inputs and an overrun); unit tests for the capture framing
//!   (`capture::tests`, a buffer that ends inside a device frame among them, and the callback
//!   inside `allocation_counter::measure`), the DSP, the ring's rules and the memory transport
//!   (paused tokio time), loom for the ring's shared atomics (`RUSTFLAGS="--cfg loom"`, with
//!   `--lib ring::concurrency`; no CI lane runs it yet, section 4.6 plans one; without the flag
//!   the same models run once on a real thread), the test sources and sinks under paused tokio
//!   time (`wav::tests`, `room::tests`: a WAV round trip, the pacing, the read and capture limits,
//!   the room's delay and attenuation on an impulse, its loopback, and a room stopped and started
//!   again), playback over a `CaptureSink` under paused tokio time (`playback::tests`: each
//!   command, compared bit for bit with the interpolator over the whole clause), echo
//!   cancellation and the energy fallback over a 16 kHz `RoomMixer` with synthetic voices, music
//!   and a speech video, made bit for bit the same on every platform, with the loopback two
//!   chunks behind her stream (`aec::tests`: an ERLE floor against the loopback and against own
//!   PCM, after a handover once AEC3 has learned her PCM's delay afresh, and after a NaN, a
//!   near-end command's energy kept alone, over Eileen and over each bed, the reference switch
//!   to the chunk, the learning time, the flags,
//!   and no false onset in ten minutes of each bed, counted from a fresh canceller, in four runs
//!   of 153 s that each stay under the test profiles' time limit), the window assembly and the
//!   detector's rules (`energy::tests`), and device tests on the owner's PC under
//!   `desktop.lock`.
//! - A bug here starts as Eileen hearing herself, or a word lost at a buffer boundary.
//!
//! # Traps
//!
//! - Echo cancellation is bypassed after 500 ms of loopback silence and Eileen's own PCM becomes
//!   the reference: once her PCM has sounded for 50 ms since the loopback last did (or no
//!   loopback runs), the loopback does not carry her, so her PCM is the reference; once the
//!   reference and her PCM have both been silent for 500 ms, AEC3 is bypassed. While the
//!   loopback works the rules agree, since her voice is in it a buffer or two late. Starting
//!   from the bypass, AEC3 runs again in the first chunk that either sounds in.
//! - The playback flag is true of its own chunk: her PCM for it is at or over -60 dBFS. Her echo
//!   tail is the canceller's business, not the flag's.
//! - A device can hand over a NaN or infinite float. The canceller makes such a sample silence
//!   before sonora sees it, since AEC3 is never reset and one bad sample would spoil it for the
//!   rest of the process.
//! - The other-audio flag compares energies over 100 ms, not samples, since the loopback lags
//!   Eileen's own stream by a buffer or more on a real endpoint; so other audio quieter than her
//!   voice shows only in her pauses, which its 1 s hold bridges.
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
//! - A read on the speech connection that misses its deadline is not a failure, and the component
//!   closes nothing for silence: the core counts missed heartbeats (each `Pong` is one) and
//!   decides a restart, and during a reload it counts none until its load deadline passes.
//! - The component's pending accept, read and effect are owned futures kept across its loop's
//!   turns, so none is dropped part way; a read in flight when a write fails is awaited within
//!   its deadline, so the two halves rejoin before the connection goes back.
//! - A mute never waits on the speech pipe. While a write, or a closing connection's last read,
//!   waits, the component keeps taking effects: a mute stops capture at once and drops any
//!   `SetCapture` parked before it, and any other effect is parked, at most 64, for the loop to
//!   serve in order after the wait. Audio goes a frame at a time under one deadline for the
//!   batch, so after a mute only the frame already begun is finished. A new wait on the pipe
//!   goes through the same race.
//! - A speech command frame's `span` is 0: an effect's envelope carries no span id, so the
//!   speech process cannot open `tts.synth` under the core's `speak` span until the plan says
//!   how one reaches this component.
//! - A capture buffer that ends inside a device frame loses that partial frame only: the framer
//!   takes whole device frames before it chunks, since the downmix refuses a chunk that is not
//!   whole frames.
//! - Tests turn on `fake` for the whole workspace's build: under resolver 3 a dev-dependency's
//!   features join the one library unit whenever test targets are built, and `eileen-cli` links
//!   that unit, so clippy over `--all-targets` never lints the shipped configuration. #174 amends
//!   section 4.6 to add the one check of it, a clippy run over libraries and binaries alone.
//! - Inputs the component sends carry zero for `seq`, `at` and `mono`: the runtime's input queue
//!   gives each its place in the replay order and its times.
//! - The test sources and sinks pace their buffers on the tokio clock and take their buffers'
//!   times from sample positions, so under paused tokio time a test's audio moves only when the
//!   test waits. Their callbacks run on a tokio task under the device's lock: a callback that
//!   stops its own stream deadlocks. A `CaptureSink`'s task runs from its first stream until the
//!   file is completed, writing silence while no stream runs, so no restart writes a long gap in
//!   one step.
//! - Playback's `samples_played` counts the clause's own 24 kHz samples, as `ClauseReady.samples`
//!   does, not the device's; and a paused utterance with audio left holds every utterance behind
//!   it, so the core stops one before it speaks the next.
//! - Playback's `Drained` means every sample queued so far for the utterance has played (the
//!   queue ran dry, or a later utterance began), as the core reads it, not that the reply is
//!   over: it comes once per dry spell, so more than once while synthesis is behind, and the
//!   utterance keeps taking audio after it. `SpeakDone` ends nothing here; only the core knows
//!   when a reply is closed.
//!
//! # Logs and tracing
//!
//! - `channel.full` from the speech link, each with its running total as `times`, as every
//!   other emitter gives it: frames the ring dropped (`audio.speech.ring`, with the store's
//!   capacity) and inputs the core's queue refused (`core.inputs`, with the capacity the queue
//!   gave with its last refusal), each in the once-a-second report when its total changed; and
//!   a command held while no connection was up dropped past the queue's maximum
//!   (`audio.speech.held`), as it happens. A connection handed back logs its id and reason at
//!   warn, or at info when the component is stopping, a connection made at info, and each accept
//!   wait that ends with none at debug.
//! - `channel.full` when playback drops something for want of room, each `channel` with its own
//!   `times`: a frame refused by the sample queue (`audio.playback`) or the marker queue
//!   (`audio.playback.markers`), a new utterance past the live limit
//!   (`audio.playback.utterances`), an early pause past the ones it remembers
//!   (`audio.playback.waiting`), a `Stopped` the player reports itself past the event limit
//!   (`audio.playback.reported`), and events its render side lost (`audio.playback.events`); all
//!   logged from the component's side, never inside the audio callback. WP4 reports the
//!   callbacks through the tick ring.
//!
//! # Do not change without a decision record
//!
//! - The sample rates and frame sizes of the speech pipe (devices at 48 kHz or 44.1 kHz, 16 kHz
//!   512-sample frames), and who owns which part of the pipe (D81).

#![forbid(unsafe_code)]

use std::fmt;
use std::sync::{Arc, Mutex};

use eileen_contracts::ports::Component;

// The component cancels the echo in its capture path with this, and runs the energy fallback
// while the speech process is not connected, in a later part; until then only their tests use
// them.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the component wires echo cancellation in a later part")
)]
mod aec;
mod capture;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the component wires the energy fallback in a later part")
)]
mod energy;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
mod link;
mod ring;
// The capture callback converts with these; synthesis takes its conversion when the component
// wires playback in a later part, so until then some items only tests use.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the component wires synthesis's conversion in a later part")
)]
mod resample;
// The component plays the speech process's PCM through this in a later part; until then only its
// tests use it.
#[cfg_attr(not(test), expect(dead_code, reason = "the component wires playback in a later part"))]
mod playback;

pub mod pace;
pub mod room;
pub mod stream;
#[cfg(test)]
mod testing;
pub mod transport;
pub mod wav;

pub use pace::Pace;
pub use room::{Bed, BedSource, RoomConfig, RoomLoopback, RoomMixer, RoomSink, RoomSource};
pub use stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, SetupError, StreamFormat,
};
pub use transport::{
    AcceptError, ByteRead, ByteWrite, ConnectionError, ConnectionId, SpeechConnection,
    SpeechReader, SpeechTransport, SpeechWriter,
};
pub use wav::{CaptureSink, WavClip, WavSource};

/// A capture source the component and its creator share: the component starts and stops its
/// stream (mute stops it), and a test keeps a handle to the same source.
pub type SharedSource = Arc<Mutex<dyn AudioSource>>;

/// What the audio component reads at start. Later parts of WP4's audio series add the playback
/// sink and the loopback.
#[derive(Clone, Default)]
pub struct AudioConfig {
    /// The speech process's connections; `None` runs without the speech pipe, as the stub and
    /// tests that need no speech process do.
    pub speech: Option<Arc<dyn SpeechTransport>>,
    /// Where the microphone's audio comes from: the endpoint, or a test's [`WavSource`] or
    /// [`RoomMixer`] microphone. `None` captures nothing.
    pub capture: Option<SharedSource>,
}

impl fmt::Debug for AudioConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioConfig")
            .field("speech", &self.speech.is_some())
            .field("capture", &self.capture.is_some())
            .finish()
    }
}

/// The audio component. It takes the audio and speech effect families: it captures into the
/// speech ring from its start, and serves the speech pipe over the connections `cfg.speech`
/// gives.
#[must_use]
pub fn component(cfg: &AudioConfig) -> Box<dyn Component> {
    Box::new(link::AudioComponent::new(cfg.speech.clone(), cfg.capture.clone()))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::EffectFamily;

    use super::{AudioConfig, component};

    #[test]
    fn the_component_takes_the_audio_and_speech_families() {
        let audio = component(&AudioConfig::default());
        assert_eq!(audio.name(), "audio", "its name");
        assert_eq!(audio.accepts(), [EffectFamily::Audio, EffectFamily::Speech], "its families");
    }
}

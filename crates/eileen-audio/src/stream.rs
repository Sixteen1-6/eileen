//! Where captured audio comes from and where playback goes (BUILD.md section 4.7): production
//! opens the WASAPI endpoints through cpal, and tests use a WAV file, a capture to a WAV and a
//! mixer that feeds playback back into the input, so the end-to-end tests run without a sound
//! card: [`WavSource`](crate::WavSource), [`CaptureSink`](crate::CaptureSink) and
//! [`RoomMixer`](crate::RoomMixer). A later part of WP4's audio series builds the cpal ones.
//!
//! A source or sink owns its timing, as cpal's streams do, and calls the component's callback
//! once per buffer: a device on its own audio thread, a test source or sink on a tokio task while
//! it holds the device's lock. The callback is the component's code on the audio thread, so it
//! allocates, frees, blocks and logs nothing (this crate's invariant).
//!
//! The source or sink also says when each buffer was captured or will play, on its own clock, as
//! cpal's callback infos do, so the callback never reads a clock of its own. The component needs
//! those times for an audio frame's replayed flag (written more than 250 ms after capture,
//! decision D81) and for playback accounting. A frame's age is the newest buffer's time less its
//! own, so the component never maps one clock onto another. A device's times come from its
//! stream's timestamps; a test source or sink derives them from its sample position, so both stay
//! deterministic however fast the test paces its buffers and under paused tokio time.

use std::io;
use std::time::Duration;

/// The shape of a stream's samples: interleaved `f32` at this rate and channel count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamFormat {
    /// Samples per second per channel (48 000 at the endpoints of section 2.8).
    pub sample_rate_hz: u32,
    /// Channels per frame.
    pub channels: u16,
}

/// When a captured buffer was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureInfo {
    /// When its first sample reached the source, on the source's clock: time since the source's
    /// first stream started, which a later stream of the same source continues, so a mute and
    /// unmute do not reset it.
    pub captured_at: Duration,
}

/// When a playback buffer will play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderInfo {
    /// When its first sample will leave the sink, on the sink's clock: time since the sink's
    /// first stream started, which a later stream of the same sink continues.
    pub plays_at: Duration,
}

/// What a source calls with each captured buffer of interleaved samples and when it was
/// captured.
pub type CaptureCallback = Box<dyn FnMut(&[f32], CaptureInfo) + Send + 'static>;

/// What a sink calls to have each playback buffer of interleaved samples filled, with when it
/// will play.
pub type RenderCallback = Box<dyn FnMut(&mut [f32], RenderInfo) + Send + 'static>;

/// Captured audio: the microphone, or a test's WAV file or room mixer.
pub trait AudioSource: Send {
    /// The format its buffers come in.
    fn format(&self) -> StreamFormat;

    /// Starts capturing, calling `on_buffer` with each buffer until the returned stream stops.
    /// It may start again after that stream stopped (mute stops capture, and unmuting starts it).
    fn start(&mut self, on_buffer: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError>;
}

/// Played audio: the speakers, or a test's capture to a WAV file or room mixer.
pub trait AudioSink: Send {
    /// The format it plays.
    fn format(&self) -> StreamFormat;

    /// Starts playing, calling `fill` for each buffer until the returned stream stops.
    fn start(&mut self, fill: RenderCallback) -> Result<Box<dyn AudioStream>, AudioError>;
}

/// A running stream. Dropping it stops it as [`AudioStream::stop`] does.
pub trait AudioStream: Send {
    /// Stops the stream. Once this returns its callback is never called again, so a mute takes
    /// effect within the buffer in progress.
    fn stop(self: Box<Self>);
}

/// Why a stream did not start.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// There is no device of the kind asked for.
    #[error("no audio device is available")]
    NoDevice,
    /// The device or file does not offer this format.
    #[error("the audio format is not supported: {0:?}")]
    Unsupported(StreamFormat),
    /// The device or file failed.
    #[error("the audio stream failed")]
    Io(#[source] io::Error),
    /// A source or sink was asked for something it cannot do.
    #[error("the audio source or sink cannot do this: {0}")]
    Setup(#[source] SetupError),
}

/// What a source or sink cannot do, as [`AudioError::Setup`] carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SetupError {
    /// A second stream was started while one runs.
    #[error("a stream of this source or sink is already running")]
    AlreadyRunning,
    /// A test source or sink was started outside a tokio runtime, which paces its buffers.
    #[error("a test source or sink needs a tokio runtime")]
    NoRuntime,
    /// A clip's samples do not fill a whole number of frames.
    #[error("a clip's samples must fill whole frames")]
    PartialFrame,
    /// A WAV clip is longer than 20 minutes, or holds more samples than 20 minutes of 48 kHz
    /// stereo.
    #[error("a WAV clip is longer than a test clip may be")]
    ClipTooLong,
    /// A capture reached the most samples a WAV file can hold.
    #[error("a capture is longer than a WAV file holds")]
    CaptureTooLong,
    /// A test room's gain is not a finite, non-negative number.
    #[error("a room's gains are finite and not negative")]
    BadGain,
    /// A test room's echo delay is over a second.
    #[error("a room's echo delay is at most 1 s")]
    EchoTooLong,
}

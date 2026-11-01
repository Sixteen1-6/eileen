//! Where captured audio comes from and where playback goes (BUILD.md section 4.7): production
//! opens the WASAPI endpoints through cpal, and tests use a WAV file, a capture to a WAV and a
//! mixer that feeds playback back into the input, so the end-to-end tests run without a sound
//! card. Later parts of WP4's audio series build those implementations.
//!
//! A source or sink owns its thread and its timing, as cpal's streams do, and calls the
//! component's callback once per buffer. The callback is the component's code on the audio
//! thread, so it allocates, frees, blocks and logs nothing (this crate's invariant).

use std::io;

/// The shape of a stream's samples: interleaved `f32` at this rate and channel count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamFormat {
    /// Samples per second per channel (48 000 at the endpoints of section 2.8).
    pub sample_rate_hz: u32,
    /// Channels per frame.
    pub channels: u16,
}

/// What a source calls with each captured buffer of interleaved samples.
pub type CaptureCallback = Box<dyn FnMut(&[f32]) + Send + 'static>;

/// What a sink calls to have each playback buffer of interleaved samples filled.
pub type RenderCallback = Box<dyn FnMut(&mut [f32]) + Send + 'static>;

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
}

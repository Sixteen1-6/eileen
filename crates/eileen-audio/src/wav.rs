//! WAV files as audio (BUILD.md section 4.7): [`WavClip`], a file's samples in memory;
//! [`WavSource`], a source that captures a clip as a microphone would; and [`CaptureSink`], a
//! sink that writes what Eileen plays to a WAV file. `eileen run --audio-source wav:<file>
//! --audio-sink capture:<file>` selects the two, and the end-to-end tests run on them without a
//! sound card. Both run on paced virtual time ([`Pace`]).
//!
//! Files are read and written through `hound`. The clips the tests use are synthetic, made in the
//! tests; a recorded fixture needs its licence recorded beside it.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eileen_stdx::num;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

use crate::pace::{self, Pace, Paced, Timeline};
use crate::stream::{
    AudioError, AudioSink, AudioSource, AudioStream, CaptureCallback, CaptureInfo, RenderCallback,
    RenderInfo, StreamFormat,
};

/// The longest clip read into memory: 20 minutes, more than the 10 minutes of video WP4's echo
/// test plays (section 6.6), and at most 460 MB for 48 kHz stereo.
const MAX_CLIP_SECONDS: u32 = 20 * 60;

/// Audio held in memory: interleaved `f32` samples at full scale 1.0, and their format.
#[derive(Clone)]
pub struct WavClip {
    format: StreamFormat,
    samples: Arc<[f32]>,
}

impl WavClip {
    /// A clip of these interleaved samples, which must hold whole frames.
    pub fn new(format: StreamFormat, samples: Vec<f32>) -> Result<Self, AudioError> {
        if format.channels == 0 || format.sample_rate_hz == 0 {
            return Err(AudioError::Unsupported(format));
        }
        if !samples.len().is_multiple_of(usize::from(format.channels)) {
            return Err(AudioError::Setup("a clip's samples must hold whole frames"));
        }
        Ok(Self { format, samples: samples.into() })
    }

    /// Reads a WAV file of integer samples of up to 32 bits or of 32-bit floats, at most
    /// 20 minutes long.
    pub fn read(path: &Path) -> Result<Self, AudioError> {
        let reader = WavReader::open(path).map_err(wav_error)?;
        let spec = reader.spec();
        let format = StreamFormat { sample_rate_hz: spec.sample_rate, channels: spec.channels };
        if reader.duration() > spec.sample_rate.saturating_mul(MAX_CLIP_SECONDS) {
            return Err(AudioError::Setup("a WAV clip is longer than 20 minutes"));
        }
        let samples = match spec.sample_format {
            SampleFormat::Float => reader.into_samples::<f32>().collect::<Result<Vec<_>, _>>(),
            SampleFormat::Int => {
                let scale = 0.5_f64.powi(i32::from(spec.bits_per_sample) - 1);
                reader
                    .into_samples::<i32>()
                    .map(|sample| sample.map(|value| narrow(f64::from(value) * scale)))
                    .collect::<Result<Vec<_>, _>>()
            }
        }
        .map_err(wav_error)?;
        Self::new(format, samples)
    }

    /// Writes the clip to a WAV file of 32-bit floats, which keeps every sample exactly.
    pub fn write(&self, path: &Path) -> Result<(), AudioError> {
        let mut writer = WavWriter::create(path, float_spec(self.format)).map_err(wav_error)?;
        for &sample in &*self.samples {
            writer.write_sample(sample).map_err(wav_error)?;
        }
        writer.finalize().map_err(wav_error)
    }

    /// The clip's format.
    #[must_use]
    pub const fn format(&self) -> StreamFormat {
        self.format
    }

    /// The clip's interleaved samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The clip's length.
    #[must_use]
    pub fn duration(&self) -> Duration {
        let frames = self.samples.len() / usize::from(self.format.channels);
        let nanos = u128::from(num::saturating_u64_from_usize(frames)) * 1_000_000_000
            / u128::from(self.format.sample_rate_hz);
        Duration::from_nanos(num::saturating_u64_from_u128(nanos))
    }
}

impl fmt::Debug for WavClip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WavClip")
            .field("format", &self.format)
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// A source that captures a WAV clip as a microphone in a room would: from the start of its
/// first stream, one 10 ms buffer per buffer's length of paced time, then silence once the clip
/// ends. A stream started again takes up where the room's time has got to, so the stretch of the
/// clip that played while it was stopped is never captured, as with a muted microphone.
pub struct WavSource {
    format: StreamFormat,
    state: Arc<Mutex<SourceState>>,
}

struct SourceState {
    timeline: Timeline,
    clip: WavClip,
    buffer: Vec<f32>,
    on_buffer: Option<CaptureCallback>,
    /// Which start the callback belongs to, so a stopped stream's handle never stops a later one.
    starts: u64,
}

impl WavSource {
    /// A source that captures the WAV file at `path`, at `pace`.
    pub fn open(path: &Path, pace: Pace) -> Result<Self, AudioError> {
        Self::new(WavClip::read(path)?, pace)
    }

    /// A source that captures `clip`, at `pace`. The clip's rate must be a whole number of
    /// hundreds of hertz, so it splits into 10 ms buffers.
    pub fn new(clip: WavClip, pace: Pace) -> Result<Self, AudioError> {
        let format = clip.format();
        let timeline = Timeline::new(format, pace)?;
        let buffer = vec![0.0; timeline.buffer_len()];
        let state = SourceState { timeline, clip, buffer, on_buffer: None, starts: 0 };
        Ok(Self { format, state: Arc::new(Mutex::new(state)) })
    }
}

impl fmt::Debug for WavSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WavSource").field("format", &self.format).finish_non_exhaustive()
    }
}

impl AudioSource for WavSource {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, on_buffer: CaptureCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut state = pace::lock(&self.state);
        if state.on_buffer.is_some() {
            return Err(AudioError::Setup("the WAV source is already capturing"));
        }
        pace::run("audio.wav_source", &self.state, &mut *state)?;
        state.on_buffer = Some(on_buffer);
        state.starts += 1;
        let start = state.starts;
        drop(state);
        Ok(Box::new(SourceStream { state: Arc::clone(&self.state), start }))
    }
}

impl Paced for SourceState {
    fn timeline(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    fn step(&mut self, position: u64) {
        let start = self.timeline.samples_in(position);
        let clip = self.clip.samples().get(start..).unwrap_or_default();
        let after_the_end = std::iter::repeat(0.0);
        for (out, sample) in self.buffer.iter_mut().zip(clip.iter().copied().chain(after_the_end)) {
            *out = sample;
        }
        if let Some(on_buffer) = self.on_buffer.as_mut() {
            on_buffer(&self.buffer, CaptureInfo { captured_at: self.timeline.time_of(position) });
        }
    }
}

/// A running [`WavSource`] stream.
struct SourceStream {
    state: Arc<Mutex<SourceState>>,
    start: u64,
}

impl AudioStream for SourceStream {
    fn stop(self: Box<Self>) {}
}

impl Drop for SourceStream {
    fn drop(&mut self) {
        let mut state = pace::lock(&self.state);
        if state.starts == self.start {
            state.timeline.halt();
            state.on_buffer = None;
        }
    }
}

/// A sink that writes what Eileen plays to a WAV file of 32-bit floats, one 10 ms buffer per
/// buffer's length of paced time. The file keeps the room's timeline: a stretch while no stream
/// ran is written as silence, so a sample's place in the file is its time on the sink's clock.
/// [`CaptureSink::finish`] completes the file; dropping the sink completes it too, without
/// reporting an error.
pub struct CaptureSink {
    format: StreamFormat,
    state: Arc<Mutex<SinkState>>,
}

struct SinkState {
    timeline: Timeline,
    writer: Option<WavWriter<BufWriter<File>>>,
    buffer: Vec<f32>,
    fill: Option<RenderCallback>,
    starts: u64,
    /// The frames the file holds.
    written: u64,
    /// The first write that failed; the file gets no more samples after it.
    failed: Option<hound::Error>,
}

impl CaptureSink {
    /// A sink of `format` that writes to a new WAV file at `path`, at `pace`. The rate must be a
    /// whole number of hundreds of hertz, so it splits into 10 ms buffers.
    pub fn create(path: &Path, format: StreamFormat, pace: Pace) -> Result<Self, AudioError> {
        let timeline = Timeline::new(format, pace)?;
        let writer = WavWriter::create(path, float_spec(format)).map_err(wav_error)?;
        let buffer = vec![0.0; timeline.buffer_len()];
        let state = SinkState {
            timeline,
            writer: Some(writer),
            buffer,
            fill: None,
            starts: 0,
            written: 0,
            failed: None,
        };
        Ok(Self { format, state: Arc::new(Mutex::new(state)) })
    }

    /// Stops any stream and completes the file. Returns the frames it holds, or the first write
    /// that failed.
    pub fn finish(self) -> Result<u64, AudioError> {
        let mut state = pace::lock(&self.state);
        state.timeline.halt();
        state.fill = None;
        let writer = state.writer.take();
        let failed = state.failed.take();
        let written = state.written;
        drop(state);
        if let Some(error) = failed {
            return Err(wav_error(error));
        }
        let Some(writer) = writer else {
            return Err(AudioError::Setup("the capture's file was already completed"));
        };
        writer.finalize().map_err(wav_error)?;
        Ok(written)
    }
}

impl fmt::Debug for CaptureSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureSink").field("format", &self.format).finish_non_exhaustive()
    }
}

impl Drop for CaptureSink {
    fn drop(&mut self) {
        let mut state = pace::lock(&self.state);
        state.timeline.halt();
        state.fill = None;
        // hound completes the file when its writer drops.
        drop(state.writer.take());
    }
}

impl AudioSink for CaptureSink {
    fn format(&self) -> StreamFormat {
        self.format
    }

    fn start(&mut self, fill: RenderCallback) -> Result<Box<dyn AudioStream>, AudioError> {
        let mut state = pace::lock(&self.state);
        if state.fill.is_some() {
            return Err(AudioError::Setup("the capture sink is already playing"));
        }
        // The file's gap is written with the next buffer, from `written`.
        pace::run("audio.capture_sink", &self.state, &mut *state)?;
        state.fill = Some(fill);
        state.starts += 1;
        let start = state.starts;
        drop(state);
        Ok(Box::new(SinkStream { state: Arc::clone(&self.state), start }))
    }
}

impl Paced for SinkState {
    fn timeline(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    fn step(&mut self, position: u64) {
        self.buffer.fill(0.0);
        if let Some(fill) = self.fill.as_mut() {
            fill(&mut self.buffer, RenderInfo { plays_at: self.timeline.time_of(position) });
        }
        let gap = self.timeline.samples_in(position.saturating_sub(self.written));
        self.written = position.saturating_add(self.timeline.buffer_frames());
        if self.failed.is_some() {
            return;
        }
        let Some(writer) = self.writer.as_mut() else { return };
        if let Err(error) = write_samples(writer, gap, &self.buffer) {
            self.failed = Some(error);
        }
    }
}

/// A running [`CaptureSink`] stream.
struct SinkStream {
    state: Arc<Mutex<SinkState>>,
    start: u64,
}

impl AudioStream for SinkStream {
    fn stop(self: Box<Self>) {}
}

impl Drop for SinkStream {
    fn drop(&mut self) {
        let mut state = pace::lock(&self.state);
        if state.starts == self.start {
            state.timeline.halt();
            state.fill = None;
        }
    }
}

/// Writes `silence` zero samples, then `buffer`.
fn write_samples(
    writer: &mut WavWriter<BufWriter<File>>,
    silence: usize,
    buffer: &[f32],
) -> Result<(), hound::Error> {
    for sample in std::iter::repeat_n(0.0, silence).chain(buffer.iter().copied()) {
        writer.write_sample(sample)?;
    }
    Ok(())
}

/// The WAV format of 32-bit floats in `format`.
const fn float_spec(format: StreamFormat) -> WavSpec {
    WavSpec {
        channels: format.channels,
        sample_rate: format.sample_rate_hz,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    }
}

/// An I/O error as itself, and a malformed or unsupported file as invalid data.
fn wav_error(error: hound::Error) -> AudioError {
    let io = if let hound::Error::IoError(io) = error {
        io
    } else {
        io::Error::new(io::ErrorKind::InvalidData, error)
    };
    AudioError::Io(io)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "an integer sample scaled to within full scale, kept to f32's precision on purpose"
)]
const fn narrow(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;

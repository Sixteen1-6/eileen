//! The capture callback's code (BUILD.md section 6.6): each buffer a source gives, at the
//! endpoint's rate and channel count, becomes 16 kHz mono and is cut into the speech pipe's
//! 512-sample frames, which go into the ring through its [`CaptureWriter`].
//!
//! [`Framer::push`] runs on the audio thread, so it allocates, frees, blocks and logs nothing:
//! its scratch buffers are allocated once, in [`Framer::new`], and a buffer longer than them is
//! taken a chunk at a time. Each frame's capture time is its first sample's place on the source's
//! clock: the buffer's time plus the samples before it, so the ring's replayed flag needs no clock
//! of its own.
//!
//! Echo cancellation is not here: sonora allocates as it runs AEC3 (`aec`'s module doc), so a
//! later part of the audio series hands these 16 kHz chunks to the component's side and cancels
//! the echo there, and the capture flags come from it. Until then every frame carries no capture
//! flag.

use std::time::Duration;

use eileen_contracts::speech::AUDIO_FRAME_SAMPLES;

use crate::resample::{ResampleError, Resampler, downmix};
use crate::ring::{CaptureFlags, CaptureWriter};
use crate::stream::{CaptureCallback, CaptureInfo, StreamFormat};

/// The speech pipe's rate.
pub(crate) const SPEECH_RATE_HZ: u32 = 16_000;

/// The most device frames taken in one step: 10 ms at 48 kHz, a WASAPI endpoint's default
/// period, so a usual buffer is one step.
const CHUNK_FRAMES: usize = 480;

/// Turns a source's buffers into the ring's frames.
pub(crate) struct Framer {
    writer: CaptureWriter,
    channels: u16,
    rate_hz: u32,
    /// `None` when the source already gives 16 kHz.
    resampler: Option<Resampler>,
    /// One chunk downmixed to mono.
    mono: Box<[f32]>,
    /// One chunk at 16 kHz.
    speech: Box<[f32]>,
    /// The frame being filled, and how much of it is.
    frame: [i16; AUDIO_FRAME_SAMPLES],
    filled: usize,
    /// When the frame's first sample was captured.
    frame_at: Duration,
}

impl Framer {
    /// A framer for a source of this format, writing into the ring through `writer`. Refuses a
    /// format with no channels or a rate no conversion of this crate takes to 16 kHz.
    pub(crate) fn new(format: StreamFormat, writer: CaptureWriter) -> Result<Self, ResampleError> {
        if format.channels == 0 {
            return Err(ResampleError::NoChannels);
        }
        let resampler = if format.sample_rate_hz == SPEECH_RATE_HZ {
            None
        } else {
            Some(Resampler::between(format.sample_rate_hz, SPEECH_RATE_HZ)?)
        };
        Ok(Self {
            writer,
            channels: format.channels,
            rate_hz: format.sample_rate_hz,
            resampler,
            mono: vec![0.0; CHUNK_FRAMES].into_boxed_slice(),
            // Every conversion to 16 kHz lowers the rate, so a chunk gives at most one sample
            // more than it holds frames (the first output of a fresh phase).
            speech: vec![0.0; CHUNK_FRAMES + 1].into_boxed_slice(),
            frame: [0; AUDIO_FRAME_SAMPLES],
            filled: 0,
            frame_at: Duration::ZERO,
        })
    }

    /// The capture callback that runs this framer.
    pub(crate) fn into_callback(mut self) -> CaptureCallback {
        Box::new(move |buffer: &[f32], info: CaptureInfo| self.push(buffer, info))
    }

    /// Takes one buffer of interleaved samples captured at `info.captured_at`. A buffer that ends
    /// part way through a device frame loses that partial frame and nothing else.
    pub(crate) fn push(&mut self, buffer: &[f32], info: CaptureInfo) {
        let Self { writer, channels, rate_hz, resampler, mono, speech, frame, filled, frame_at } =
            self;
        let width = usize::from(*channels);
        // Whole device frames only, so every chunk below holds whole frames and downmixes.
        let partial = buffer.len().checked_rem(width).unwrap_or_default();
        let buffer = buffer.get(..buffer.len() - partial).unwrap_or_default();
        let mut done = 0_usize;
        for chunk in buffer.chunks(CHUNK_FRAMES.saturating_mul(width)) {
            let chunk_at = info.captured_at.saturating_add(offset(done, *rate_hz));
            let Ok(frames) = downmix(chunk, *channels, mono) else {
                return;
            };
            done = done.saturating_add(frames);
            let mono = mono.get(..frames).unwrap_or_default();
            let samples = match resampler.as_mut() {
                Some(resampler) => match resampler.process(mono, speech) {
                    Ok(n) => speech.get(..n).unwrap_or_default(),
                    Err(_) => return,
                },
                None => mono,
            };
            for (at, sample) in samples.iter().enumerate() {
                if *filled == 0 {
                    *frame_at = chunk_at.saturating_add(offset(at, SPEECH_RATE_HZ));
                }
                if let Some(slot) = frame.get_mut(*filled) {
                    *slot = to_i16(*sample);
                }
                *filled += 1;
                if *filled == AUDIO_FRAME_SAMPLES {
                    // A full handoff loses the frame but uses its number, so the ring sees the
                    // gap and counts it; there is nothing to do about it here.
                    let _handed = writer.push(frame, *frame_at, CaptureFlags::default());
                    *filled = 0;
                }
            }
        }
    }
}

/// The time `samples` take at `rate_hz`.
fn offset(samples: usize, rate_hz: u32) -> Duration {
    let samples = u64::try_from(samples).unwrap_or(u64::MAX);
    Duration::from_nanos(samples.saturating_mul(1_000_000_000) / u64::from(rate_hz.max(1)))
}

/// A sample as the pipe's `i16`: clamped to full scale, and silence for a NaN.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is clamped to full scale first, and a NaN casts to zero"
)]
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use allocation_counter::AllocationInfo;

    use super::{Framer, to_i16};
    use crate::ring::SpeechRing;
    use crate::stream::{CaptureInfo, StreamFormat};

    fn at(ms: u64) -> CaptureInfo {
        CaptureInfo { captured_at: Duration::from_millis(ms) }
    }

    #[test]
    fn samples_become_the_pipes_i16_clamped_with_nan_as_silence() {
        assert_eq!(to_i16(0.5), 16_384);
        assert_eq!(to_i16(-1.0), -32_767);
        assert_eq!(to_i16(3.0), 32_767);
        assert_eq!(to_i16(f32::NAN), 0);
    }

    #[test]
    fn sixteen_kilohertz_mono_is_framed_sample_for_sample_with_each_frames_time() {
        let mut ring = SpeechRing::new(16, 16);
        let mut framer =
            Framer::new(StreamFormat { sample_rate_hz: 16_000, channels: 1 }, ring.open_capture())
                .unwrap();
        // 10 ms buffers of 160 samples whose values count up, so each frame's samples show where
        // it was cut.
        let mut next = 0_i16;
        for buffer in 0..10_u64 {
            let samples: Vec<f32> = (0..160)
                .map(|_| {
                    next += 1;
                    f32::from(next) / f32::from(i16::MAX)
                })
                .collect();
            framer.push(&samples, at(buffer * 10));
        }
        ring.connected();
        let first = ring.next_frame().unwrap();
        assert_eq!((first.seq, first.captured_at), (1, Duration::ZERO));
        assert_eq!((first.samples[0], first.samples[511]), (1, 512));
        let second = ring.next_frame().unwrap();
        assert_eq!(second.captured_at, Duration::from_millis(32), "512 samples later");
        assert_eq!((second.samples[0], second.samples[511]), (513, 1_024));
        let third = ring.next_frame().unwrap();
        assert_eq!(third.samples[0], 1_025);
        assert!(ring.next_frame().is_none(), "1 600 samples make three whole frames");
    }

    #[test]
    fn a_48_kilohertz_stereo_source_gives_a_frame_each_32_ms() {
        let mut ring = SpeechRing::new(64, 64);
        let mut framer =
            Framer::new(StreamFormat { sample_rate_hz: 48_000, channels: 2 }, ring.open_capture())
                .unwrap();
        let buffer = vec![0.25_f32; 960];
        for n in 0..100_u64 {
            framer.push(&buffer, at(n * 10));
        }
        ring.connected();
        let mut times = Vec::new();
        while let Some(frame) = ring.next_frame() {
            times.push(frame.captured_at);
        }
        assert_eq!(times.len(), 31, "one second at 16 kHz is 31 whole frames");
        assert_eq!(times[1], Duration::from_millis(32));
        assert_eq!(times[30], Duration::from_millis(960));
    }

    #[test]
    fn a_buffer_that_ends_inside_a_device_frame_loses_only_that_partial_frame() {
        // 48 kHz stereo buffers of 959 samples: 479 whole frames and one sample of the next, the
        // same as the buffer of 958 samples beside it apart from that one sample.
        let format = StreamFormat { sample_rate_hz: 48_000, channels: 2 };
        let mut ragged_ring = SpeechRing::new(64, 64);
        let mut whole_ring = SpeechRing::new(64, 64);
        let mut ragged = Framer::new(format, ragged_ring.open_capture()).unwrap();
        let mut whole = Framer::new(format, whole_ring.open_capture()).unwrap();
        for n in 0..64_u64 {
            // A ramp that wraps every 200 frames, so a frame cut in the wrong place shows in its
            // samples, and a stray last sample at full scale, so one that leaked in shows too.
            let buffer: Vec<f32> = (0..479_u64)
                .flat_map(|k| {
                    let level = f32::from(u16::try_from((n * 479 + k) % 200).unwrap()) / 400.0;
                    [level, level]
                })
                .chain(std::iter::once(1.0))
                .collect();
            assert_eq!(buffer.len(), 959, "479 whole frames and one sample");
            ragged.push(&buffer, at(n * 10));
            whole.push(&buffer[..958], at(n * 10));
        }
        let frames = |ring: &mut SpeechRing| {
            ring.connected();
            let mut frames = Vec::new();
            while let Some(frame) = ring.next_frame() {
                frames.push((frame.seq, frame.captured_at, *frame.samples));
            }
            frames
        };
        let got = frames(&mut ragged_ring);
        assert_eq!(got.len(), 19, "64 buffers of 479 frames at 48 kHz are 19 whole frames");
        assert!(got == frames(&mut whole_ring), "the frames of buffers that end on a whole frame");
    }

    #[test]
    fn formats_with_no_conversion_or_no_channels_are_refused() {
        let mut ring = SpeechRing::new(4, 4);
        assert!(
            Framer::new(StreamFormat { sample_rate_hz: 32_000, channels: 1 }, ring.open_capture())
                .is_err()
        );
        assert!(
            Framer::new(StreamFormat { sample_rate_hz: 48_000, channels: 0 }, ring.open_capture())
                .is_err()
        );
    }

    #[test]
    fn the_capture_callback_allocates_nothing() {
        let mut ring = SpeechRing::new(64, 64);
        let format = StreamFormat { sample_rate_hz: 44_100, channels: 2 };
        let mut callback = Framer::new(format, ring.open_capture()).unwrap().into_callback();
        // Longer than one chunk, so the chunking runs too.
        let buffer = vec![0.1_f32; 2 * 1_323];
        let counted = allocation_counter::measure(|| {
            for n in 0..10_u64 {
                callback(&buffer, at(n * 30));
            }
        });
        assert_eq!(counted, AllocationInfo::default(), "no allocation, reallocation or free");
    }
}

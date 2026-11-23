//! Sample-rate conversion and the capture downmix (BUILD.md sections 2.8 and 6.6): captured
//! audio from the endpoint's rate to the speech pipe's 16 kHz mono, and synthesized speech from
//! 24 kHz to the endpoint's rate.
//!
//! One polyphase engine serves every conversion. A conversion upsamples by `up` and downsamples
//! by `down`: a windowed-sinc low-pass at `up` times the input rate, split into `up` phases of
//! `taps` coefficients each, so each output costs one `taps`-long dot product and the zeros of
//! the upsampled signal are never computed. At the 48 kHz endpoint rate section 2.8 fixes, both
//! ratios are integers: capture is the 3:1 decimator (`up` 1, `down` 3) and synthesis the 1:2
//! interpolator (`up` 2, `down` 1). An endpoint whose shared-mode rate is 44.1 kHz needs `up` 160
//! and `down` 441 for capture, since cpal opens a WASAPI capture stream only at the device's own
//! rates (cpal 0.18.2, `host/wasapi/device.rs`, where only output streams take
//! `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`). The loopback is such a capture stream, opened on the
//! render endpoint with `AUDCLNT_STREAMFLAGS_LOOPBACK`, so the echo reference of a 44.1 kHz
//! endpoint takes the same 160:441 conversion. An endpoint mixing at any other rate gets
//! [`ResampleError::UnsupportedRates`]. Synthesis to a 44.1 kHz endpoint takes `up` 147 and
//! `down` 80, which Eileen keeps so that the render path need not rely on Windows' own converter,
//! whose quality this crate cannot measure on CI.
//!
//! Each conversion meets the bar of section 6.6, measured in `tests` through `realfft`: a capture
//! conversion is flat within 0.1 dB up to 7 kHz and at least 60 dB down from 9 kHz up, so nothing
//! aliases below 7 kHz; a synthesis conversion is flat within 0.1 dB up to 10 kHz, with its
//! images at least 60 dB down from 14 kHz up. The filters are designed for 80 dB, which leaves
//! margin for the `f32` coefficients. The cost bar (at most 25 us per 10 ms buffer in a release
//! build on the owner's PC) is the `owner_pc::` benchmark in `tests`.
//!
//! [`Resampler::process`] and [`downmix`] run in the audio callbacks, so they allocate, free,
//! block and log nothing: the coefficients and the delay line are allocated once, in
//! [`Resampler::new`]. A resampler carries its history from one buffer to the next, so a stream
//! cut into buffers of any size gives the same samples as one call over the whole.

mod design;
#[cfg(test)]
mod tests;

use std::fmt;

/// Why a conversion did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ResampleError {
    /// No conversion of this crate goes between these rates.
    #[error("no conversion from {input_hz} Hz to {output_hz} Hz")]
    UnsupportedRates {
        /// The rate asked for at the input.
        input_hz: u32,
        /// The rate asked for at the output.
        output_hz: u32,
    },
    /// The output slice cannot hold every sample the input gives.
    #[error("the output holds {got} samples, and the input gives {needed}")]
    OutputTooShort {
        /// Samples the input gives.
        needed: usize,
        /// Samples the output holds.
        got: usize,
    },
    /// A stream with no channels.
    #[error("a stream with no channels")]
    NoChannels,
    /// Interleaved samples that end part way through a frame.
    #[error("{samples} samples is not a whole number of {channels}-channel frames")]
    PartialFrame {
        /// The samples given.
        samples: usize,
        /// The channels per frame.
        channels: u16,
    },
}

/// The conversions Eileen's audio needs: capture to the speech pipe's 16 kHz, and 24 kHz
/// synthesis to the endpoint, at an endpoint rate of 48 kHz or 44.1 kHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Conversion {
    /// Capture at 48 kHz to 16 kHz: the 3:1 decimator.
    From48kTo16k,
    /// Capture at 44.1 kHz to 16 kHz: up by 160, down by 441.
    From44k1To16k,
    /// Synthesis at 24 kHz to a 48 kHz endpoint: the 1:2 interpolator.
    From24kTo48k,
    /// Synthesis at 24 kHz to a 44.1 kHz endpoint: up by 147, down by 80.
    From24kTo44k1,
}

impl Conversion {
    /// Every conversion, capture first.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] =
        [Self::From48kTo16k, Self::From44k1To16k, Self::From24kTo48k, Self::From24kTo44k1];

    /// The conversion from `input_hz` to `output_hz`, if this crate has one.
    pub(crate) const fn between(input_hz: u32, output_hz: u32) -> Option<Self> {
        match (input_hz, output_hz) {
            (48_000, 16_000) => Some(Self::From48kTo16k),
            (44_100, 16_000) => Some(Self::From44k1To16k),
            (24_000, 48_000) => Some(Self::From24kTo48k),
            (24_000, 44_100) => Some(Self::From24kTo44k1),
            _ => None,
        }
    }

    /// The rate it takes.
    pub(crate) const fn input_hz(self) -> u32 {
        match self {
            Self::From48kTo16k => 48_000,
            Self::From44k1To16k => 44_100,
            Self::From24kTo48k | Self::From24kTo44k1 => 24_000,
        }
    }

    /// The rate it gives.
    #[cfg(test)]
    pub(crate) const fn output_hz(self) -> u32 {
        match self {
            Self::From48kTo16k | Self::From44k1To16k => 16_000,
            Self::From24kTo48k => 48_000,
            Self::From24kTo44k1 => 44_100,
        }
    }
}

/// A streaming polyphase resampler for one [`Conversion`].
pub(crate) struct Resampler {
    conversion: Conversion,
    /// The ratio's numerator: phases per input sample.
    up: usize,
    /// The ratio's denominator: upsampled positions per output sample.
    down: usize,
    /// Coefficients per phase, and the samples of history each output reads.
    taps: usize,
    /// One slice of `taps` coefficients per phase, each ordered oldest sample first, so it lines
    /// up with the delay line's window.
    phases: Box<[Box<[f32]>]>,
    /// The last `taps` input samples, written twice (at `pos` and `pos + taps`), so the window
    /// from `pos` is always `taps` contiguous samples, oldest first.
    line: Box<[f32]>,
    /// Where the next input sample goes, below `taps`.
    pos: usize,
    /// Where the next output falls, on the upsampled grid, counted from the start of the next
    /// input sample's `up` positions; always below `down` between calls.
    offset: usize,
}

impl Resampler {
    /// A resampler for `conversion`, with silence as its history. This designs the filter and
    /// allocates its coefficients and delay line, so it runs outside the audio callbacks.
    pub(crate) fn new(conversion: Conversion) -> Self {
        let design = design::Design::of(conversion);
        Self {
            conversion,
            up: design.up as usize,
            down: design.down as usize,
            taps: design.taps as usize,
            phases: design.phases(),
            line: vec![0.0; 2 * design.taps as usize].into_boxed_slice(),
            pos: 0,
            offset: 0,
        }
    }

    /// A resampler from `input_hz` to `output_hz`, if this crate has that conversion.
    pub(crate) fn between(input_hz: u32, output_hz: u32) -> Result<Self, ResampleError> {
        Conversion::between(input_hz, output_hz)
            .map(Self::new)
            .ok_or(ResampleError::UnsupportedRates { input_hz, output_hz })
    }

    /// Its conversion.
    #[cfg(test)]
    pub(crate) const fn conversion(&self) -> Conversion {
        self.conversion
    }

    /// Exactly how many samples [`Resampler::process`] gives for `input_len` samples now. Outputs
    /// fall every `down` positions of the upsampled grid, and the input's samples cover the
    /// next `input_len * up` positions from where the next output stands at `offset`.
    pub(crate) const fn output_len(&self, input_len: usize) -> usize {
        let covered = input_len.saturating_mul(self.up);
        if covered <= self.offset { 0 } else { (covered - self.offset).div_ceil(self.down) }
    }

    /// Converts `input` into the front of `output` and returns how many samples it wrote, which
    /// is [`Resampler::output_len`] of the input. An output too short for them is refused before
    /// any input is taken, so the resampler is unchanged. Allocates nothing.
    pub(crate) fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<usize, ResampleError> {
        let needed = self.output_len(input.len());
        if output.len() < needed {
            return Err(ResampleError::OutputTooShort { needed, got: output.len() });
        }
        let mut written = 0;
        for &sample in input {
            if let Some(slot) = self.line.get_mut(self.pos) {
                *slot = sample;
            }
            if let Some(slot) = self.line.get_mut(self.pos + self.taps) {
                *slot = sample;
            }
            self.pos = if self.pos + 1 == self.taps { 0 } else { self.pos + 1 };
            // `offset` below `up` names the phase of the next output, which this sample is the
            // newest input of; the window ends at this sample.
            while let Some(phase) = self.phases.get(self.offset) {
                // The doubled delay line always holds `taps` samples from `pos`.
                let window = self.line.get(self.pos..self.pos + self.taps).unwrap_or_default();
                let value = dot(phase, window);
                if let Some(slot) = output.get_mut(written) {
                    *slot = value;
                }
                written += 1;
                self.offset += self.down;
            }
            self.offset -= self.up;
        }
        Ok(written)
    }
}

/// Partial sums [`dot`] keeps apart. Every filter's `taps` is a multiple of it.
const LANES: usize = 8;

/// The inner product of a phase's coefficients and the window of samples they multiply. A
/// single running sum would make each multiply-add wait on the one before it, since the compiler
/// may not reorder `f32` additions; `LANES` independent sums let it use vector registers. The
/// order of the additions depends only on `taps`, never on how the input was cut into buffers,
/// so the result is the same bit for bit whatever the buffer sizes.
fn dot(coefficients: &[f32], window: &[f32]) -> f32 {
    let (coefficient_chunks, coefficient_rest) = coefficients.as_chunks::<LANES>();
    let (window_chunks, window_rest) = window.as_chunks::<LANES>();
    let mut lanes = [0.0_f32; LANES];
    for (c, x) in coefficient_chunks.iter().zip(window_chunks) {
        for ((lane, c), x) in lanes.iter_mut().zip(c).zip(x) {
            *lane += c * x;
        }
    }
    let rest: f32 = coefficient_rest.iter().zip(window_rest).map(|(c, x)| c * x).sum();
    lanes.iter().sum::<f32>() + rest
}

impl fmt::Debug for Resampler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resampler")
            .field("conversion", &self.conversion)
            .field("taps", &self.taps)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

/// Averages each frame of `interleaved` samples, `channels` to a frame, into the front of `mono`,
/// and returns the frames written: the capture downmix from a stereo endpoint to the speech
/// pipe's mono. Allocates nothing.
pub(crate) fn downmix(
    interleaved: &[f32],
    channels: u16,
    mono: &mut [f32],
) -> Result<usize, ResampleError> {
    if channels == 0 {
        return Err(ResampleError::NoChannels);
    }
    let width = usize::from(channels);
    if !interleaved.len().is_multiple_of(width) {
        return Err(ResampleError::PartialFrame { samples: interleaved.len(), channels });
    }
    let frames = interleaved.len() / width;
    if mono.len() < frames {
        return Err(ResampleError::OutputTooShort { needed: frames, got: mono.len() });
    }
    let scale = 1.0 / f32::from(channels);
    for (frame, slot) in interleaved.chunks_exact(width).zip(mono.iter_mut()) {
        *slot = frame.iter().sum::<f32>() * scale;
    }
    Ok(frames)
}

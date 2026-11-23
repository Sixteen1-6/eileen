//! The filters: a Kaiser-windowed sinc low-pass per conversion, designed in `f64` when a
//! resampler is made and kept as `f32` phases.
//!
//! The lengths come from Kaiser's estimate for an 80 dB stopband,
//! `taps * up - 1 = (80 - 7.95) / (2.285 * 2 * pi * transition / (input_hz * up))`, rounded up
//! with some margin: a capture conversion's transition runs from 7 kHz to 9 kHz around a cutoff
//! of 8 kHz, a synthesis conversion's from 10 kHz to 14 kHz around 12 kHz, half the 24 kHz input
//! rate. The tests in `super::tests`
//! measure the result against the bar, so these numbers need no other proof.

use super::Conversion;

/// Kaiser's beta for an 80 dB stopband, `0.1102 * (80 - 8.7)`.
const KAISER_BETA: f64 = 7.857;

/// One conversion's filter.
#[derive(Debug, Clone, Copy)]
pub(super) struct Design {
    /// The ratio's numerator: the prototype runs at `up` times the input rate.
    pub(super) up: u32,
    /// The ratio's denominator.
    pub(super) down: u32,
    /// Coefficients per phase; the prototype has `up * taps`.
    pub(super) taps: u32,
    /// The low-pass's cutoff, half-way across the transition band.
    cutoff_hz: u32,
    /// The input rate.
    input_hz: u32,
}

impl Design {
    pub(super) const fn of(conversion: Conversion) -> Self {
        let (up, down, taps, cutoff_hz) = match conversion {
            Conversion::From48kTo16k => (1, 3, 128, 8_000),
            Conversion::From44k1To16k => (160, 441, 120, 8_000),
            Conversion::From24kTo48k => (2, 1, 32, 12_000),
            Conversion::From24kTo44k1 => (147, 80, 32, 12_000),
        };
        Self { up, down, taps, cutoff_hz, input_hz: conversion.input_hz() }
    }

    /// The prototype split into `up` phases of `taps` coefficients: phase `p` holds the
    /// prototype's coefficients `p`, `p + up`, `p + 2 up` and so on, which multiply the newest
    /// input sample, the one before it and so on, reversed so that the oldest sample comes
    /// first, as in the resampler's delay line.
    pub(super) fn phases(&self) -> Box<[Box<[f32]>]> {
        let prototype = self.prototype();
        let (up, taps) = (self.up as usize, self.taps as usize);
        (0..up)
            .map(|phase| {
                (0..taps)
                    .rev()
                    .map(|age| prototype.get(phase + age * up).copied().map_or(0.0, narrow))
                    .collect()
            })
            .collect()
    }

    /// The windowed sinc at `up` times the input rate, scaled so its coefficients sum to `up`:
    /// zero-stuffing by `up` divides the signal's level by `up`, so this gives unity gain at DC.
    fn prototype(&self) -> Vec<f64> {
        let len = self.up * self.taps;
        let width =
            2.0 * f64::from(self.cutoff_hz) / (f64::from(self.input_hz) * f64::from(self.up));
        let middle = f64::from(len - 1) / 2.0;
        let window_norm = bessel_i0(KAISER_BETA);
        let mut coefficients: Vec<f64> = (0..len)
            .map(|n| {
                let x = f64::from(n) - middle;
                let sinc = if x == 0.0 {
                    width
                } else {
                    (std::f64::consts::PI * width * x).sin() / (std::f64::consts::PI * x)
                };
                let r = x / middle;
                let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / window_norm;
                sinc * window
            })
            .collect();
        let sum: f64 = coefficients.iter().sum();
        let scale = f64::from(self.up) / sum;
        for c in &mut coefficients {
            *c *= scale;
        }
        coefficients
    }
}

/// The modified Bessel function of the first kind, order zero, by its power series, which
/// converges fast for the arguments a Kaiser window takes (up to beta).
fn bessel_i0(x: f64) -> f64 {
    let quarter_square = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..=64_u32 {
        term *= quarter_square / f64::from(k * k);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a coefficient below 2 in magnitude, kept to f32's precision on purpose"
)]
const fn narrow(value: f64) -> f32 {
    value as f32
}

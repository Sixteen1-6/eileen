//! Checked and saturating conversions, named for what they do.
//!
//! Outside the numeric kernels listed in `xtask/tidy/numeric-modules.toml`, code converts
//! numbers through these functions instead of `as`, so a value that does not fit is either a
//! typed [`OutOfRange`] error or an explicit saturation, never a silent wrap.

use std::fmt;
use std::time::Duration;

/// A value that does not fit the type it was converted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfRange {
    /// The target type, for example `"u32"`.
    pub target: &'static str,
    /// The value, saturated to the `i128` range.
    pub value: i128,
}

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} does not fit in {}", self.value, self.target)
    }
}

impl std::error::Error for OutOfRange {}

/// Converts with `TryFrom`, naming the target in the error.
fn checked<T, U>(value: T, target: &'static str) -> Result<U, OutOfRange>
where
    T: Copy,
    U: TryFrom<T>,
    i128: TryFrom<T>,
{
    U::try_from(value)
        .map_err(|_| OutOfRange { target, value: i128::try_from(value).unwrap_or(i128::MAX) })
}

/// `usize` to `u32`.
pub fn u32_from_usize(value: usize) -> Result<u32, OutOfRange> {
    checked(value, "u32")
}

/// `usize` to `u16`.
pub fn u16_from_usize(value: usize) -> Result<u16, OutOfRange> {
    checked(value, "u16")
}

/// `u64` to `u32`.
pub fn u32_from_u64(value: u64) -> Result<u32, OutOfRange> {
    checked(value, "u32")
}

/// `u64` to `usize`.
pub fn usize_from_u64(value: u64) -> Result<usize, OutOfRange> {
    checked(value, "usize")
}

/// `u64` to `i64`.
pub fn i64_from_u64(value: u64) -> Result<i64, OutOfRange> {
    checked(value, "i64")
}

/// `i64` to `u64`; negative values are out of range.
pub fn u64_from_i64(value: i64) -> Result<u64, OutOfRange> {
    checked(value, "u64")
}

/// `i64` to `i32`.
pub fn i32_from_i64(value: i64) -> Result<i32, OutOfRange> {
    checked(value, "i32")
}

/// `u64` to `u32`, saturating at `u32::MAX`.
#[must_use]
pub fn saturating_u32_from_u64(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// `usize` to `u32`, saturating at `u32::MAX`.
#[must_use]
pub fn saturating_u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// `usize` to `u64`, saturating (only on targets wider than 64 bits).
#[must_use]
pub fn saturating_u64_from_usize(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// `u64` to `usize`, saturating on 32-bit targets.
#[must_use]
pub fn saturating_usize_from_u64(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// `u128` to `u64`, saturating at `u64::MAX` (for `Duration::as_nanos` and friends).
#[must_use]
pub fn saturating_u64_from_u128(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// `u128` to `i64`, saturating at `i64::MAX`.
#[must_use]
pub fn saturating_i64_from_u128(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// `u64` to `i64`, saturating at `i64::MAX`.
#[must_use]
pub fn saturating_i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// `i64` to `u64`, with negative values as zero.
#[must_use]
pub fn saturating_u64_from_i64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Whole milliseconds of a duration, saturating at `u64::MAX`.
#[must_use]
pub fn millis_u64(duration: Duration) -> u64 {
    saturating_u64_from_u128(duration.as_millis())
}

/// Whole milliseconds of a duration, saturating at `u32::MAX` (about 49 days).
#[must_use]
pub fn millis_u32(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// Whole microseconds of a duration, saturating at `i64::MAX`.
#[must_use]
pub fn micros_i64(duration: Duration) -> i64 {
    saturating_i64_from_u128(duration.as_micros())
}

/// Rounds a float to the nearest `u32`: NaN and negative values give 0, values past the top
/// give `u32::MAX`.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the range and NaN are checked first, so the cast is exact after rounding"
)]
pub fn rounded_u32_from_f64(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value.round() as u32
    }
}

/// A `u64` as the nearest `f64`; values above 2^53 lose their low bits, which is fine for
/// averages and percentiles.
#[must_use]
#[expect(clippy::cast_precision_loss, reason = "the nearest f64 is what the name promises")]
pub const fn approx_f64_from_u64(value: u64) -> f64 {
    value as f64
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::{
        OutOfRange, approx_f64_from_u64, i64_from_u64, millis_u32, rounded_u32_from_f64,
        saturating_u32_from_u64, saturating_u64_from_i64, u32_from_usize, u64_from_i64,
    };

    #[test]
    fn errors_name_the_target_and_value() {
        assert_eq!(u32_from_usize(usize::MAX).unwrap_err().target, "u32", "target named",);
        assert_eq!(
            u64_from_i64(-3),
            Err(OutOfRange { target: "u64", value: -3 }),
            "negative to unsigned",
        );
        assert_eq!(
            i64_from_u64(u64::MAX).unwrap_err().to_string(),
            "18446744073709551615 does not fit in i64",
            "message",
        );
    }

    #[test]
    fn floats_round_and_clamp() {
        assert_eq!(rounded_u32_from_f64(f64::NAN), 0, "NaN");
        assert_eq!(rounded_u32_from_f64(-1.0), 0, "negative");
        assert_eq!(rounded_u32_from_f64(2.5), 3, "half away from zero");
        assert_eq!(rounded_u32_from_f64(1e12), u32::MAX, "past the top");
        assert_eq!(millis_u32(Duration::from_secs(u64::MAX)), u32::MAX, "saturated millis");
        assert_eq!(saturating_u64_from_i64(-9), 0, "negative to zero");
    }

    proptest! {
        #[test]
        fn saturation_never_wraps(v: u64) {
            let s = saturating_u32_from_u64(v);
            prop_assert!(u64::from(s) == v || s == u32::MAX, "exact or saturated");
        }

        #[test]
        fn small_values_round_trip_through_f64(v in 0_u64..(1 << 53)) {
            prop_assert_eq!(rounded_u32_from_f64(approx_f64_from_u64(v)), saturating_u32_from_u64(v), "round trip");
        }
    }
}

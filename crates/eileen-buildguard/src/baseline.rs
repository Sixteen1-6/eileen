//! The baseline: thresholds measured from a typical working day on this PC.
//!
//! `cargo xtask slots --baseline` samples every 5 s over a working day and calls
//! [`compute_baseline`] on what it gathered. Samples taken while a build slot was held are
//! recorded but left out, so the baseline describes the working day and not the guard's own
//! builds. From the remaining samples, grouped into 30 s windows:
//!
//! - the slot-0 paging limit is the 75th percentile of the window averages plus 25 percent;
//! - the re-check limit is the 95th percentile plus 25 percent, and never less than twice the
//!   slot-0 limit;
//! - the one headroom floor is the lower of 2 GB and the 10th percentile of commit headroom,
//!   but never under 1 GB.

use std::collections::BTreeMap;

use crate::samples::{Baseline, Sample, percentile, plus_25};

/// The floor never goes under this, MB (section 2.5 rule 1).
pub const MIN_FLOOR_MB: u32 = 1_024;
/// The floor never goes over this, MB.
pub const MAX_FLOOR_MB: u32 = 2_048;

/// The thresholds from a day of samples; `None` when every sample had a slot held.
#[must_use]
pub fn compute_baseline(samples: &[Sample]) -> Option<Baseline> {
    let kept: Vec<&Sample> = samples.iter().filter(|s| !s.slot_held).collect();
    let excluded = samples.len() - kept.len();
    let mut windows: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    for s in &kept {
        let entry = windows.entry(s.at_ms / 30_000).or_insert((0, 0));
        entry.0 += u64::from(s.paging_kb_s);
        entry.1 += 1;
    }
    let averages: Vec<u32> = windows
        .values()
        .map(|(sum, n)| u32::try_from(sum / (*n).max(1)).unwrap_or(u32::MAX))
        .collect();
    let slot0 = plus_25(percentile(&averages, 75)?);
    let recheck = plus_25(percentile(&averages, 95)?).max(slot0.saturating_mul(2));
    let headroom: Vec<u32> = kept.iter().map(|s| s.headroom_mb).collect();
    let floor = percentile(&headroom, 10)?.clamp(MIN_FLOOR_MB, MAX_FLOOR_MB);
    let first = kept.iter().map(|s| s.at_ms).min()?;
    let last = kept.iter().map(|s| s.at_ms).max()?;
    Some(Baseline {
        measured_from: utc_date(first),
        measured_to: utc_date(last),
        samples: u64::try_from(kept.len()).unwrap_or(u64::MAX),
        slot_held_excluded: u64::try_from(excluded).unwrap_or(u64::MAX),
        slot0_paging_kb_s: slot0,
        recheck_paging_kb_s: recheck,
        headroom_floor_mb: floor,
    })
}

/// The distinct UTC hours the samples not marked `slot_held` fall in. The baseline asks for
/// at least four separate hours of a typical working day.
#[must_use]
pub fn hours_covered(samples: &[Sample]) -> usize {
    let mut hours: Vec<u64> =
        samples.iter().filter(|s| !s.slot_held).map(|s| s.at_ms / 3_600_000).collect();
    hours.sort_unstable();
    hours.dedup();
    hours.len()
}

/// `YYYY-MM-DD` of a Unix millisecond time, UTC (Howard Hinnant's `civil_from_days`).
#[must_use]
pub fn utc_date(at_ms: u64) -> String {
    let days = i64::try_from(at_ms / 86_400_000).unwrap_or(i64::MAX) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::{compute_baseline, hours_covered, utc_date};
    use crate::samples::{Sample, Source};

    fn sample(at_ms: u64, headroom_mb: u32, paging_kb_s: u32, slot_held: bool) -> Sample {
        Sample { at_ms, avail_mb: 1_500, headroom_mb, paging_kb_s, slot_held, src: Source::Guard }
    }

    #[test]
    fn dates_are_utc() {
        assert_eq!(utc_date(0), "1970-01-01", "epoch");
        assert_eq!(utc_date(1_761_004_800_000), "2025-10-21", "a recent day");
        assert_eq!(utc_date(951_782_400_000), "2000-02-29", "a leap day");
    }

    #[test]
    fn slot_held_samples_are_left_out() {
        let mut samples: Vec<Sample> =
            (0..120).map(|i| sample(i * 5_000, 6_000, 1_000, false)).collect();
        samples.extend((120..240).map(|i| sample(i * 5_000, 100, 90_000, true)));
        let baseline = compute_baseline(&samples).unwrap();
        assert_eq!(baseline.samples, 120, "kept");
        assert_eq!(baseline.slot_held_excluded, 120, "excluded");
        assert_eq!(baseline.slot0_paging_kb_s, 1_250, "the builds' paging does not count");
        assert_eq!(baseline.headroom_floor_mb, 2_048, "the builds' headroom does not count");
        assert!(compute_baseline(&samples[120..]).is_none(), "nothing left");
    }

    #[test]
    fn the_recheck_limit_is_at_least_twice_slot0_and_the_floor_at_least_a_gigabyte() {
        // Paging rising across windows, headroom around 0.6 GB.
        let samples: Vec<Sample> = (0..600)
            .map(|i| sample(i * 5_000, 614, 1_000 + u32::try_from(i).unwrap(), false))
            .collect();
        let baseline = compute_baseline(&samples).unwrap();
        assert!(baseline.recheck_paging_kb_s >= 2 * baseline.slot0_paging_kb_s, "{baseline:?}");
        assert_eq!(
            baseline.headroom_floor_mb, 1_024,
            "10th percentile 0.6 GB gives the 1 GB floor"
        );
        assert_eq!(hours_covered(&samples), 1, "fifty minutes is one hour");
    }
}

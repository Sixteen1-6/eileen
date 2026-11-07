//! The hourly summaries the machine profile reads (BUILD.md sections 2.16 and 6.1), and the
//! usual memory they give.
//!
//! `machine/memory-hours.jsonl` holds one `{"at_ms": <the hour's end, Unix ms>,
//! "avail_p10_mb": u32, "avail_p50_mb": u32, "avail_p90_mb": u32, "headroom_p10_mb": u32,
//! "paging_p75_kb_s": u32, "plan_items_mb": u32, "slot_held_pct": u8, "samples": u32, "src":
//! "runtime"}` per hour, which the runtime computes from its own 5 s samples and appends under
//! the samples lock, only when reading real memory and only when the file's newest line ends at
//! least an hour before the new one does, so the PC gets one line per hour however many
//! runtimes run. Lines older than 30 days are dropped by the next writer that finds the file
//! over 2 MB. [`usual_of`] turns them into the [`Usual`] memory, which the runtime hands the
//! core as the contracts' `UsualMemory`.

use std::io;
use std::time::Duration;

use super::{Source, Store, append_line, percentile, read_lines, trim};
use crate::clock::{Clock, RealClock, millis};
use crate::json;

/// The hourly summaries, relative to the machine root.
pub const HOURS_FILE: &str = "machine/memory-hours.jsonl";
/// The hours file is trimmed once it passes this size.
pub const HOURS_TRIM_BYTES: u64 = 2 * 1024 * 1024;
/// How long hours are kept when the file is trimmed.
pub const HOURS_KEEP: Duration = Duration::from_hours(30 * 24);
/// The length of the hour one line summarises.
pub const HOUR: Duration = Duration::from_hours(1);
/// An hour counts toward the usual figure only with at least this many samples: half an hour
/// of 5 s samples.
pub const USUAL_MIN_SAMPLES: u32 = 360;
/// The usual figure needs at least this many qualifying hours; with fewer, the runtime uses the
/// highest available reading over the first minute after start (section 2.16).
pub const USUAL_MIN_HOURS: usize = 24;
/// The days the usual figure looks back over.
pub const USUAL_DAYS: u32 = 7;

/// One hour's summary of the runtime's 5 s samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HourSummary {
    /// The hour's end, Unix milliseconds.
    pub at_ms: u64,
    /// The hour's 10th percentile of available memory, MB.
    pub avail_p10_mb: u32,
    /// Its median of available memory, MB.
    pub avail_p50_mb: u32,
    /// Its 90th percentile of available memory, MB.
    pub avail_p90_mb: u32,
    /// Its 10th percentile of commit headroom, MB.
    pub headroom_p10_mb: u32,
    /// Its 75th percentile of the paging rate, KB/s.
    pub paging_p75_kb_s: u32,
    /// The median of what the plan's own items held at the same samples (the spare's and the
    /// pooled sessions' measured working sets, and the chosen speech models' resident sizes), MB.
    pub plan_items_mb: u32,
    /// The share of its samples taken while a build slot was held, percent.
    pub slot_held_pct: u8,
    /// How many samples it summarises.
    pub samples: u32,
    /// Who wrote it (always the runtime).
    pub src: Source,
}

impl HourSummary {
    fn to_line(self) -> String {
        json::Writer::default()
            .num("at_ms", self.at_ms)
            .num("avail_p10_mb", u64::from(self.avail_p10_mb))
            .num("avail_p50_mb", u64::from(self.avail_p50_mb))
            .num("avail_p90_mb", u64::from(self.avail_p90_mb))
            .num("headroom_p10_mb", u64::from(self.headroom_p10_mb))
            .num("paging_p75_kb_s", u64::from(self.paging_p75_kb_s))
            .num("plan_items_mb", u64::from(self.plan_items_mb))
            .num("slot_held_pct", u64::from(self.slot_held_pct))
            .num("samples", u64::from(self.samples))
            .str("src", self.src.as_str())
            .finish()
    }

    fn from_line(line: &str) -> Option<Self> {
        let o = json::parse(line)?;
        Some(Self {
            at_ms: o.u64("at_ms")?,
            avail_p10_mb: o.u32("avail_p10_mb")?,
            avail_p50_mb: o.u32("avail_p50_mb")?,
            avail_p90_mb: o.u32("avail_p90_mb")?,
            headroom_p10_mb: o.u32("headroom_p10_mb")?,
            paging_p75_kb_s: o.u32("paging_p75_kb_s")?,
            plan_items_mb: o.u32("plan_items_mb")?,
            slot_held_pct: u8::try_from(o.u32("slot_held_pct")?).ok()?,
            samples: o.u32("samples")?,
            src: Source::parse(o.str("src")?)?,
        })
    }
}

/// The PC's usual memory over the qualifying hours: those holding at least
/// [`USUAL_MIN_SAMPLES`] samples. The fields are the contracts' `UsualMemory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usual {
    /// How many hours qualified.
    pub hours: u16,
    /// The 25th percentile of their medians of available memory, MB.
    pub available_mb: u32,
    /// The median of what the plan's own items held in them, MB.
    pub plan_items_mb: u32,
    /// The 25th percentile of their 10th percentiles of commit headroom, MB.
    pub headroom_p10_mb: u32,
}

impl Usual {
    /// The usual free memory: the 25th percentile of the hourly medians with the plan's own
    /// items added back, what the PC would usually have free without Eileen's extras.
    #[must_use]
    pub const fn free_mb(&self) -> u32 {
        self.available_mb.saturating_add(self.plan_items_mb)
    }
}

/// The usual memory over `hours`, by the nearest-rank percentile; `None` with fewer than
/// [`USUAL_MIN_HOURS`] qualifying hours.
#[must_use]
pub fn usual_of(hours: &[HourSummary]) -> Option<Usual> {
    let qualifying: Vec<&HourSummary> =
        hours.iter().filter(|hour| hour.samples >= USUAL_MIN_SAMPLES).collect();
    if qualifying.len() < USUAL_MIN_HOURS {
        return None;
    }
    let field = |value: fn(&HourSummary) -> u32| -> Vec<u32> {
        qualifying.iter().map(|hour| value(hour)).collect()
    };
    Some(Usual {
        hours: u16::try_from(qualifying.len()).unwrap_or(u16::MAX),
        available_mb: percentile(&field(|hour| hour.avail_p50_mb), 25)?,
        plan_items_mb: percentile(&field(|hour| hour.plan_items_mb), 50)?,
        headroom_p10_mb: percentile(&field(|hour| hour.headroom_p10_mb), 25)?,
    })
}

impl Store {
    /// Appends one hour's summary unless the file's newest line ends less than an hour before
    /// it does (checked under the lock), so two runtimes never write one hour twice, trimming
    /// lines older than 30 days first when the file has passed 2 MB. Returns whether it was
    /// written. A busy lock is waited for on `clock`.
    pub fn append_hour(&self, hour: &HourSummary, clock: &dyn Clock) -> io::Result<bool> {
        let path = self.path(HOURS_FILE);
        self.locked(clock, || {
            let newest =
                read_lines(&path, HourSummary::from_line)?.into_iter().map(|h| h.at_ms).max();
            if newest.is_some_and(|at| at.saturating_add(millis(HOUR)) > hour.at_ms) {
                return Ok(false);
            }
            let keep_from = hour.at_ms.saturating_sub(millis(HOURS_KEEP));
            trim(&path, HOURS_TRIM_BYTES, |line| {
                HourSummary::from_line(line).is_some_and(|h| h.at_ms >= keep_from)
            })?;
            append_line(&path, &hour.to_line())?;
            Ok(true)
        })
    }

    /// The hourly summaries of the `days` before `now_ms`, oldest first.
    pub fn hours_at(&self, now_ms: u64, days: u32) -> io::Result<Vec<HourSummary>> {
        let from = now_ms.saturating_sub(u64::from(days) * 86_400_000);
        let mut hours: Vec<HourSummary> =
            read_lines(&self.path(HOURS_FILE), HourSummary::from_line)?
                .into_iter()
                .filter(|h| h.at_ms >= from && h.at_ms <= now_ms)
                .collect();
        hours.sort_by_key(|h| h.at_ms);
        Ok(hours)
    }

    /// The usual memory over the `days` before `now_ms` ([`usual_of`]).
    pub fn usual_at(&self, now_ms: u64, days: u32) -> io::Result<Option<Usual>> {
        Ok(usual_of(&self.hours_at(now_ms, days)?))
    }
}

/// Appends an hour's summary to the machine root (see [`Store::append_hour`]).
pub fn append_hour(hour: &HourSummary) -> io::Result<bool> {
    Store::machine()?.append_hour(hour, &RealClock)
}

/// The machine root's hourly summaries from the last `days`.
pub fn hours(days: u32) -> io::Result<Vec<HourSummary>> {
    Store::machine()?.hours_at(RealClock.now_ms(), days)
}

/// The machine root's usual memory over the last `days` ([`usual_of`]); `None` also when the
/// machine root is unknown or the file cannot be read, so the runtime falls back to the first
/// minute's reading.
#[must_use]
pub fn usual(days: u32) -> Option<Usual> {
    Store::machine().ok()?.usual_at(RealClock.now_ms(), days).ok().flatten()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::PathBuf;

    use super::{
        HOURS_FILE, HOURS_TRIM_BYTES, HourSummary, USUAL_MIN_HOURS, USUAL_MIN_SAMPLES, Usual,
        usual_of,
    };
    use crate::clock::ManualClock;
    use crate::samples::{Source, Store};

    /// 2026-10-26T00:00:00Z.
    const NOW: u64 = 1_792_972_800_000;
    const HOUR_MS: u64 = 3_600_000;

    /// A full hour ending `hours_ago` hours before [`NOW`].
    fn hour(hours_ago: u64, avail_p50_mb: u32, plan_items_mb: u32) -> HourSummary {
        HourSummary {
            at_ms: NOW - hours_ago * HOUR_MS,
            avail_p10_mb: avail_p50_mb / 2,
            avail_p50_mb,
            avail_p90_mb: avail_p50_mb * 2,
            headroom_p10_mb: 5_000 + avail_p50_mb,
            paging_p75_kb_s: 2_000,
            plan_items_mb,
            slot_held_pct: 0,
            samples: 720,
            src: Source::Runtime,
        }
    }

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("eileen-buildguard-hours-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_hour_line_round_trips_in_the_documented_format() {
        let summary = HourSummary { slot_held_pct: 25, ..hour(0, 1_800, 300) };
        let line = summary.to_line();
        assert_eq!(
            line,
            r#"{"at_ms": 1792972800000, "avail_p10_mb": 900, "avail_p50_mb": 1800, "avail_p90_mb": 3600, "headroom_p10_mb": 6800, "paging_p75_kb_s": 2000, "plan_items_mb": 300, "slot_held_pct": 25, "samples": 720, "src": "runtime"}"#,
            "hour line"
        );
        assert_eq!(HourSummary::from_line(&line), Some(summary), "round trip");
        let too_big = line.replace("\"slot_held_pct\": 25", "\"slot_held_pct\": 256");
        assert_eq!(HourSummary::from_line(&too_big), None, "a share past a u8 is refused");
    }

    #[test]
    fn hour_lines_past_30_days_are_dropped() {
        let dir = root("trim");
        let store = Store::at(&dir);
        let path = dir.join(HOURS_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Hours from 40 days back, enough of them to take the file past 2 MB, then one from
        // 29 days back that is kept.
        let old_from = 40 * 24;
        let mut text = String::new();
        let mut hours_ago = old_from;
        while u64::try_from(text.len()).unwrap() <= HOURS_TRIM_BYTES {
            let line = hour(hours_ago, 1_000, 0).to_line();
            writeln!(text, "{line}").unwrap();
            // Keep every old line older than 30 days: cycle within days 31 to 40.
            hours_ago = if hours_ago == 31 * 24 { old_from } else { hours_ago - 1 };
        }
        let kept = hour(29 * 24, 1_000, 0);
        writeln!(text, "{}", kept.to_line()).unwrap();
        std::fs::write(&path, text).unwrap();
        let clock = ManualClock::new(NOW);
        assert!(store.append_hour(&hour(0, 1_000, 0), &clock).unwrap(), "the new hour is written");
        let left: Vec<u64> =
            store.hours_at(NOW, 60).unwrap().into_iter().map(|h| h.at_ms).collect();
        assert_eq!(left, [kept.at_ms, NOW], "only the hours of the last 30 days are left");
        let _ignored = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_line_for_the_same_hour_is_not_written() {
        let dir = root("one-hour");
        let store = Store::at(&dir);
        let clock = ManualClock::new(NOW);
        assert!(store.append_hour(&hour(1, 900, 0), &clock).unwrap(), "first");
        let same = HourSummary { at_ms: NOW - HOUR_MS + 1_000, ..hour(1, 950, 0) };
        assert!(!store.append_hour(&same, &clock).unwrap(), "within the hour: refused");
        assert!(store.append_hour(&hour(0, 1_000, 0), &clock).unwrap(), "the next hour");
        assert_eq!(store.hours_at(NOW, 1).unwrap().len(), 2, "two lines");
        let _ignored = std::fs::remove_dir_all(&dir);
    }

    /// A fixture week: 168 full hours whose medians run 400, 500, ... in a cycle of eight, with
    /// the plan's items at 300, 200 or 400.
    fn fixture_week() -> Vec<HourSummary> {
        (0..168)
            .map(|i| {
                let step = u32::try_from(i % 8).unwrap();
                let items = [300, 200, 400][usize::try_from(i % 3).unwrap()];
                hour(i, 400 + step * 100, items)
            })
            .collect()
    }

    #[test]
    fn usual_is_the_25th_percentile_of_hourly_medians_with_plan_items_added_back() {
        let usual = usual_of(&fixture_week()).unwrap();
        // 21 hours each at 400, 500, ..., 1100: the 42nd of 168 sorted medians is 500.
        assert_eq!(usual.available_mb, 500, "the 25th percentile of the hourly medians");
        assert_eq!(usual.plan_items_mb, 300, "the median of the plan's items");
        assert_eq!(usual.headroom_p10_mb, 5_500, "the usual 10th percentile of headroom");
        assert_eq!(usual.hours, 168, "every hour qualified");
        assert_eq!(usual.free_mb(), 800, "with the plan's items added back");
    }

    #[test]
    fn usual_leaves_out_hours_under_360_samples() {
        let mut week = fixture_week();
        // Forty short hours with very little free: counted, they would set the 25th percentile.
        week.extend(
            (0..40).map(|i| HourSummary { samples: USUAL_MIN_SAMPLES - 1, ..hour(200 + i, 50, 0) }),
        );
        let usual = usual_of(&week).unwrap();
        assert_eq!(usual.hours, 168, "the short hours are left out");
        assert_eq!(usual.available_mb, 500, "and do not move the figure");
        let mut exactly = fixture_week();
        exactly.extend(
            (0..100).map(|i| HourSummary { samples: USUAL_MIN_SAMPLES, ..hour(200 + i, 50, 0) }),
        );
        assert_eq!(usual_of(&exactly).unwrap().available_mb, 50, "360 samples count");
    }

    #[test]
    fn usual_is_none_with_fewer_than_24_qualifying_hours() {
        let day: Vec<HourSummary> = (0..24).map(|i| hour(i, 1_000, 100)).collect();
        assert_eq!(
            usual_of(&day),
            Some(Usual {
                hours: 24,
                available_mb: 1_000,
                plan_items_mb: 100,
                headroom_p10_mb: 6_000
            }),
            "24 hours are enough"
        );
        let short_of_a_day = &day[1..];
        assert_eq!(short_of_a_day.len(), USUAL_MIN_HOURS - 1, "23 hours");
        assert_eq!(usual_of(short_of_a_day), None, "23 are not");
        let mut padded = short_of_a_day.to_vec();
        padded.push(HourSummary { samples: 10, ..hour(30, 1_000, 100) });
        assert_eq!(usual_of(&padded), None, "and a short hour does not make up the day");
    }
}

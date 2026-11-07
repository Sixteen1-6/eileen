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
    pub(super) fn to_line(self) -> String {
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

    pub(super) fn from_line(line: &str) -> Option<Self> {
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

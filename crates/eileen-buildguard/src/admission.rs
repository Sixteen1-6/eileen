//! Admission: the pure rules that decide from memory samples whether a build may start.
//!
//! Section 6.1 rule 1, as functions of samples so tests drive them with fake memory:
//!
//! - **Slot 0's rule**, for a build that starts while no other build holds a slot, admits it
//!   when commit headroom, minus the build's own estimated commit (1 GB plus 0.5 GB per
//!   compile job of the ticket), stays at or above the floor over the last 30 s, and the 30 s
//!   paging average is at or under the slot-0 limit, whatever the available figure.
//! - **Slot 1's rule**, for a second build while another holds a slot (whichever index is
//!   free: slot 0's build may finish while slot 1's runs), admits it only when available
//!   memory is at least 1.5 GB plus 0.5 GB per compile job, the same headroom rule holds, and
//!   the running build has been sampled past its peak (its memory has stopped rising for
//!   30 s).
//! - The ticket's job count is [`jobs_for`]: `min(6, max(2, floor((available_gb - 1.5) /
//!   0.5)))`, ceiling 8 when one holder has both slots; it never falls as memory rises.

use std::fmt;

use crate::samples::{Baseline, Sample};

/// The slot-0 paging limit until the baseline exists, KB/s.
pub const DEFAULT_SLOT0_PAGING_KB_S: u32 = 5_000;
/// The re-check paging limit until the baseline exists, KB/s.
pub const DEFAULT_RECHECK_PAGING_KB_S: u32 = 20_000;
/// The headroom floor until the baseline exists, MB.
pub const DEFAULT_HEADROOM_FLOOR_MB: u32 = 2_048;
/// The window the rules look at, ms.
pub const WINDOW_MS: u64 = 30_000;
/// The newest sample of a usable window is younger than this, ms.
pub const FRESH_MS: u64 = 10_000;
/// How often a waiting guard samples, ms.
pub const SAMPLE_EVERY_MS: u64 = 5_000;
/// A window covers 30 s when its oldest sample is at least this old (one sampling interval
/// of slack), ms.
pub const COVERED_MS: u64 = WINDOW_MS - SAMPLE_EVERY_MS;

/// The limits the rules use: the baseline's, or the code defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// The slot-0 paging limit, KB/s.
    pub slot0_paging_kb_s: u32,
    /// The re-check paging limit, KB/s.
    pub recheck_paging_kb_s: u32,
    /// The one headroom floor for admission and the re-check, MB.
    pub headroom_floor_mb: u32,
    /// Whether they came from `buildguard.toml`.
    pub from_baseline: bool,
}

impl Thresholds {
    /// The code defaults: 5,000 KB/s for slot 0, 20,000 KB/s for the re-check, a 2 GB floor.
    pub const DEFAULT: Self = Self {
        slot0_paging_kb_s: DEFAULT_SLOT0_PAGING_KB_S,
        recheck_paging_kb_s: DEFAULT_RECHECK_PAGING_KB_S,
        headroom_floor_mb: DEFAULT_HEADROOM_FLOOR_MB,
        from_baseline: false,
    };

    /// The baseline's thresholds when there is one, else the defaults.
    #[must_use]
    pub const fn from_baseline(baseline: Option<&Baseline>) -> Self {
        match baseline {
            Some(b) => Self {
                slot0_paging_kb_s: b.slot0_paging_kb_s,
                recheck_paging_kb_s: b.recheck_paging_kb_s,
                headroom_floor_mb: b.headroom_floor_mb,
                from_baseline: true,
            },
            None => Self::DEFAULT,
        }
    }
}

/// The compile jobs a ticket gets at `avail_mb` of available memory.
#[must_use]
pub fn jobs_for(avail_mb: u32, both_slots: bool) -> u32 {
    let cap = if both_slots { 8 } else { 6 };
    (avail_mb.saturating_sub(1_536) / 512).clamp(2, cap)
}

/// A build's estimated commit, MB: 1 GB plus 0.5 GB per compile job.
#[must_use]
pub const fn estimated_commit_mb(jobs: u32) -> u32 {
    1_024_u32.saturating_add(jobs.saturating_mul(512))
}

/// The available memory slot 1 needs, MB: 1.5 GB plus 0.5 GB per compile job.
#[must_use]
pub const fn slot1_needs_mb(jobs: u32) -> u32 {
    1_536_u32.saturating_add(jobs.saturating_mul(512))
}

/// What the last 30 s of samples say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    /// Samples in the window.
    pub count: usize,
    /// The newest sample's age, ms.
    pub newest_age_ms: u64,
    /// The oldest sample's age, ms.
    pub oldest_age_ms: u64,
    /// The average paging rate, KB/s.
    pub avg_paging_kb_s: u32,
    /// The lowest headroom, MB.
    pub min_headroom_mb: u32,
    /// The newest sample's available memory, MB.
    pub latest_avail_mb: u32,
    /// The newest sample's headroom, MB.
    pub latest_headroom_mb: u32,
}

impl View {
    /// The samples of the 30 s before `now_ms`; `None` when there are none.
    #[must_use]
    pub fn of(samples: &[Sample], now_ms: u64) -> Option<Self> {
        let from = now_ms.saturating_sub(WINDOW_MS);
        let mut window: Vec<&Sample> =
            samples.iter().filter(|s| s.at_ms >= from && s.at_ms <= now_ms).collect();
        window.sort_by_key(|s| s.at_ms);
        let newest = *window.last()?;
        let oldest = *window.first()?;
        let count = window.len();
        let total: u64 = window.iter().map(|s| u64::from(s.paging_kb_s)).sum();
        let avg = total / u64::try_from(count).unwrap_or(1).max(1);
        Some(Self {
            count,
            newest_age_ms: now_ms - newest.at_ms,
            oldest_age_ms: now_ms - oldest.at_ms,
            avg_paging_kb_s: u32::try_from(avg).unwrap_or(u32::MAX),
            min_headroom_mb: window.iter().map(|s| s.headroom_mb).min().unwrap_or(0),
            latest_avail_mb: newest.avail_mb,
            latest_headroom_mb: newest.headroom_mb,
        })
    }

    /// True when the samples cover the last 30 s and the newest is under 10 s old, so a
    /// command can decide at once without sampling for itself.
    #[must_use]
    pub const fn covers(&self) -> bool {
        self.newest_age_ms < FRESH_MS && self.oldest_age_ms >= COVERED_MS
    }
}

/// Why a slot was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Commit headroom minus the build's estimate would fall under the floor.
    Headroom {
        /// The lowest headroom in the window, MB.
        headroom_mb: u32,
        /// The build's estimated commit, MB.
        estimate_mb: u32,
        /// The floor, MB.
        floor_mb: u32,
    },
    /// The 30 s paging average is over the limit.
    Paging {
        /// The average, KB/s.
        avg_kb_s: u32,
        /// The limit, KB/s.
        limit_kb_s: u32,
    },
    /// A second build (slot 1's rule) needs more available memory.
    Slot1Memory {
        /// Available memory, MB.
        avail_mb: u32,
        /// What it needs, MB.
        needs_mb: u32,
    },
    /// A second build waits until the running build is past its peak (or until the running
    /// build's holder record says when it started).
    Slot1BeforePeak,
    /// Both slots are held (or, for exclusive work, at least one is).
    SlotsTaken,
    /// Another desktop test or manual desktop session holds `desktop.lock`.
    DesktopTaken,
    /// The volume has less than 20 GB free.
    Disk {
        /// Free space, GB.
        free_gb: u64,
    },
    /// The samples do not cover 30 s yet.
    Sampling,
    /// A re-check during the build found memory too tight to launch the next cargo.
    Recheck(Box<Self>),
    /// Reading memory failed.
    Unreadable(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Headroom { headroom_mb, estimate_mb, floor_mb } => write!(
                f,
                "commit headroom {headroom_mb} MB minus this build's {estimate_mb} MB is under the {floor_mb} MB floor"
            ),
            Self::Paging { avg_kb_s, limit_kb_s } => {
                write!(
                    f,
                    "paging averaged {avg_kb_s} KB/s over 30 s, over the {limit_kb_s} KB/s limit"
                )
            }
            Self::Slot1Memory { avail_mb, needs_mb } => {
                write!(f, "a second build needs {needs_mb} MB available and {avail_mb} MB is")
            }
            Self::Slot1BeforePeak => {
                f.write_str("a second build waits until the running build is past its memory peak")
            }
            Self::SlotsTaken => f.write_str("the build slots are taken"),
            Self::DesktopTaken => f.write_str("another desktop session holds desktop.lock"),
            Self::Disk { free_gb } => {
                write!(f, "only {free_gb} GB of disk is free; builds need 20 GB")
            }
            Self::Sampling => f.write_str("sampling memory for 30 s"),
            Self::Recheck(inner) => write!(f, "re-check before the next cargo: {inner}"),
            Self::Unreadable(why) => write!(f, "memory could not be read: {why}"),
        }
    }
}

/// An admission decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Admitted with this many compile jobs.
    Admit {
        /// The ticket's job count.
        jobs: u32,
    },
    /// Refused.
    Refuse(Reason),
}

/// The headroom rule shared by both slots.
const fn headroom_ok(view: &View, jobs: u32, t: &Thresholds) -> Result<(), Reason> {
    let estimate_mb = estimated_commit_mb(jobs);
    if view.min_headroom_mb.saturating_sub(estimate_mb) < t.headroom_floor_mb
        || view.min_headroom_mb < estimate_mb
    {
        return Err(Reason::Headroom {
            headroom_mb: view.min_headroom_mb,
            estimate_mb,
            floor_mb: t.headroom_floor_mb,
        });
    }
    Ok(())
}

/// The paging rule.
const fn paging_ok(view: &View, t: &Thresholds) -> Result<(), Reason> {
    if view.avg_paging_kb_s > t.slot0_paging_kb_s {
        return Err(Reason::Paging {
            avg_kb_s: view.avg_paging_kb_s,
            limit_kb_s: t.slot0_paging_kb_s,
        });
    }
    Ok(())
}

/// Slot 0's rule, for a build that starts while no other build holds a slot.
#[must_use]
pub fn decide_slot0(view: &View, t: &Thresholds) -> Decision {
    let jobs = jobs_for(view.latest_avail_mb, false);
    match headroom_ok(view, jobs, t).and_then(|()| paging_ok(view, t)) {
        Ok(()) => Decision::Admit { jobs },
        Err(reason) => Decision::Refuse(reason),
    }
}

/// Slot 1's rule, for a second build while another holds a slot (whichever slot is free),
/// given whether the running build is past its peak.
#[must_use]
pub fn decide_slot1(view: &View, running_past_peak: bool, t: &Thresholds) -> Decision {
    let jobs = jobs_for(view.latest_avail_mb, false);
    let needs_mb = slot1_needs_mb(jobs);
    if view.latest_avail_mb < needs_mb {
        return Decision::Refuse(Reason::Slot1Memory { avail_mb: view.latest_avail_mb, needs_mb });
    }
    if let Err(reason) = headroom_ok(view, jobs, t) {
        return Decision::Refuse(reason);
    }
    if !running_past_peak {
        return Decision::Refuse(Reason::Slot1BeforePeak);
    }
    Decision::Admit { jobs }
}

/// Exclusive work's rule (both slots free): slot 0's rule and slot 1's memory requirement,
/// with the job ceiling at 8.
#[must_use]
pub fn decide_exclusive(view: &View, t: &Thresholds) -> Decision {
    let jobs = jobs_for(view.latest_avail_mb, true);
    let needs_mb = slot1_needs_mb(jobs);
    if view.latest_avail_mb < needs_mb {
        return Decision::Refuse(Reason::Slot1Memory { avail_mb: view.latest_avail_mb, needs_mb });
    }
    match headroom_ok(view, jobs, t).and_then(|()| paging_ok(view, t)) {
        Ok(()) => Decision::Admit { jobs },
        Err(reason) => Decision::Refuse(reason),
    }
}

/// The re-check while a build runs: too tight while the 30 s paging average is over the
/// re-check limit or headroom is under the floor.
#[must_use]
pub const fn recheck(view: &View, t: &Thresholds) -> Option<Reason> {
    if view.avg_paging_kb_s > t.recheck_paging_kb_s {
        return Some(Reason::Paging {
            avg_kb_s: view.avg_paging_kb_s,
            limit_kb_s: t.recheck_paging_kb_s,
        });
    }
    if view.latest_headroom_mb < t.headroom_floor_mb {
        return Some(Reason::Headroom {
            headroom_mb: view.latest_headroom_mb,
            estimate_mb: 0,
            floor_mb: t.headroom_floor_mb,
        });
    }
    None
}

/// Whether the build that started at `started_ms` is past its memory peak at `now_ms`: it
/// has been sampled for more than 30 s, and no sample in the last 30 s set a new low of
/// available memory.
#[must_use]
pub fn past_peak(samples: &[Sample], started_ms: u64, now_ms: u64) -> bool {
    let split = now_ms.saturating_sub(WINDOW_MS);
    let during: Vec<&Sample> =
        samples.iter().filter(|s| s.at_ms >= started_ms && s.at_ms <= now_ms).collect();
    let before = during.iter().filter(|s| s.at_ms < split).map(|s| s.avail_mb).min();
    let recent = during.iter().filter(|s| s.at_ms >= split).map(|s| s.avail_mb).min();
    match (before, recent) {
        (Some(before), Some(recent)) => recent >= before,
        (Some(_), None) | (None, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{
        Decision, Reason, Thresholds, View, decide_slot0, decide_slot1, jobs_for, past_peak,
    };
    use crate::samples::{Sample, Source};

    fn sample(at_ms: u64, avail_mb: u32, headroom_mb: u32, paging_kb_s: u32) -> Sample {
        Sample { at_ms, avail_mb, headroom_mb, paging_kb_s, slot_held: false, src: Source::Guard }
    }

    fn view(avail_mb: u32, headroom_mb: u32, paging_kb_s: u32) -> View {
        let samples: Vec<Sample> = (0..7)
            .map(|i| sample(100_000 + i * 5_000, avail_mb, headroom_mb, paging_kb_s))
            .collect();
        View::of(&samples, 130_001).unwrap()
    }

    #[test]
    fn jobs_are_two_up_to_two_and_a_half_gigabytes() {
        let gb = |tenths: u32| tenths * 1024 / 10;
        assert_eq!(jobs_for(gb(20), false), 2, "2.0 GB");
        assert_eq!(jobs_for(gb(24), false), 2, "2.4 GB");
        assert_eq!(jobs_for(gb(25), false), 2, "2.5 GB");
        assert_eq!(jobs_for(gb(30), false), 3, "3.0 GB");
        assert_eq!(jobs_for(gb(45), false), 6, "4.5 GB");
        assert_eq!(jobs_for(gb(90), false), 6, "capped at 6");
        assert_eq!(jobs_for(gb(90), true), 8, "8 with both slots");
        assert_eq!(jobs_for(0, false), 2, "never under 2");
    }

    proptest! {
        #[test]
        fn jobs_never_fall_as_memory_rises(a: u32, b: u32, both: bool) {
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(jobs_for(low, both) <= jobs_for(high, both), "monotonic");
        }
    }

    #[test]
    fn slot0_admits_low_memory_with_low_paging() {
        // 1.3 GB available, 5.5 GB headroom, light paging: admitted with 2 jobs.
        assert_eq!(
            decide_slot0(&view(1_331, 5_632, 300), &Thresholds::DEFAULT),
            Decision::Admit { jobs: 2 },
            "admitted"
        );
    }

    #[test]
    fn slot0_refuses_high_paging_and_tight_headroom() {
        let high = decide_slot0(&view(1_331, 5_632, 9_000), &Thresholds::DEFAULT);
        assert!(matches!(high, Decision::Refuse(Reason::Paging { .. })), "{high:?}");
        // 2 jobs estimate 2 GB of commit; 3.9 GB of headroom minus that is under the 2 GB floor.
        let tight = decide_slot0(&view(1_331, 3_993, 100), &Thresholds::DEFAULT);
        assert!(
            matches!(tight, Decision::Refuse(Reason::Headroom { estimate_mb: 2_048, .. })),
            "{tight:?}"
        );
    }

    #[test]
    fn slot1_needs_memory_and_a_past_peak() {
        let low = decide_slot1(&view(2_000, 9_000, 100), true, &Thresholds::DEFAULT);
        assert_eq!(
            low,
            Decision::Refuse(Reason::Slot1Memory { avail_mb: 2_000, needs_mb: 2_560 }),
            "below"
        );
        let early = decide_slot1(&view(5_000, 9_000, 100), false, &Thresholds::DEFAULT);
        assert_eq!(early, Decision::Refuse(Reason::Slot1BeforePeak), "before the peak");
        assert!(
            matches!(
                decide_slot1(&view(5_000, 9_000, 100), true, &Thresholds::DEFAULT),
                Decision::Admit { .. }
            ),
            "admitted"
        );
    }

    #[test]
    fn a_window_covers_thirty_seconds_with_a_fresh_sample() {
        let samples: Vec<Sample> = (0..7).map(|i| sample(i * 5_000, 1, 1, 1)).collect();
        assert!(View::of(&samples, 30_000).unwrap().covers(), "covered");
        assert!(!View::of(&samples, 41_000).unwrap().covers(), "stale");
        assert!(!View::of(&samples[4..], 30_000).unwrap().covers(), "too short");
    }

    #[test]
    fn the_peak_has_passed_once_memory_stops_falling_for_thirty_seconds() {
        let falling: Vec<Sample> =
            (0..13).map(|i| sample(i * 5_000, 5_000 - u32_of(i) * 100, 1, 1)).collect();
        assert!(!past_peak(&falling, 0, 60_000), "still falling");
        let mut levelled = falling;
        levelled.extend((13..20).map(|i| sample(i * 5_000, 4_000, 1, 1)));
        assert!(past_peak(&levelled, 0, 95_000), "levelled off");
        assert!(!past_peak(&levelled, 80_000, 95_000), "not sampled for 30 s yet");
    }

    fn u32_of(i: u64) -> u32 {
        u32::try_from(i).unwrap()
    }
}

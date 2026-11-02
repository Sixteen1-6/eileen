//! The simulated PC (BUILD.md section 4.7): its available memory, commit headroom and paging,
//! its CPU load and power, and a heavy app that opens, grows, holds its size and closes. The
//! world reads it every 5 s into `Input::Memory`, as the runtime samples memory (section 2.5).
//!
//! Everything here is a function of virtual time and of what Eileen's own processes hold, so a
//! run needs no random numbers for it and a replay reads the same figures from its recorded
//! inputs. Available memory is physical memory less what the other apps, the heavy app and
//! Eileen hold; the commit charge is what the other apps committed plus the heavy app and
//! Eileen, each of which commits what it holds; the PC pages in proportion to how far available
//! memory is under the profile's floor, a steady rate, so it is already the 30 s average the
//! runtime reports.
//!
//! The CPU's busy percent and the power state are what section 2.16's `MachineEvent::Load`
//! carries every 30 s; that input arrives with WP0's `wp0/machine-contracts`, so until then a
//! test reads them here ([`Pc::load`]) and the core is not told.

use eileen_contracts::ids::Personal;
use eileen_contracts::jobs::MemoryPressure;

use crate::profile::{EileenCosts, PcProfile, Power};

/// How often the runtime samples memory into `Input::Memory`, ms (section 2.5).
pub const MEMORY_SAMPLE_MS: u64 = 5_000;
/// Milliseconds in a second.
const MS_PER_S: u64 = 1_000;

/// Which of Eileen's processes hold memory now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EileenUse {
    /// The speech process runs: loading, ready or hung, until it ends.
    pub speech: bool,
    /// Worker sessions started and not yet exited.
    pub workers: u32,
}

/// The CPU and power at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    /// The CPU's busy percent.
    pub cpu_busy_pct: u8,
    /// Where the PC draws its power from.
    pub power: Power,
    /// Whether the heavy app is open.
    pub heavy_app_open: bool,
}

/// The simulated PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pc {
    profile: PcProfile,
    costs: EileenCosts,
}

impl Pc {
    /// The PC a profile describes, with Eileen's processes costing `costs`.
    #[must_use]
    pub fn new(profile: &PcProfile, costs: EileenCosts) -> Self {
        Self { profile: profile.clone(), costs }
    }

    /// The heavy app's working set at `now_ms`, MB: none before it opens or once it closed,
    /// growing at its rate in between until it reaches its peak.
    #[must_use]
    pub fn heavy_app_mb(&self, now_ms: u64) -> u32 {
        let Some(app) = &self.profile.heavy_app else { return 0 };
        let opens = u64::from(app.opens_at_s).saturating_mul(MS_PER_S);
        let closes = u64::from(app.closes_at_s).saturating_mul(MS_PER_S);
        if now_ms < opens || now_ms >= closes {
            return 0;
        }
        let grown = u64::from(app.grows_mb_s).saturating_mul(now_ms - opens) / MS_PER_S;
        clamp(grown.min(u64::from(app.peak_mb)))
    }

    /// What Eileen's processes hold, MB: the core and the brain always, the speech process
    /// while it runs, and each live worker session.
    #[must_use]
    pub fn eileen_mb(&self, using: EileenUse) -> u32 {
        let speech = if using.speech { self.costs.speech_mb } else { 0 };
        let workers = u64::from(self.costs.worker_mb).saturating_mul(u64::from(using.workers));
        clamp(
            u64::from(self.costs.core_mb)
                .saturating_add(u64::from(self.costs.brain_mb))
                .saturating_add(u64::from(speech))
                .saturating_add(workers),
        )
    }

    /// The memory reading the runtime would send at `now_ms` with Eileen using `using`.
    #[must_use]
    pub fn reading(&self, now_ms: u64, using: EileenUse) -> MemoryPressure {
        let pc = &self.profile;
        let heavy = u64::from(self.heavy_app_mb(now_ms));
        let eileen = u64::from(self.eileen_mb(using));
        let held = u64::from(pc.others_mb).saturating_add(heavy).saturating_add(eileen);
        let available = u64::from(pc.physical_mb).saturating_sub(held);
        let charge = u64::from(pc.others_commit_mb).saturating_add(heavy).saturating_add(eileen);
        let headroom = u64::from(pc.commit_limit_mb).saturating_sub(charge);
        let short = u64::from(pc.paging_floor_mb).saturating_sub(available);
        let paging = short.saturating_mul(u64::from(pc.paging_kb_s_per_mb));
        let largest = match &pc.heavy_app {
            Some(app) if heavy > u64::from(pc.largest_mb) => (app.name.clone(), clamp(heavy)),
            Some(_) | None => (pc.largest.clone(), pc.largest_mb),
        };
        MemoryPressure {
            available_mb: clamp(available),
            commit_headroom_mb: clamp(headroom),
            paging_kb_s: clamp(paging),
            paging_limit_kb_s: pc.paging_limit_kb_s,
            largest_process: Some((Personal::new(largest.0), largest.1)),
        }
    }

    /// The CPU and power at `now_ms`: the profile's base load, plus the heavy app's while it
    /// is open, at most 100 percent.
    #[must_use]
    pub fn load(&self, now_ms: u64) -> Load {
        let open = self.profile.heavy_app.as_ref().is_some_and(|app| {
            let opens = u64::from(app.opens_at_s).saturating_mul(MS_PER_S);
            let closes = u64::from(app.closes_at_s).saturating_mul(MS_PER_S);
            (opens..closes).contains(&now_ms)
        });
        let added = match &self.profile.heavy_app {
            Some(app) if open => app.cpu_pct,
            Some(_) | None => 0,
        };
        Load {
            cpu_busy_pct: self.profile.cpu_busy_pct.saturating_add(added).min(100),
            power: self.profile.power,
            heavy_app_open: open,
        }
    }
}

/// `value` as a `u32`, held at `u32::MAX`.
fn clamp(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use eileen_contracts::ids::Personal;

    use super::{EileenUse, Pc};
    use crate::profile::{Power, Profile};

    fn pc(name: &str) -> Pc {
        let profile = Profile::load(name).unwrap();
        Pc::new(&profile.pc, profile.eileen)
    }

    fn largest(pc: &Pc, now_ms: u64) -> (String, u32) {
        let (name, mb) = pc.reading(now_ms, EileenUse::default()).largest_process.unwrap();
        (name.into_inner(), mb)
    }

    #[test]
    fn the_heavy_app_opens_grows_holds_and_closes() {
        // low-memory: opens at 20 s, 50 MB/s up to 2,000 MB, closes at 120 s.
        let pc = pc("low-memory");
        assert_eq!(pc.heavy_app_mb(19_999), 0, "not yet open");
        assert_eq!(pc.heavy_app_mb(20_000), 0, "opens with nothing");
        assert_eq!(pc.heavy_app_mb(30_000), 500, "10 s at 50 MB/s");
        assert_eq!(pc.heavy_app_mb(60_000), 2_000, "at its peak after 40 s");
        assert_eq!(pc.heavy_app_mb(119_999), 2_000, "holds its peak");
        assert_eq!(pc.heavy_app_mb(120_000), 0, "closed");
        assert_eq!(pc("default").heavy_app_mb(60_000), 0, "the default PC has none");
    }

    #[test]
    fn memory_follows_the_apps_and_eileen() {
        let pc = pc("low-memory");
        let idle = EileenUse { speech: true, workers: 0 };
        // 16,384 less 12,600 others, 60 core, 400 brain and 1,250 speech.
        let before = pc.reading(0, idle);
        assert_eq!(before.available_mb, 2_074, "{before:?}");
        // 20,480 less 15,000 committed by others and 1,710 by Eileen.
        assert_eq!(before.commit_headroom_mb, 3_770, "{before:?}");
        assert_eq!(before.paging_kb_s, 0, "above the 1,000 MB floor, no paging");
        assert_eq!(before.paging_limit_kb_s, 4_000, "the effective limit is passed on");
        let peak = pc.reading(60_000, EileenUse { speech: true, workers: 2 });
        assert_eq!(peak.available_mb, 0, "2,074 less 2,000 and two workers of 400: {peak:?}");
        assert_eq!(peak.commit_headroom_mb, 970, "under 1 GB of commit left: {peak:?}");
        assert_eq!(peak.paging_kb_s, 40_000, "1,000 MB under the floor at 40 KB/s each");
        let after = pc.reading(120_000, idle);
        assert_eq!(after, before, "closing the app gives it all back");
        let no_speech = pc.reading(0, EileenUse::default());
        assert_eq!(no_speech.available_mb, 3_324, "the speech process's 1,250 MB is free");
    }

    #[test]
    fn the_biggest_process_is_the_heavy_app_once_it_outgrows_the_browser() {
        let pc = pc("low-memory");
        assert_eq!(largest(&pc, 0), ("browser".to_owned(), 3_000), "the browser at first");
        assert_eq!(largest(&pc, 60_000), ("browser".to_owned(), 3_000), "2,000 is smaller");
        let roomy = Profile::load("default").unwrap();
        let mut grown = roomy.pc.clone();
        grown.heavy_app = Profile::load("low-memory").unwrap().pc.heavy_app;
        if let Some(app) = grown.heavy_app.as_mut() {
            app.peak_mb = 4_000;
        }
        let pc = Pc::new(&grown, roomy.eileen);
        assert_eq!(largest(&pc, 100_000), ("video-editor".to_owned(), 4_000), "outgrown");
        let reading = pc.reading(100_000, EileenUse::default());
        assert_eq!(
            reading.largest_process,
            Some((Personal::new("video-editor".to_owned()), 4_000)),
            "named as the spoken reason would name it"
        );
    }

    #[test]
    fn the_cpu_carries_the_heavy_app_while_it_is_open() {
        let pc = pc("low-memory");
        let before = pc.load(0);
        assert_eq!((before.cpu_busy_pct, before.heavy_app_open), (30, false), "{before:?}");
        let open = pc.load(50_000);
        assert_eq!((open.cpu_busy_pct, open.heavy_app_open), (90, true), "30 plus 60: {open:?}");
        assert_eq!(pc.load(120_000).cpu_busy_pct, 30, "back once it closed");
        assert_eq!(before.power, Power::Ac, "on mains");
        assert_eq!(pc("media-playing").load(0).power, Power::Battery);
    }
}

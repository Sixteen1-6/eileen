//! The world's fault points (BUILD.md section 4.7): places where a simulated part misbehaves,
//! switched per run through `eileen_stdx::buggify`, a runtime switch and never a Cargo
//! feature.
//!
//! A seeded run turns them on ([`Faults::live`]): each site is active for the whole run or not,
//! decided from the seed and its name, and an active site fires each time it is reached with
//! the profile's chance. Every fire becomes a [`FaultRecord`] with the `seq` of the step it
//! fired in, so a `SimRegression` carries each fault as an explicit record. A replay injects
//! exactly those records ([`Faults::scripted`]) and never draws: a site fires in a replay when,
//! and as often as, a record names it at that `seq`. A site the core itself names (none yet)
//! would fire through the same switch; a run records it after its step ([`Faults::sync`]), and
//! a replay refuses it, since the world cannot reach inside the core to fire it.
//!
//! A test that breaks one part on purpose fires its site every time it is reached
//! ([`Faults::every`]), with the same records.
//!
//! Each site is reached only inside a step, while the world applies the step's input or its
//! effects, so the `seq` a record names is the one the site was reached at in the run and in
//! the replay.

use std::collections::{BTreeMap, BTreeSet};

use eileen_contracts::record::FaultRecord;
use eileen_stdx::buggify;

use crate::profile::FaultProfile;

/// The brain sends nothing for the profile's `stall_ms`, then its whole answer; an interrupt
/// reaches it only once the stall ends. Reached at each `BrainCmd::Turn`.
pub const BRAIN_STALL: &str = "sim.brain.stall";
/// The brain's turn ends in an error with no text. Reached at each `BrainCmd::Turn`.
pub const BRAIN_FAIL: &str = "sim.brain.fail";
/// The speech process crashes. Reached at each heartbeat it sends.
pub const SPEECH_CRASH: &str = "sim.speech.crash";
/// The speech process hangs and sends nothing more. Reached at each heartbeat it sends.
pub const SPEECH_HANG: &str = "sim.speech.hang";
/// The speech process's load takes [`crate::parts::SLOW_LOAD_MS`] longer. Reached at each
/// `Start` and `Restart`.
pub const SPEECH_SLOW_LOAD: &str = "sim.speech.slow_load";
/// A worker session stops part way through its job. Reached at each `WorkerCmd::Bind`.
pub const WORKER_CRASH: &str = "sim.worker.crash";
/// A worker session takes [`crate::parts::SLOW_WORKER_MS`] longer over its job. Reached at
/// each `WorkerCmd::Bind`.
pub const WORKER_SLOW: &str = "sim.worker.slow";
/// The slow bus subscriber takes nothing off its queue this step. Reached once a step.
pub const BUS_STALL: &str = "sim.bus.stall";

/// Every site the world has, with what a record of it says was injected.
pub const SITES: &[(&str, &str)] = &[
    (BRAIN_STALL, "the brain stalled, then sent its whole answer"),
    (BRAIN_FAIL, "the brain's turn failed with no text"),
    (SPEECH_CRASH, "the speech process crashed"),
    (SPEECH_HANG, "the speech process hung"),
    (SPEECH_SLOW_LOAD, "the speech process loaded slowly"),
    (WORKER_CRASH, "the worker session stopped part way"),
    (WORKER_SLOW, "the worker session ran slowly"),
    (BUS_STALL, "the bus subscriber took nothing this step"),
];

/// Whether `site` is one of the world's.
#[must_use]
pub fn is_world_site(site: &str) -> bool {
    SITES.iter().any(|(known, _)| *known == site)
}

/// What a record of `site` says was injected.
fn what(site: &str) -> &'static str {
    SITES.iter().find(|(known, _)| *known == site).map_or("fired", |(_, what)| what)
}

/// How a run decides its faults.
#[derive(Debug, Default)]
enum Mode {
    /// No fault fires: a world a test drives by hand.
    #[default]
    Off,
    /// `buggify` decides; the scope keeps it on for this thread until the world is dropped.
    Live {
        _scope: buggify::Scope,
        /// How often each site had fired at the last [`Faults::sync`].
        seen: BTreeMap<&'static str, u64>,
    },
    /// A replay: how many fires of each site are left at each `seq`.
    Scripted(BTreeMap<(u64, String), u32>),
    /// A hand-written test: these sites fire every time they are reached.
    Every(BTreeSet<&'static str>),
}

/// The world's fault points for one run, and the record of every fault that fired.
#[derive(Debug, Default)]
pub struct Faults {
    mode: Mode,
    records: Vec<FaultRecord>,
}

impl Faults {
    /// Faults that never fire.
    #[must_use]
    pub fn off() -> Self {
        Self::default()
    }

    /// Faults decided by `buggify` from `seed` with the profile's chances, switched on for
    /// this thread until the returned value is dropped; off when the profile turns them off.
    #[must_use]
    pub fn live(seed: u64, profile: FaultProfile) -> Self {
        if !profile.on() {
            return Self::off();
        }
        let scope = buggify::enable_with(seed, profile.activate_per_mille, profile.fire_per_mille);
        Self { mode: Mode::Live { _scope: scope, seen: BTreeMap::new() }, records: Vec::new() }
    }

    /// Exactly the faults `records` name, for a replay. `Err` names the first site the world
    /// cannot fire.
    pub fn scripted(records: &[FaultRecord]) -> Result<Self, String> {
        let mut left: BTreeMap<(u64, String), u32> = BTreeMap::new();
        for record in records {
            if !is_world_site(&record.site) {
                return Err(format!(
                    "fault site `{}` at seq {} is not one the world can fire",
                    record.site, record.seq
                ));
            }
            let count = left.entry((record.seq, record.site.clone())).or_insert(0);
            *count = count.saturating_add(1);
        }
        Ok(Self { mode: Mode::Scripted(left), records: Vec::new() })
    }

    /// Faults that fire every time one of `sites` is reached, for a test that breaks one part
    /// on purpose. `Err` names the first site the world does not have.
    pub fn every(sites: &[&'static str]) -> Result<Self, String> {
        if let Some(site) = sites.iter().find(|site| !is_world_site(site)) {
            return Err(format!("the world has no fault site `{site}`"));
        }
        Ok(Self { mode: Mode::Every(sites.iter().copied().collect()), records: Vec::new() })
    }

    /// Whether the world's fault point `site`, reached in the step `seq`, fires. `drawn` is
    /// what `buggify!(site)` answered there, which only a live run uses.
    pub fn hit(&mut self, seq: u64, site: &'static str, drawn: bool) -> bool {
        let fires = match &mut self.mode {
            Mode::Off => false,
            Mode::Live { seen, .. } => {
                if drawn {
                    let count = seen.entry(site).or_insert(0);
                    *count = count.saturating_add(1);
                }
                drawn
            }
            Mode::Scripted(left) => match left.get_mut(&(seq, site.to_owned())) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    true
                }
                Some(_) | None => false,
            },
            Mode::Every(sites) => sites.contains(site),
        };
        if fires {
            self.records.push(FaultRecord {
                seq,
                site: site.to_owned(),
                fault: what(site).to_owned(),
            });
        }
        fires
    }

    /// After the step `seq`: records every fire of a site that is not the world's (one the
    /// core names), which [`Faults::hit`] never saw.
    pub fn sync(&mut self, seq: u64) {
        let Mode::Live { seen, .. } = &mut self.mode else { return };
        for (site, fired) in buggify::fired() {
            let count = seen.entry(site).or_insert(0);
            while *count < fired {
                *count = count.saturating_add(1);
                self.records.push(FaultRecord {
                    seq,
                    site: site.to_owned(),
                    fault: what(site).to_owned(),
                });
            }
        }
    }

    /// Every fault that fired so far, in order.
    #[must_use]
    pub fn records(&self) -> &[FaultRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::record::FaultRecord;

    use super::{BRAIN_STALL, BUS_STALL, Faults, SITES, is_world_site};
    use crate::profile::FaultProfile;

    const ALWAYS: FaultProfile = FaultProfile { activate_per_mille: 1_000, fire_per_mille: 1_000 };

    #[test]
    fn off_never_fires_even_when_drawn() {
        let mut faults = Faults::off();
        assert!(!faults.hit(1, BRAIN_STALL, true), "off");
        assert!(faults.records().is_empty(), "nothing recorded");
        let quiet = FaultProfile { activate_per_mille: 1_000, fire_per_mille: 0 };
        let mut faults = Faults::live(1, quiet);
        assert!(!faults.hit(1, BRAIN_STALL, eileen_stdx::buggify!("sim.brain.stall")), "0 fires");
        assert!(!eileen_stdx::buggify::is_enabled(), "a profile with no chance switches none on");
    }

    #[test]
    fn a_live_run_records_each_fire_with_its_step() {
        let mut faults = Faults::live(7, ALWAYS);
        assert!(faults.hit(3, BRAIN_STALL, eileen_stdx::buggify!("sim.brain.stall")), "fires");
        assert!(faults.hit(3, BUS_STALL, eileen_stdx::buggify!("sim.bus.stall")), "fires");
        assert!(!faults.hit(4, BUS_STALL, false), "a site that was not drawn does not fire");
        faults.sync(3);
        assert_eq!(
            faults.records(),
            [
                FaultRecord {
                    seq: 3,
                    site: BRAIN_STALL.to_owned(),
                    fault: "the brain stalled, then sent its whole answer".to_owned(),
                },
                FaultRecord {
                    seq: 3,
                    site: BUS_STALL.to_owned(),
                    fault: "the bus subscriber took nothing this step".to_owned(),
                },
            ],
            "each fire once, with the step it fired in"
        );
        drop(faults);
        assert!(!eileen_stdx::buggify::is_enabled(), "dropping the faults switches them off");
    }

    #[test]
    fn a_site_the_core_names_is_recorded_after_its_step() {
        let mut faults = Faults::live(7, ALWAYS);
        assert!(eileen_stdx::buggify!("store.write"), "a site outside the world fires");
        faults.sync(9);
        assert_eq!(faults.records().len(), 1, "{:?}", faults.records());
        assert_eq!(faults.records()[0].site, "store.write", "named");
        assert_eq!(faults.records()[0].seq, 9, "at its step");
        faults.sync(10);
        assert_eq!(faults.records().len(), 1, "recorded once");
    }

    #[test]
    fn a_replay_fires_exactly_the_recorded_faults() {
        let record =
            |seq, site: &str| FaultRecord { seq, site: site.to_owned(), fault: String::new() };
        let records = [record(5, BUS_STALL), record(5, BUS_STALL), record(8, BRAIN_STALL)];
        let mut faults = Faults::scripted(&records).unwrap();
        assert!(!faults.hit(4, BUS_STALL, true), "not at seq 4, whatever was drawn");
        assert!(faults.hit(5, BUS_STALL, false), "the first at seq 5");
        assert!(faults.hit(5, BUS_STALL, false), "the second at seq 5");
        assert!(!faults.hit(5, BUS_STALL, false), "no third");
        assert!(!faults.hit(8, BUS_STALL, false), "another site at seq 8");
        assert!(faults.hit(8, BRAIN_STALL, false), "the brain's at seq 8");
        assert_eq!(faults.records().len(), 3, "the replay records what it fired");
        assert!(!eileen_stdx::buggify::is_enabled(), "a replay never draws");
    }

    #[test]
    fn a_replay_refuses_a_site_the_world_cannot_fire() {
        let record = FaultRecord { seq: 1, site: "store.write".to_owned(), fault: String::new() };
        let error = Faults::scripted(&[record]).unwrap_err();
        assert!(error.contains("store.write"), "{error}");
        assert!(SITES.iter().all(|(site, _)| is_world_site(site) && site.starts_with("sim.")));
    }

    #[test]
    fn a_test_can_fire_a_site_every_time() {
        let mut faults = Faults::every(&[BUS_STALL]).unwrap();
        assert!(faults.hit(1, BUS_STALL, false), "fires");
        assert!(faults.hit(2, BUS_STALL, false), "and again");
        assert!(!faults.hit(2, BRAIN_STALL, true), "only the sites named");
        assert_eq!(faults.records().len(), 2, "each fire recorded");
        assert!(Faults::every(&["store.write"]).is_err(), "a site the world does not have");
    }
}

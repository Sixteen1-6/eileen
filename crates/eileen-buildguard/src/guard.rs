//! Acquiring a build slot: the wait, the ticket, the re-check and exit code 75.
//!
//! [`acquire`] decides at once when the shared samples cover the last 30 s with one under
//! 10 s old; otherwise it samples every 5 s until they do. While a slot is refused it waits,
//! reporting the reason and the holders every 30 s, and after `wait_max` returns a
//! [`Refusal`] whose exit code is 75 ("no build slot; try again later"), so a background
//! agent can tell waiting from failing. Before each cargo invocation, [`Ticket::recheck`]
//! waits while memory is too tight and, after `recheck_wait_max`, releases the slot and
//! refuses the same way.

use std::fmt;
use std::time::Duration;

use crate::admission::{
    Decision, Reason, SAMPLE_EVERY_MS, Thresholds, View, decide_exclusive, decide_second_build,
    decide_slot0, past_peak, recheck,
};
use crate::clock::{Clock, millis};
use crate::disk::MIN_FREE_DISK_GB;
use crate::memory::MemorySource;
use crate::samples::{Sample, Source, Store};
use crate::slots::{self, Held, Holder, Slot};

/// The exit code of a refused or abandoned slot.
pub const EXIT_NO_SLOT: i32 = 75;
/// Set in cargo's environment so a nested `cargo xtask` reuses the slot instead of
/// deadlocking: `<slots>:<jobs>:<holder pid>`, slots being `0`, `1` or `0+1`.
pub const SLOT_HELD_ENV: &str = "EILEEN_BUILD_SLOT_HELD";
/// How long a refused command waits by default (`--wait-max`).
pub const DEFAULT_WAIT_MAX: Duration = Duration::from_mins(30);
/// How long a running build waits on its re-check by default (`--recheck-wait-max`).
pub const DEFAULT_RECHECK_WAIT_MAX: Duration = Duration::from_mins(20);
/// How often a running build re-checks.
pub const RECHECK_EVERY: Duration = Duration::from_secs(10);
/// How often a waiting command says why.
pub const REPORT_EVERY: Duration = Duration::from_secs(30);

/// What a guarded command asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The command, for the holder record (`cargo xtask test -p eileen-core`).
    pub command: String,
    /// The worktree, for the holder record.
    pub worktree: String,
    /// Both slots (release builds, speech models, soaks, benchmarks).
    pub exclusive: bool,
    /// How long to wait for a slot.
    pub wait_max: Duration,
    /// How long a running build waits on its re-check.
    pub recheck_wait_max: Duration,
}

impl Request {
    /// A request for one slot with the default waits.
    pub fn new(command: impl Into<String>, worktree: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            worktree: worktree.into(),
            exclusive: false,
            wait_max: DEFAULT_WAIT_MAX,
            recheck_wait_max: DEFAULT_RECHECK_WAIT_MAX,
        }
    }

    /// Asks for both slots.
    #[must_use]
    pub const fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }

    /// Sets `--wait-max`.
    #[must_use]
    pub const fn wait_max(mut self, wait: Duration) -> Self {
        self.wait_max = wait;
        self
    }

    /// Sets `--recheck-wait-max`.
    #[must_use]
    pub const fn recheck_wait_max(mut self, wait: Duration) -> Self {
        self.recheck_wait_max = wait;
        self
    }
}

/// No slot: the command exits with code 75.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The last reason.
    pub reason: Reason,
    /// Who held the slots.
    pub holders: Vec<Holder>,
    /// How long it waited.
    pub waited: Duration,
}

impl Refusal {
    /// 75: "no build slot; try again later".
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "every refusal exits 75; callers ask the refusal, not a constant"
    )]
    pub const fn exit_code(&self) -> i32 {
        EXIT_NO_SLOT
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no build slot; try again later ({}; waited {} s)",
            self.reason,
            self.waited.as_secs()
        )
    }
}

impl std::error::Error for Refusal {}

/// What a guarded command runs against: the machine files, memory, time, and where it says
/// why it waits.
pub struct Env<'a> {
    /// The machine-wide files.
    pub store: &'a Store,
    /// Where memory readings come from.
    pub memory: &'a mut dyn MemorySource,
    /// The clock the waits use.
    pub clock: &'a dyn Clock,
    /// Free disk on the worktree's volume, GB, when known.
    pub free_disk_gb: Option<u64>,
    /// The value of `EILEEN_BUILD_SLOT_HELD` in this process's environment.
    pub slot_held: Option<String>,
    /// This process's id, for the holder record.
    pub pid: u32,
    /// Called with the reason and the holders while waiting, at most every 30 s.
    pub report: &'a mut dyn FnMut(&Reason, &[Holder]),
}

impl fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("store", &self.store)
            .field("free_disk_gb", &self.free_disk_gb)
            .field("slot_held", &self.slot_held)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// Which slots a ticket holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slots {
    /// Slot 0.
    Zero,
    /// Slot 1.
    One,
    /// Both (exclusive work).
    Both,
    /// None of its own: an outer `cargo xtask` holds them (`EILEEN_BUILD_SLOT_HELD`).
    Nested,
}

impl Slots {
    /// The slots part of `EILEEN_BUILD_SLOT_HELD` for slots this ticket holds itself; `None`
    /// for a nested ticket, which passes on the outer command's value unchanged.
    const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Zero => Some("0"),
            Self::One => Some("1"),
            Self::Both => Some("0+1"),
            Self::Nested => None,
        }
    }
}

/// An admitted build. Dropping it releases its slots.
#[derive(Debug)]
pub struct Ticket {
    held: Vec<Held>,
    slots: Slots,
    jobs: u32,
    pid: u32,
    nested_value: Option<String>,
    thresholds: Thresholds,
    recheck_wait_max: Duration,
    local: Vec<Sample>,
}

impl Ticket {
    /// The compile jobs cargo gets (`CARGO_BUILD_JOBS`).
    #[must_use]
    pub const fn jobs(&self) -> u32 {
        self.jobs
    }

    /// The slots held.
    #[must_use]
    pub const fn slots(&self) -> Slots {
        self.slots
    }

    /// The thresholds admission used.
    #[must_use]
    pub const fn thresholds(&self) -> Thresholds {
        self.thresholds
    }

    /// The variables every cargo invocation under this ticket gets.
    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        let jobs = ("CARGO_BUILD_JOBS".to_owned(), self.jobs.to_string());
        let held = match (&self.nested_value, self.slots.as_str()) {
            (Some(outer), _) => outer.clone(),
            (None, Some(slots)) => format!("{slots}:{}:{}", self.jobs, self.pid),
            // A nested ticket always carries the outer value; without one there is none to pass.
            (None, None) => return vec![jobs],
        };
        vec![jobs, (SLOT_HELD_ENV.to_owned(), held)]
    }

    /// Takes one memory sample marked `slot_held`, for a running build's 10 s tick.
    pub fn sample(&mut self, env: &mut Env<'_>) {
        // A reading that fails is not a reason to stop a build that is already running.
        let _sample: Result<Sample, Reason> = take_sample(env, true, &mut self.local);
    }

    /// Before the next cargo invocation: waits while the 30 s paging average is over the
    /// re-check limit or headroom is under the floor (it never kills a running compile),
    /// re-checking every 10 s; after `recheck_wait_max` releases the slots and refuses.
    pub fn recheck(&mut self, env: &mut Env<'_>) -> Result<(), Refusal> {
        if self.slots == Slots::Nested {
            return Ok(());
        }
        let started = env.clock.mono_ms();
        let mut last_report: Option<u64> = None;
        loop {
            if take_sample(env, true, &mut self.local).is_err() {
                return Ok(());
            }
            let now = env.clock.now_ms();
            let Some(reason) =
                window(env, &self.local, now).and_then(|v| recheck(&v, &self.thresholds))
            else {
                return Ok(());
            };
            let reason = Reason::Recheck(Box::new(reason));
            let waited = env.clock.mono_ms().saturating_sub(started);
            if waited >= millis(self.recheck_wait_max) {
                self.held.clear();
                return Err(Refusal {
                    reason,
                    holders: Vec::new(),
                    waited: Duration::from_millis(waited),
                });
            }
            report(env, &reason, &[], &mut last_report);
            env.clock.sleep(RECHECK_EVERY);
        }
    }
}

/// Reports at most every 30 s.
fn report(env: &mut Env<'_>, reason: &Reason, holders: &[Holder], last: &mut Option<u64>) {
    let now = env.clock.mono_ms();
    if last.is_none_or(|at| now.saturating_sub(at) >= millis(REPORT_EVERY)) {
        (env.report)(reason, holders);
        *last = Some(now);
    }
}

/// Takes a reading and appends it to the shared samples, unless the reading is fake and the
/// store is the real machine root (a test with fake memory never writes the machine files);
/// then it is kept in `local` only. The sample is marked `slot_held` when this command holds a
/// slot or the store's probe finds either slot held ([`Store::slot_held`]).
fn take_sample(
    env: &mut Env<'_>,
    holds_slot: bool,
    local: &mut Vec<Sample>,
) -> Result<Sample, Reason> {
    let reading = env.memory.sample(env.clock).map_err(|e| Reason::Unreadable(e.to_string()))?;
    let sample = Sample {
        at_ms: env.clock.now_ms(),
        avail_mb: reading.avail_mb,
        headroom_mb: reading.headroom_mb,
        paging_kb_s: reading.paging_kb_s,
        slot_held: holds_slot || env.store.slot_held(),
        src: Source::Guard,
    };
    let shared = (env.memory.is_real() || !env.store.is_machine())
        && env.store.append(&sample, env.clock).is_ok();
    if !shared {
        local.push(sample);
    }
    Ok(sample)
}

/// The view of the last 30 s from the shared samples and this command's own.
fn window(env: &Env<'_>, local: &[Sample], now_ms: u64) -> Option<View> {
    let mut all = env.store.recent_at(now_ms, Duration::from_secs(35)).unwrap_or_default();
    all.extend(local.iter().copied());
    View::of(&all, now_ms)
}

/// The holder records of both slots.
fn holders(env: &Env<'_>) -> Vec<Holder> {
    Slot::ALL.iter().filter_map(|&slot| slots::slot_holder(env.store.root(), slot)).collect()
}

/// A valid `EILEEN_BUILD_SLOT_HELD`: its holder's record names that process and the lock is
/// held, so a stale variable inherited by an unrelated shell cannot bypass the guard.
fn nested_jobs(env: &Env<'_>) -> Option<u32> {
    let value = env.slot_held.as_deref()?;
    let mut parts = value.split(':');
    let slot = match parts.next()? {
        "0" | "0+1" => Slot::Zero,
        "1" => Slot::One,
        _ => return None,
    };
    let jobs: u32 = parts.next()?.parse().ok()?;
    let pid: u32 = parts.next()?.parse().ok()?;
    let root = env.store.root();
    let holds = slots::slot_holder(root, slot).is_some_and(|h| h.pid == pid)
        && slots::is_slot_held(root, slot).unwrap_or(false);
    (holds && jobs > 0).then_some(jobs)
}

/// Waits for a build slot as section 6.1 rule 1 says, or refuses with exit code 75.
pub fn acquire(request: &Request, env: &mut Env<'_>) -> Result<Ticket, Refusal> {
    let started = env.clock.mono_ms();
    let thresholds = Thresholds::from_baseline(env.store.baseline().ok().flatten().as_ref());
    if let Some(jobs) = nested_jobs(env) {
        return Ok(Ticket {
            held: Vec::new(),
            slots: Slots::Nested,
            jobs,
            pid: env.pid,
            nested_value: env.slot_held.clone(),
            thresholds,
            recheck_wait_max: request.recheck_wait_max,
            local: Vec::new(),
        });
    }
    if let Some(free_gb) = env.free_disk_gb.filter(|gb| *gb < MIN_FREE_DISK_GB) {
        return Err(Refusal {
            reason: Reason::Disk { free_gb },
            holders: holders(env),
            waited: Duration::ZERO,
        });
    }
    let mut local = Vec::new();
    let mut last_report = None;
    loop {
        let mut view = window(env, &local, env.clock.now_ms()).filter(View::covers);
        let mut sampling = Reason::Sampling;
        if view.is_none() {
            // Not enough fresh shared samples: sample for ourselves, then look again.
            if let Err(reason) = take_sample(env, false, &mut local) {
                sampling = reason;
            }
            view = window(env, &local, env.clock.now_ms()).filter(View::covers);
        }
        let reason = match view {
            Some(view) => match admit(request, env, &view, &thresholds) {
                Ok((held, slots, jobs)) => {
                    return Ok(Ticket {
                        held,
                        slots,
                        jobs,
                        pid: env.pid,
                        nested_value: None,
                        thresholds,
                        recheck_wait_max: request.recheck_wait_max,
                        local,
                    });
                }
                Err(reason) => reason,
            },
            None => sampling,
        };
        let waited = env.clock.mono_ms().saturating_sub(started);
        if waited >= millis(request.wait_max) {
            return Err(Refusal {
                reason,
                holders: holders(env),
                waited: Duration::from_millis(waited),
            });
        }
        if reason != Reason::Sampling {
            let holders = holders(env);
            report(env, &reason, &holders, &mut last_report);
        }
        env.clock.sleep(Duration::from_millis(SAMPLE_EVERY_MS));
    }
}

/// The job count of an admission, or its refusal.
fn admitted(decision: Decision) -> Result<u32, Reason> {
    match decision {
        Decision::Admit { jobs } => Ok(jobs),
        Decision::Refuse(reason) => Err(reason),
    }
}

/// One admission attempt against a covering window.
///
/// It decides from the slots as they stand and takes a lock only once the rules admit, so a
/// refused attempt holds nothing and writes no record. The rule depends on whether another
/// build runs, not on which index is free: with both slots free it is slot 0's rule; with one
/// held it is the second-build rule (slot 1's rule in section 6.1), for whichever slot is
/// free. A take that loses a race to another command refuses for this tick, and the next tick
/// decides again from the new state.
fn admit(
    request: &Request,
    env: &Env<'_>,
    view: &View,
    t: &Thresholds,
) -> Result<(Vec<Held>, Slots, u32), Reason> {
    let root = env.store.root();
    let unreadable = |e: std::io::Error| Reason::Unreadable(format!("slot lock: {e}"));
    let zero_held = slots::is_slot_held(root, Slot::Zero).map_err(unreadable)?;
    let one_held = slots::is_slot_held(root, Slot::One).map_err(unreadable)?;
    let holder = Holder {
        pid: env.pid,
        worktree: request.worktree.clone(),
        command: request.command.clone(),
        started_ms: env.clock.now_ms(),
    };
    let take = |slot| {
        slots::try_take_slot(root, slot, &holder).map_err(unreadable)?.ok_or(Reason::SlotsTaken)
    };
    if request.exclusive {
        if zero_held || one_held {
            return Err(Reason::SlotsTaken);
        }
        let jobs = admitted(decide_exclusive(view, t))?;
        let zero = take(Slot::Zero)?;
        let one = take(Slot::One)?;
        return Ok((vec![zero, one], Slots::Both, jobs));
    }
    let (free, running) = match (zero_held, one_held) {
        (false, false) => {
            let jobs = admitted(decide_slot0(view, t))?;
            return Ok((vec![take(Slot::Zero)?], Slots::Zero, jobs));
        }
        (false, true) => (Slot::Zero, Slot::One),
        (true, false) => (Slot::One, Slot::Zero),
        (true, true) => return Err(Reason::SlotsTaken),
    };
    // A held lock with no record is a take in progress or another command's brief probe, so
    // the running build's start is unknown: wait for the next tick rather than guess it.
    let Some(first) = slots::slot_holder(root, running) else {
        return Err(Reason::HolderUnknown);
    };
    let now = env.clock.now_ms();
    let during = env
        .store
        .recent_at(now, Duration::from_millis(now.saturating_sub(first.started_ms)))
        .unwrap_or_default();
    let past = past_peak(&during, first.started_ms, now);
    let jobs = admitted(decide_second_build(view, past, t))?;
    let held = match free {
        Slot::Zero => Slots::Zero,
        Slot::One => Slots::One,
    };
    Ok((vec![take(free)?], held, jobs))
}

/// Takes `desktop.lock` for a desktop test or `cargo xtask desktop-lock --hold`, waiting up to
/// `wait_max` and checking every 5 s; refuses with exit code 75 after that.
pub fn acquire_desktop(
    store: &Store,
    holder: &Holder,
    wait_max: Duration,
    clock: &dyn Clock,
    report: &mut dyn FnMut(&Reason, &[Holder]),
) -> Result<Held, Refusal> {
    let started = clock.mono_ms();
    let mut last_report: Option<u64> = None;
    loop {
        let reason = match slots::try_take(store.root(), slots::DESKTOP, holder) {
            Ok(Some(held)) => return Ok(held),
            Ok(None) => Reason::DesktopTaken,
            Err(e) => Reason::Unreadable(format!("desktop lock: {e}")),
        };
        let others: Vec<Holder> = slots::holder(store.root(), slots::DESKTOP).into_iter().collect();
        let waited = clock.mono_ms().saturating_sub(started);
        if waited >= millis(wait_max) {
            return Err(Refusal { reason, holders: others, waited: Duration::from_millis(waited) });
        }
        let now = clock.mono_ms();
        if last_report.is_none_or(|at| now.saturating_sub(at) >= millis(REPORT_EVERY)) {
            report(&reason, &others);
            last_report = Some(now);
        }
        clock.sleep(Duration::from_millis(SAMPLE_EVERY_MS));
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Env, Request, Slots, admit};
    use crate::admission::{Reason, Thresholds, View};
    use crate::clock::ManualClock;
    use crate::memory::FakeMemory;
    use crate::samples::{Sample, Source, Store};
    use crate::slots::{self, Holder, Slot};

    /// 2026-10-22T12:00:00Z.
    const NOW: u64 = 1_761_134_400_000;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("eileen-buildguard-guard-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Thirty seconds of steady samples ending now.
    fn view(avail_mb: u32, paging_kb_s: u32) -> View {
        let samples: Vec<Sample> = (0..7)
            .map(|i| Sample {
                at_ms: NOW - (6 - i) * 5_000,
                avail_mb,
                headroom_mb: 9_000,
                paging_kb_s,
                slot_held: false,
                src: Source::Guard,
            })
            .collect();
        View::of(&samples, NOW).unwrap()
    }

    /// One admission attempt for a plain command against `dir`.
    fn attempt(dir: &Path, view: &View) -> Result<(), Reason> {
        let store = Store::at(dir);
        let clock = ManualClock::new(NOW);
        let mut memory = FakeMemory::default();
        let mut report = |_: &Reason, _: &[Holder]| {};
        let env = Env {
            store: &store,
            memory: &mut memory,
            clock: &clock,
            free_disk_gb: None,
            slot_held: None,
            pid: std::process::id(),
            report: &mut report,
        };
        admit(&Request::new("cargo xtask test", "wt"), &env, view, &Thresholds::DEFAULT)
            .map(|_admitted| ())
    }

    #[test]
    fn a_held_slot_without_a_record_waits_for_its_holder() {
        let dir = root("no-record");
        let other = Holder {
            pid: 4_242,
            worktree: "other".to_owned(),
            command: "cargo xtask test".to_owned(),
            started_ms: 0,
        };
        let held = slots::try_take_slot(&dir, Slot::Zero, &other).unwrap().unwrap();
        std::fs::remove_file(dir.join("build-slot-0.holder")).unwrap();
        // Memory is plentiful, so only the unknown start of the running build refuses, and the
        // reason says so rather than claiming the build is before its peak.
        assert_eq!(attempt(&dir, &view(9_000, 100)), Err(Reason::HolderUnknown), "waits");
        drop(held);
        let _ignored = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_ticket_holding_its_own_slots_names_them() {
        assert_eq!(Slots::Zero.as_str(), Some("0"), "slot 0");
        assert_eq!(Slots::One.as_str(), Some("1"), "slot 1");
        assert_eq!(Slots::Both.as_str(), Some("0+1"), "both");
        assert_eq!(Slots::Nested.as_str(), None, "a nested ticket passes on the outer value");
    }

    #[test]
    fn a_refused_attempt_takes_no_lock_and_writes_no_record() {
        let dir = root("refused");
        let refused = attempt(&dir, &view(1_331, 9_000));
        assert!(matches!(refused, Err(Reason::Paging { .. })), "{refused:?}");
        // The lock files are created only by a take, so none exists after a refusal.
        for name in ["build-slot-0.lock", "build-slot-0.holder", "build-slot-1.lock"] {
            assert!(!dir.join(name).exists(), "{name} was never touched");
        }
        let _ignored = std::fs::remove_dir_all(&dir);
    }
}

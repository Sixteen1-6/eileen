//! The build guard as xtask uses it (BUILD.md section 6.1 rule 1): one build slot from
//! `eileen-buildguard` for a whole command, the re-check before each cargo invocation, the
//! reason and holders printed every 30 s while a slot is refused, exit code 75 after
//! `--wait-max`, and the guard's log.
//!
//! The log is one JSON object per line in `<machine root>/buildguard-log.jsonl`, one line per
//! admission and per refusal: the time, the event, the command, the slots, the job count, the
//! reason, and the last 30 s of shared samples (the paging average, the lowest headroom and the
//! newest available memory). Acceptance 4's owner's-PC half is read from it.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use eileen_buildguard::clock::{Clock as _, RealClock};
use eileen_buildguard::guard::RECHECK_EVERY;
use eileen_buildguard::slots::Holder;
use eileen_buildguard::{
    CargoInvocation, Env, Reason, Refusal, Request, SLOT_HELD_ENV, Slots, Store, SystemMemory,
    Ticket, acquire, acquire_desktop, disk, spawn_cargo,
};

use crate::cli::GuardOptions;

/// The guard's log, beside the machine-wide samples.
pub(crate) const LOG_FILE: &str = "buildguard-log.jsonl";

/// One cargo invocation of a guarded command.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    /// What the summary calls it (`clippy`, `nextest`).
    pub(crate) label: String,
    /// The invocation, before the ticket's variables are added.
    pub(crate) invocation: CargoInvocation,
    /// Whether it runs the `desktop` test group, which takes `desktop.lock` on Windows.
    pub(crate) desktop: bool,
}

impl Step {
    /// A step that runs `cargo` with `args`.
    pub(crate) fn cargo<I, S>(label: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        Self { label: label.to_owned(), invocation: CargoInvocation::cargo(args), desktop: false }
    }
}

/// How a guarded command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ran {
    /// No slot: exit 75.
    NoSlot,
    /// The steps that ran, in order, each with whether it passed.
    Steps(Vec<(String, bool)>),
}

impl Ran {
    /// Whether every step that ran passed (a refused slot is not a pass).
    pub(crate) fn passed(&self) -> bool {
        match self {
            Self::NoSlot => false,
            Self::Steps(steps) => steps.iter().all(|(_, passed)| *passed),
        }
    }
}

/// Runs `steps` under one slot of this machine: its samples, its memory and its clock. With
/// `keep_going`, every step runs and a failed one does not stop the rest.
pub(crate) fn run(
    command: &str,
    options: GuardOptions,
    steps: Vec<Step>,
    keep_going: bool,
    out: &mut dyn Write,
) -> io::Result<Ran> {
    let store = Store::machine()?;
    std::fs::create_dir_all(store.root())?;
    let clock = RealClock;
    let mut memory = SystemMemory::new(&clock)?;
    let worktree = crate::repo_root();
    let mut report = report_wait;
    let mut env = Env {
        store: &store,
        memory: &mut memory,
        clock: &clock,
        free_disk_gb: disk::free_disk_gb(&worktree),
        slot_held: std::env::var(SLOT_HELD_ENV).ok(),
        pid: std::process::id(),
        report: &mut report,
    };
    run_in(&request(command, &worktree, options), &mut env, steps, keep_going, out)
}

/// The request for `command` with the guard's options.
fn request(command: &str, worktree: &Path, options: GuardOptions) -> Request {
    let request = Request::new(command, worktree.display().to_string())
        .wait_max(options.wait_max)
        .recheck_wait_max(options.recheck_wait_max);
    if options.exclusive { request.exclusive() } else { request }
}

/// Runs `steps` under one slot of `env`.
fn run_in(
    request: &Request,
    env: &mut Env<'_>,
    steps: Vec<Step>,
    keep_going: bool,
    out: &mut dyn Write,
) -> io::Result<Ran> {
    let mut ticket = match acquire(request, env) {
        Ok(ticket) => ticket,
        Err(refusal) => {
            log_event(env, "refused", &request.command, None, Some(&refusal.reason));
            writeln!(out, "xtask: {}", refusal_text(&refusal))?;
            return Ok(Ran::NoSlot);
        }
    };
    log_event(env, "admitted", &request.command, Some(&ticket), None);
    writeln!(out, "xtask: {} with {} compile jobs", slots_text(ticket.slots()), ticket.jobs())?;
    let mut ran = Vec::new();
    for step in steps {
        if let Err(refusal) = ticket.recheck(env) {
            let reason = Some(&refusal.reason);
            log_event(env, "recheck_refused", &request.command, Some(&ticket), reason);
            writeln!(out, "xtask: {}", refusal_text(&refusal))?;
            return Ok(Ran::NoSlot);
        }
        let passed = run_step(&step, &mut ticket, env, request.wait_max, out)?;
        ran.push((step.label, passed));
        if !passed && !keep_going {
            break;
        }
    }
    Ok(Ran::Steps(ran))
}

/// Runs one step and says whether it passed. A desktop step that cannot take `desktop.lock`
/// within `wait_max` fails.
fn run_step(
    step: &Step,
    ticket: &mut Ticket,
    env: &mut Env<'_>,
    wait_max: Duration,
    out: &mut dyn Write,
) -> io::Result<bool> {
    let _desktop = if step.desktop && cfg!(windows) {
        let holder = Holder {
            pid: env.pid,
            worktree: crate::repo_root().display().to_string(),
            command: step.label.clone(),
            started_ms: env.clock.now_ms(),
        };
        let mut report = report_wait;
        match acquire_desktop(env.store, &holder, wait_max, env.clock, &mut report) {
            Ok(held) => Some(held),
            Err(refusal) => {
                writeln!(out, "xtask: {}", refusal_text(&refusal))?;
                return Ok(false);
            }
        }
    } else {
        None
    };
    let invocation =
        step.invocation.clone().with_env(ticket.env()).with_env(crate::machine::native_env());
    writeln!(out, "xtask: {}: {}", step.label, command_text(&invocation))?;
    out.flush()?;
    let mut child = spawn_cargo(&invocation)?;
    let clock = env.clock;
    let status = child.wait_ticking(RECHECK_EVERY, clock, &mut || ticket.sample(env))?;
    if !status.success() {
        writeln!(out, "xtask: {} failed ({status})", step.label)?;
    }
    Ok(status.success())
}

/// What a waiting command says every 30 s: the reason and who holds the slots.
fn report_wait(reason: &Reason, holders: &[Holder]) {
    let mut text = format!("xtask: waiting for a build slot: {reason}");
    push_holders(&mut text, holders);
    // Nobody is left to tell if standard error is closed; the wait goes on either way.
    let _reported: io::Result<()> = writeln!(io::stderr().lock(), "{text}");
}

/// The refusal as one line, with the exit code it leads to.
fn refusal_text(refusal: &Refusal) -> String {
    let mut text = format!("{refusal}; exit code {}", refusal.exit_code());
    push_holders(&mut text, &refusal.holders);
    text
}

fn push_holders(text: &mut String, holders: &[Holder]) {
    for holder in holders {
        // Writing to a String cannot fail.
        let _written: std::fmt::Result = write!(
            text,
            "; held by pid {} running `{}` in {}",
            holder.pid, holder.command, holder.worktree
        );
    }
}

/// "slot 0", "slots 0 and 1" or "the outer command's slot".
const fn slots_text(slots: Slots) -> &'static str {
    match slots {
        Slots::Zero => "slot 0",
        Slots::One => "slot 1",
        Slots::Both => "slots 0 and 1",
        Slots::Nested => "the outer command's slot",
    }
}

/// The slots field of a log line.
const fn slots_field(slots: Slots) -> &'static str {
    match slots {
        Slots::Zero => "0",
        Slots::One => "1",
        Slots::Both => "0+1",
        Slots::Nested => "nested",
    }
}

/// The command line, for the report.
fn command_text(invocation: &CargoInvocation) -> String {
    let mut text = invocation.program.display().to_string();
    for arg in &invocation.args {
        text.push(' ');
        text.push_str(&arg.to_string_lossy());
    }
    text
}

/// The last 30 s of shared samples: the paging average, the lowest headroom and the newest
/// available memory, each `None` when there are none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Window {
    /// The average paging rate, KB/s.
    pub(crate) paging_kb_s: Option<u32>,
    /// The lowest commit headroom, MB.
    pub(crate) headroom_min_mb: Option<u32>,
    /// The newest available memory, MB.
    pub(crate) avail_mb: Option<u32>,
}

/// One line of the guard's log.
pub(crate) fn log_line(
    at_ms: u64,
    event: &str,
    command: &str,
    ticket: Option<(Slots, u32)>,
    reason: Option<&Reason>,
    window: Window,
) -> String {
    serde_json::json!({
        "at_ms": at_ms,
        "event": event,
        "command": command,
        "slots": ticket.map(|(slots, _)| slots_field(slots)),
        "jobs": ticket.map(|(_, jobs)| jobs),
        "reason": reason.map(ToString::to_string),
        "paging_kb_s_30s": window.paging_kb_s,
        "headroom_min_mb_30s": window.headroom_min_mb,
        "avail_mb": window.avail_mb,
    })
    .to_string()
}

/// The last 30 s of `store`'s samples at `now_ms`.
fn window(store: &Store, now_ms: u64) -> Window {
    let samples = store.recent_at(now_ms, Duration::from_secs(30)).unwrap_or_default();
    let count = u64::try_from(samples.len()).unwrap_or(u64::MAX);
    let paging_sum: u64 = samples.iter().map(|s| u64::from(s.paging_kb_s)).sum();
    Window {
        paging_kb_s: paging_sum.checked_div(count).and_then(|avg| u32::try_from(avg).ok()),
        headroom_min_mb: samples.iter().map(|s| s.headroom_mb).min(),
        avail_mb: samples.iter().max_by_key(|s| s.at_ms).map(|s| s.avail_mb),
    }
}

/// Appends one line to the guard's log. A log that cannot be written never stops a build.
fn log_event(
    env: &Env<'_>,
    event: &str,
    command: &str,
    ticket: Option<&Ticket>,
    reason: Option<&Reason>,
) {
    let now = env.clock.now_ms();
    let ticket = ticket.map(|t| (t.slots(), t.jobs()));
    let line = log_line(now, event, command, ticket, reason, window(env.store, now));
    let appended = OpenOptions::new()
        .create(true)
        .append(true)
        .open(env.store.root().join(LOG_FILE))
        .and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()));
    if let Err(error) = appended {
        let _reported: io::Result<()> =
            writeln!(io::stderr().lock(), "xtask: cannot write the guard's log: {error}");
    }
}

#[cfg(test)]
mod tests {
    use eileen_buildguard::clock::ManualClock;
    use eileen_buildguard::slots;
    use eileen_buildguard::{FakeMemory, MemorySample};

    use super::*;
    use crate::repo::mem::ScratchGit;

    const fn quiet(_reason: &Reason, _holders: &[Holder]) {}

    /// Runs `steps` against a scratch machine root with `memory`.
    fn run_scratch(
        label: &str,
        memory: &mut FakeMemory,
        request: &Request,
        steps: Vec<Step>,
        keep_going: bool,
    ) -> (Ran, String, String) {
        let scratch = ScratchGit::new(label);
        let store = Store::at(scratch.path().join("machine"));
        std::fs::create_dir_all(store.root()).unwrap();
        let clock = ManualClock::new(1_000_000);
        let mut report = quiet;
        let mut env = Env {
            store: &store,
            memory,
            clock: &clock,
            free_disk_gb: Some(100),
            slot_held: None,
            pid: std::process::id(),
            report: &mut report,
        };
        let mut out = Vec::new();
        let ran = run_in(request, &mut env, steps, keep_going, &mut out).unwrap();
        let log = std::fs::read_to_string(store.root().join(LOG_FILE)).unwrap_or_default();
        assert!(!slots::any_slot_held(store.root()), "every slot is released afterwards");
        (ran, String::from_utf8(out).unwrap(), log)
    }

    #[test]
    fn a_log_line_names_the_event_the_slots_and_the_window() {
        let window =
            Window { paging_kb_s: Some(1_200), headroom_min_mb: Some(4_000), avail_mb: Some(1_300) };
        let line = log_line(1_000, "admitted", "cargo xtask test", Some((Slots::Zero, 2)), None, window);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["event"], "admitted", "the event");
        assert_eq!(value["slots"], "0", "slot 0");
        assert_eq!(value["jobs"], 2, "two compile jobs");
        assert_eq!(value["paging_kb_s_30s"], 1_200, "the window's paging average");
        assert_eq!(value["headroom_min_mb_30s"], 4_000, "the window's lowest headroom");
        assert_eq!(value["avail_mb"], 1_300, "the newest available memory");
        assert!(value["reason"].is_null(), "an admission has no reason");
        assert!(!line.contains('\n'), "one line");
    }

    #[test]
    fn an_admitted_command_runs_its_steps_and_logs_the_admission() {
        let mut memory = FakeMemory::steady(8_000, 16_000, 100);
        let request = Request::new("cargo xtask check", "worktree");
        let steps = vec![Step::cargo("version", ["--version"])];
        let (ran, out, log) = run_scratch("guard-admit", &mut memory, &request, steps, false);
        assert_eq!(ran, Ran::Steps(vec![("version".to_owned(), true)]), "cargo --version passed");
        assert!(ran.passed(), "every step passed");
        assert!(out.contains("compile jobs"), "it says what it was admitted with: {out}");
        assert!(log.contains("\"event\":\"admitted\""), "the admission is logged: {log}");
    }

    #[test]
    fn a_refused_command_exits_75_and_logs_the_refusal() {
        // Paging far over the default slot-0 limit for longer than the wait, so slot 0 is never
        // admitted.
        let busy = MemorySample { avail_mb: 8_000, headroom_mb: 16_000, paging_kb_s: 90_000 };
        let mut memory = FakeMemory::new(std::iter::repeat_n(busy, 2_000));
        let request =
            Request::new("cargo xtask check", "worktree").wait_max(Duration::from_secs(60));
        let steps = vec![Step::cargo("never", ["--version"])];
        let (ran, out, log) = run_scratch("guard-refuse", &mut memory, &request, steps, false);
        assert_eq!(ran, Ran::NoSlot, "no slot within a minute");
        assert!(!ran.passed(), "a refusal is not a pass");
        assert!(out.contains("exit code 75"), "it says why it stops: {out}");
        assert!(log.contains("\"event\":\"refused\""), "the refusal is logged: {log}");
    }

    #[test]
    fn a_failed_step_stops_the_rest_unless_asked_to_keep_going() {
        let steps = || {
            vec![
                Step::cargo("bad", ["no-such-cargo-command-for-xtask"]),
                Step::cargo("good", ["--version"]),
            ]
        };
        for (keep_going, expected) in [
            (false, vec![("bad".to_owned(), false)]),
            (true, vec![("bad".to_owned(), false), ("good".to_owned(), true)]),
        ] {
            let mut memory = FakeMemory::steady(8_000, 16_000, 100);
            let request = Request::new("cargo xtask ci", "worktree");
            let (ran, _, _) = run_scratch("guard-steps", &mut memory, &request, steps(), keep_going);
            assert_eq!(ran, Ran::Steps(expected), "keep going: {keep_going}");
        }
    }
}

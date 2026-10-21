//! Admission end to end with fake memory and a manual clock, against a temporary root.

use std::time::Duration;

use eileen_buildguard::admission::Reason;
use eileen_buildguard::clock::{Clock, ManualClock};
use eileen_buildguard::samples::{Baseline, Store};
use eileen_buildguard::slots::{self, Holder, Slot};
use eileen_buildguard::{EXIT_NO_SLOT, Env, FakeMemory, Refusal, Request, Slots, Ticket, acquire};

use crate::support::{TempRoot, prefill};

/// 2026-10-22T12:00:00Z.
const NOW: u64 = 1_761_134_400_000;

/// Runs `acquire` with fake memory, counting the reports.
fn run(
    store: &Store,
    memory: &mut FakeMemory,
    clock: &ManualClock,
    request: &Request,
) -> (Result<Ticket, Refusal>, usize) {
    let mut reports = 0;
    let mut report = |_: &Reason, _: &[Holder]| reports += 1;
    let mut env = Env {
        store,
        memory,
        clock,
        free_disk_gb: Some(300),
        slot_held: None,
        pid: std::process::id(),
        report: &mut report,
    };
    let result = acquire(request, &mut env);
    (result, reports)
}

fn other_holder() -> Holder {
    Holder {
        pid: 4_242,
        worktree: "other".to_owned(),
        command: "cargo xtask test".to_owned(),
        started_ms: NOW,
    }
}

#[test]
fn slot0_is_admitted_at_1_3_gb_with_low_paging_and_two_jobs_and_records_its_holder() {
    let root = TempRoot::new("admit");
    let store = Store::at(root.path());
    let clock = ManualClock::new(NOW);
    // No shared samples: the command samples for itself for 30 s, then decides.
    let mut memory = FakeMemory::steady(1_331, 5_632, 300);
    let (ticket, _) = run(&store, &mut memory, &clock, &Request::new("cargo xtask test", "wt"));
    let ticket = ticket.unwrap();
    assert_eq!(ticket.slots(), Slots::Zero, "slot 0");
    assert_eq!(ticket.jobs(), 2, "two compile jobs at 1.3 GB");
    assert!(clock.mono_ms() >= 25_000, "sampled for itself first");
    let holder = slots::slot_holder(root.path(), Slot::Zero).unwrap();
    assert_eq!(holder.pid, std::process::id(), "holder recorded");
    assert_eq!(holder.command, "cargo xtask test", "command recorded");
    assert!(
        ticket.env().contains(&("CARGO_BUILD_JOBS".to_owned(), "2".to_owned())),
        "jobs passed to cargo"
    );
    drop(ticket);
    assert!(!slots::is_slot_held(root.path(), Slot::Zero).unwrap(), "released on drop");
    assert!(slots::slot_holder(root.path(), Slot::Zero).is_none(), "record removed");
}

#[test]
fn a_command_with_fresh_shared_samples_decides_without_sampling() {
    let root = TempRoot::new("fresh");
    let store = Store::at(root.path());
    prefill(&store, NOW, 1_331, 5_632, 300);
    let clock = ManualClock::new(NOW + 2_000);
    // A fake with no readings: sampling would fail, so admission must come from the file.
    let mut memory = FakeMemory::default();
    let (ticket, _) = run(&store, &mut memory, &clock, &Request::new("cargo xtask check", "wt"));
    assert_eq!(ticket.unwrap().jobs(), 2, "admitted from the shared samples");
    assert_eq!(clock.mono_ms(), 0, "no waiting");
}

#[test]
fn slot0_is_refused_while_paging_is_high_and_exits_75_after_wait_max() {
    let root = TempRoot::new("paging");
    let store = Store::at(root.path());
    let clock = ManualClock::new(NOW);
    let mut memory = FakeMemory::steady(1_331, 5_632, 9_000);
    let request = Request::new("cargo xtask test", "wt").wait_max(Duration::from_secs(120));
    let (result, reports) = run(&store, &mut memory, &clock, &request);
    let refusal = result.unwrap_err();
    assert_eq!(refusal.exit_code(), EXIT_NO_SLOT, "exit code 75");
    assert!(
        matches!(refusal.reason, Reason::Paging { avg_kb_s: 9_000, limit_kb_s: 5_000 }),
        "{refusal}"
    );
    assert!(refusal.waited >= Duration::from_secs(120), "waited for --wait-max");
    assert!(reports >= 3, "said why every 30 s: {reports}");
    assert!(!slots::is_slot_held(root.path(), Slot::Zero).unwrap(), "nothing left held");
}

#[test]
fn slot0_is_refused_when_headroom_minus_the_estimate_is_under_the_floor() {
    let root = TempRoot::new("headroom");
    let store = Store::at(root.path());
    // 3.9 GB of headroom minus 2 GB for two jobs leaves less than the 2 GB floor.
    prefill(&store, NOW, 1_331, 3_993, 100);
    let clock = ManualClock::new(NOW);
    let request = Request::new("cargo xtask test", "wt").wait_max(Duration::ZERO);
    let (result, _) = run(&store, &mut FakeMemory::default(), &clock, &request);
    let reason = result.unwrap_err().reason;
    assert_eq!(
        reason,
        Reason::Headroom { headroom_mb: 3_993, estimate_mb: 2_048, floor_mb: 2_048 },
        "refused"
    );
}

#[test]
fn slot1_is_refused_below_its_requirement() {
    let root = TempRoot::new("slot1");
    let store = Store::at(root.path());
    prefill(&store, NOW, 2_000, 9_000, 100);
    let _first = slots::try_take_slot(root.path(), Slot::Zero, &other_holder()).unwrap().unwrap();
    let clock = ManualClock::new(NOW);
    let request = Request::new("cargo xtask test", "wt").wait_max(Duration::ZERO);
    let (result, _) = run(&store, &mut FakeMemory::default(), &clock, &request);
    let refusal = result.unwrap_err();
    assert_eq!(
        refusal.reason,
        Reason::Slot1Memory { avail_mb: 2_000, needs_mb: 2_560 },
        "below 2.5 GB"
    );
    assert_eq!(refusal.holders, vec![other_holder()], "the holder is named");
}

#[test]
fn exclusive_work_needs_both_slots() {
    let root = TempRoot::new("exclusive");
    let store = Store::at(root.path());
    prefill(&store, NOW, 9_000, 12_000, 100);
    let clock = ManualClock::new(NOW);
    let request = Request::new("cargo xtask bench", "wt").exclusive().wait_max(Duration::ZERO);
    let first = slots::try_take_slot(root.path(), Slot::One, &other_holder()).unwrap().unwrap();
    let (refused, _) = run(&store, &mut FakeMemory::default(), &clock, &request);
    assert_eq!(refused.unwrap_err().reason, Reason::SlotsTaken, "one slot is held");
    drop(first);
    let (ticket, _) = run(&store, &mut FakeMemory::default(), &clock, &request);
    let ticket = ticket.unwrap();
    assert_eq!((ticket.slots(), ticket.jobs()), (Slots::Both, 8), "both slots, up to 8 jobs");
}

#[test]
fn a_nested_command_reuses_the_outer_slot() {
    let root = TempRoot::new("nested");
    let store = Store::at(root.path());
    prefill(&store, NOW, 3_072, 9_000, 100);
    let clock = ManualClock::new(NOW);
    let outer =
        run(&store, &mut FakeMemory::default(), &clock, &Request::new("cargo xtask ci", "wt"))
            .0
            .unwrap();
    let held = outer.env().into_iter().find(|(k, _)| k == "EILEEN_BUILD_SLOT_HELD").unwrap().1;
    let mut report = |_: &Reason, _: &[Holder]| {};
    let mut memory = FakeMemory::default();
    let mut env = Env {
        store: &store,
        memory: &mut memory,
        clock: &clock,
        free_disk_gb: Some(300),
        slot_held: Some(held),
        pid: std::process::id(),
        report: &mut report,
    };
    let inner = acquire(&Request::new("cargo xtask test", "wt"), &mut env).unwrap();
    assert_eq!((inner.slots(), inner.jobs()), (Slots::Nested, 3), "reused, same jobs");
    env.slot_held = Some("0:3:4242".to_owned());
    let refused =
        acquire(&Request::new("cargo xtask test", "wt").wait_max(Duration::ZERO), &mut env);
    // A variable naming another process is ignored: this command waits for slot 1 instead.
    assert_eq!(refused.unwrap_err().reason, Reason::Slot1BeforePeak, "not nested");
    drop(outer);
}

#[test]
fn a_full_disk_refuses_at_once() {
    let root = TempRoot::new("disk");
    let store = Store::at(root.path());
    let clock = ManualClock::new(NOW);
    let mut report = |_: &Reason, _: &[Holder]| {};
    let mut memory = FakeMemory::default();
    let mut env = Env {
        store: &store,
        memory: &mut memory,
        clock: &clock,
        free_disk_gb: Some(12),
        slot_held: None,
        pid: std::process::id(),
        report: &mut report,
    };
    let refusal = acquire(&Request::new("cargo xtask test", "wt"), &mut env).unwrap_err();
    assert_eq!(refusal.exit_code(), 75, "exit code 75");
    assert_eq!(refusal.reason, Reason::Disk { free_gb: 12 }, "under 20 GB");
}

#[test]
fn a_recheck_past_its_wait_max_releases_the_slot_and_exits_75() {
    let root = TempRoot::new("recheck");
    let store = Store::at(root.path());
    prefill(&store, NOW, 3_072, 9_000, 100);
    let clock = ManualClock::new(NOW);
    let request = Request::new("cargo xtask ci", "wt").recheck_wait_max(Duration::from_secs(60));
    let mut ticket = run(&store, &mut FakeMemory::default(), &clock, &request).0.unwrap();
    // The build pages hard from now on; the good samples age out.
    clock.advance(Duration::from_secs(60));
    let mut memory = FakeMemory::steady(3_072, 9_000, 25_000);
    let mut reports = 0;
    let mut report = |_: &Reason, _: &[Holder]| reports += 1;
    let mut env = Env {
        store: &store,
        memory: &mut memory,
        clock: &clock,
        free_disk_gb: Some(300),
        slot_held: None,
        pid: std::process::id(),
        report: &mut report,
    };
    let refusal = ticket.recheck(&mut env).unwrap_err();
    assert_eq!(refusal.exit_code(), 75, "exit code 75");
    assert!(matches!(refusal.reason, Reason::Recheck(_)), "{refusal}");
    assert!(refusal.waited >= Duration::from_secs(60), "waited for --recheck-wait-max");
    assert!(!slots::is_slot_held(root.path(), Slot::Zero).unwrap(), "the slot was released");
    assert!(reports >= 1, "said why");
}

#[test]
fn a_recheck_passes_when_memory_is_fine() {
    let root = TempRoot::new("recheck-ok");
    let store = Store::at(root.path());
    prefill(&store, NOW, 3_072, 9_000, 100);
    let clock = ManualClock::new(NOW);
    let mut ticket =
        run(&store, &mut FakeMemory::default(), &clock, &Request::new("cargo xtask ci", "wt"))
            .0
            .unwrap();
    let mut memory = FakeMemory::steady(3_072, 9_000, 100);
    let mut report = |_: &Reason, _: &[Holder]| {};
    let mut env = Env {
        store: &store,
        memory: &mut memory,
        clock: &clock,
        free_disk_gb: Some(300),
        slot_held: None,
        pid: std::process::id(),
        report: &mut report,
    };
    ticket.recheck(&mut env).unwrap();
    assert_eq!(clock.mono_ms(), 0, "no wait");
}

#[test]
fn baseline_reads_back_the_keys_slots_baseline_wrote() {
    let root = TempRoot::new("baseline");
    let store = Store::at(root.path());
    assert_eq!(store.baseline().unwrap(), None, "none yet");
    let written = Baseline {
        measured_from: "2026-10-22".to_owned(),
        measured_to: "2026-10-23".to_owned(),
        samples: 17_280,
        slot_held_excluded: 412,
        slot0_paging_kb_s: 24_250,
        recheck_paging_kb_s: 48_500,
        headroom_floor_mb: 2_048,
    };
    store.write_baseline(&written).unwrap();
    assert_eq!(store.baseline().unwrap(), Some(written), "read back");
    let text = std::fs::read_to_string(root.path().join("buildguard.toml")).unwrap();
    for key in [
        "measured_from",
        "measured_to",
        "samples",
        "slot_held_excluded",
        "slot0_paging_kb_s",
        "recheck_paging_kb_s",
        "headroom_floor_mb",
    ] {
        assert!(text.lines().any(|l| l.starts_with(&format!("{key} = "))), "{key} written");
    }
}

#[test]
fn the_baseline_thresholds_replace_the_defaults() {
    let root = TempRoot::new("thresholds");
    let store = Store::at(root.path());
    store
        .write_baseline(&Baseline {
            measured_from: "2026-10-22".to_owned(),
            measured_to: "2026-10-22".to_owned(),
            samples: 1,
            slot_held_excluded: 0,
            slot0_paging_kb_s: 24_000,
            recheck_paging_kb_s: 48_000,
            headroom_floor_mb: 1_024,
        })
        .unwrap();
    // 19,400 KB/s would be refused by the 5,000 default; the baseline admits it.
    prefill(&store, NOW, 1_331, 5_632, 19_400);
    let clock = ManualClock::new(NOW);
    let request = Request::new("cargo xtask test", "wt").wait_max(Duration::ZERO);
    let ticket = run(&store, &mut FakeMemory::default(), &clock, &request).0.unwrap();
    assert!(ticket.thresholds().from_baseline, "from buildguard.toml");
}

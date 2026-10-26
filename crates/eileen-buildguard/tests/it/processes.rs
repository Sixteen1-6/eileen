//! The machine-wide files across processes, a stale holder, and what a guarded cargo runs in:
//! the kill-on-close job, or on Unix the process group its watchdog kills, and the priority.

use std::time::Duration;

use eileen_buildguard::clock::{Clock, RealClock};
#[cfg(windows)]
use eileen_buildguard::procs::Watched;
#[cfg(any(windows, target_os = "linux"))]
use eileen_buildguard::procs::process_alive;
use eileen_buildguard::samples::{PAGING_FILE, Source, Store};
use eileen_buildguard::slots::{self, Holder, Slot};

#[cfg(target_os = "linux")]
use crate::support::Watched;
use crate::support::{TempRoot, helper, read_ready, ready_helper, wait_until_free};

/// Holds `block.lock`, which keeps the blocking helpers alive until it is dropped.
fn hold_block(root: &TempRoot) -> slots::Held {
    let holder = Holder {
        pid: std::process::id(),
        worktree: String::new(),
        command: "test".to_owned(),
        started_ms: 0,
    };
    slots::try_take(root.path(), "block", &holder).unwrap().unwrap()
}

#[test]
fn a_sample_appended_by_another_process_is_read_back_by_recent() {
    let root = TempRoot::new("cross-sample");
    let status = helper("append-sample", &root, &[]).wait().unwrap();
    assert!(status.success(), "the runtime stand-in appended");
    let status = helper("read-recent", &root, &[]).wait().unwrap();
    assert!(status.success(), "samples::recent in another process read it back");
    let store = Store::at(root.machine());
    let mine = store.recent_at(RealClock.now_ms(), Duration::from_secs(600)).unwrap();
    assert!(mine.iter().any(|s| s.src == Source::Runtime), "and so does this one");
}

#[test]
fn two_processes_appending_one_window_leave_one_line() {
    let root = TempRoot::new("cross-paging");
    let at = RealClock.now_ms().to_string();
    let mut first = helper("append-paging", &root, &[("EILEEN_BG_AT", at.clone())]);
    let mut second = helper("append-paging", &root, &[("EILEEN_BG_AT", at)]);
    assert!(first.wait().unwrap().success(), "first runtime");
    assert!(second.wait().unwrap().success(), "second runtime");
    let text = std::fs::read_to_string(root.machine().join(PAGING_FILE)).unwrap();
    assert_eq!(text.lines().count(), 1, "one line for one window:\n{text}");
}

#[test]
fn kill_stale_frees_a_dead_holders_slot() {
    let root = TempRoot::new("stale");
    let block = hold_block(&root);
    let (mut holder, mut stdout) = ready_helper("hold-slot", &root);
    assert_eq!(read_ready(&mut stdout), "slot 0", "the helper took slot 0");
    let machine = root.machine();
    assert_eq!(
        slots::slot_holder(&machine, Slot::Zero).unwrap().pid,
        holder.id(),
        "its holder is recorded"
    );
    assert!(slots::is_slot_held(&machine, Slot::Zero).unwrap(), "held");
    holder.kill().unwrap();
    holder.wait().unwrap();
    wait_until_free(&machine, "build-slot-0.lock");
    assert!(!slots::is_slot_held(&machine, Slot::Zero).unwrap(), "the OS released the lock");
    let listed = slots::list(&machine).unwrap();
    assert_eq!(listed[0].holder_alive, Some(false), "the record names a dead holder");
    let freed = slots::kill_stale(&machine).unwrap();
    assert_eq!(freed.len(), 1, "one stale slot");
    assert_eq!(
        (freed[0].slot, freed[0].holder.pid),
        (Slot::Zero, holder.id()),
        "slot 0, the dead holder"
    );
    assert!(slots::slot_holder(&machine, Slot::Zero).is_none(), "record removed");
    drop(stdout);
    drop(block);
}

/// How long a test waits for a stand-in the guard should have killed; only a failing test
/// waits this out.
#[cfg(any(windows, target_os = "linux"))]
const END_WITHIN: Duration = Duration::from_secs(10);

/// The process ids on a stand-in cargo's ready line: its own, then its stand-in rustc's.
#[cfg(any(windows, target_os = "linux"))]
fn cargo_and_rustc(stdout: &mut impl std::io::BufRead) -> (u32, u32) {
    let pids: Vec<u32> =
        read_ready(stdout).split_whitespace().map(|p| p.parse().unwrap()).collect();
    assert_eq!(pids.len(), 2, "cargo and rustc: {pids:?}");
    (pids[0], pids[1])
}

/// Opens each running process, so its end is waited on rather than polled for.
#[cfg(any(windows, target_os = "linux"))]
fn watch(pids: &[u32]) -> Vec<(u32, Watched)> {
    assert!(pids.iter().all(|pid| process_alive(*pid)), "{pids:?} run");
    pids.iter().map(|pid| (*pid, Watched::open(*pid).unwrap())).collect()
}

#[cfg(any(windows, target_os = "linux"))]
fn assert_all_end(watched: &[(u32, Watched)]) {
    for (pid, process) in watched {
        assert!(process.wait_exit(END_WITHIN).unwrap(), "{pid} was left running");
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn killing_xtask_mid_build_leaves_no_rustc_running() {
    let root = TempRoot::new("kill-xtask");
    let block = hold_block(&root);
    let (mut xtask, mut stdout) = ready_helper("fake-xtask", &root);
    // The stand-in cargo is in xtask's job (on Unix, its process group); its stand-in rustc is
    // a plain child of cargo's, with no job or group of its own, as real cargo starts rustc.
    let (cargo, rustc) = cargo_and_rustc(&mut stdout);
    let watched = watch(&[cargo, rustc]);
    // Kill only the xtask process, as a crash or an agent's kill would; its job (on Unix, its
    // watchdog) takes the rest.
    xtask.kill_root().unwrap();
    xtask.wait().unwrap();
    assert_all_end(&watched);
    drop(stdout);
    drop(block);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn killing_a_guarded_cargo_takes_what_it_started() {
    let root = TempRoot::new("kill-cargo");
    let block = hold_block(&root);
    let (mut cargo, mut stdout) = ready_helper("fake-cargo", &root);
    let (cargo_pid, rustc) = cargo_and_rustc(&mut stdout);
    assert_eq!(cargo_pid, cargo.id(), "the guard's child is the stand-in cargo");
    let watched = watch(&[cargo_pid, rustc]);
    cargo.kill().unwrap();
    cargo.wait().unwrap();
    assert_all_end(&watched);
    drop(stdout);
    drop(block);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn dropping_a_guarded_cargo_takes_what_it_left_running() {
    let root = TempRoot::new("drop-cargo");
    let block = hold_block(&root);
    let (mut cargo, mut stdout) = ready_helper("fake-cargo-leaves", &root);
    let (_cargo, rustc) = cargo_and_rustc(&mut stdout);
    let watched = watch(&[rustc]);
    assert!(cargo.wait().unwrap().success(), "the stand-in cargo exited, leaving its rustc");
    drop(cargo);
    assert_all_end(&watched);
    drop(stdout);
    drop(block);
}

/// The nice value of `pid`, the 19th field of `/proc/<pid>/stat`.
#[cfg(target_os = "linux")]
fn nice_of(pid: u32) -> i32 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    // The fields after the parenthesised command name start with the 3rd, the state.
    let (_, rest) = stat.rsplit_once(')').unwrap();
    rest.split_whitespace().nth(19 - 3).unwrap().parse().unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn a_guarded_cargo_and_what_it_starts_run_ten_nice_levels_down() {
    let root = TempRoot::new("nice");
    let block = hold_block(&root);
    let (cargo, mut stdout) = ready_helper("fake-cargo", &root);
    let (cargo_pid, rustc) = cargo_and_rustc(&mut stdout);
    let expected = (nice_of(std::process::id()) + 10).min(19);
    assert_eq!(nice_of(cargo_pid), expected, "the stand-in cargo");
    assert_eq!(nice_of(rustc), expected, "its stand-in rustc, which inherits it");
    drop(cargo);
    drop(stdout);
    drop(block);
}

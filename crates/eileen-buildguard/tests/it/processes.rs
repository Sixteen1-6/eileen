//! The machine-wide files across processes, a stale holder, and the kill-on-close job.

use std::time::Duration;

use eileen_buildguard::clock::{Clock, RealClock};
use eileen_buildguard::samples::{PAGING_FILE, Source, Store};
use eileen_buildguard::slots::{self, Holder, Slot};

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

#[cfg(windows)]
#[test]
fn killing_xtask_mid_build_leaves_no_rustc_running() {
    use eileen_buildguard::procs::{Watched, process_alive};

    let root = TempRoot::new("kill-xtask");
    let block = hold_block(&root);
    let (mut xtask, mut stdout) = ready_helper("fake-xtask", &root);
    // The stand-in cargo is in xtask's job; its stand-in rustc is a plain child of cargo's,
    // with no job of its own, as real cargo starts rustc.
    let pids: Vec<u32> =
        read_ready(&mut stdout).split_whitespace().map(|p| p.parse().unwrap()).collect();
    assert_eq!(pids.len(), 2, "cargo and rustc: {pids:?}");
    assert!(pids.iter().all(|pid| process_alive(*pid)), "cargo and rustc run");
    // Opened while they run, so their ends are waited on rather than polled for.
    let watched: Vec<Watched> = pids.iter().map(|pid| Watched::open(*pid).unwrap()).collect();
    // Kill only the xtask process, as a crash or an agent's kill would; its job takes the rest.
    xtask.kill_root().unwrap();
    xtask.wait().unwrap();
    for (pid, process) in pids.iter().zip(&watched) {
        assert!(process.wait_exit(Duration::from_secs(10)).unwrap(), "{pid} was left running");
    }
    drop(stdout);
    drop(block);
}

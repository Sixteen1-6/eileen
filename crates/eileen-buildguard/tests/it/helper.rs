//! The helper processes the cross-process tests start: this test binary, run again with
//! `helper::entry --exact` and `EILEEN_BG_HELPER` naming what to do. Without the variable the
//! entry returns at once, so in an ordinary run it is an empty test.
//!
//! Helpers that must stay alive block on `block.lock`, which the test holds, instead of
//! sleeping; the test kills them or releases the lock.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::time::Duration;

use eileen_buildguard::clock::{Clock, RealClock};
use eileen_buildguard::samples::{self, PagingWindow, Sample, Source, Store};
use eileen_buildguard::slots::{self, Holder, Slot};
use eileen_buildguard::{CargoChild, CargoInvocation, spawn_cargo};

fn dir() -> PathBuf {
    PathBuf::from(std::env::var_os("EILEEN_BG_DIR").unwrap())
}

/// Blocks until the test releases `block.lock`.
fn block() {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir().join("block.lock"))
        .unwrap();
    lock.lock().unwrap();
}

/// Starts this binary again as the helper `mode`, in a kill-on-close job of this process.
fn child(mode: &str) -> CargoChild {
    let exe = std::env::current_exe().unwrap();
    let invocation =
        CargoInvocation::program(exe, ["helper::entry", "--exact", "--nocapture", "--quiet"])
            .with_env([("EILEEN_BG_HELPER", mode)])
            .quiet();
    spawn_cargo(&invocation).unwrap()
}

#[test]
fn entry() {
    let Some(mode) = std::env::var_os("EILEEN_BG_HELPER") else {
        return;
    };
    match mode.to_str().unwrap() {
        // Eileen's runtime writing one sample to the machine root.
        "append-sample" => samples::append(&Sample {
            at_ms: RealClock.now_ms(),
            avail_mb: 1_234,
            headroom_mb: 5_678,
            paging_kb_s: 42,
            slot_held: false,
            src: Source::Runtime,
        })
        .unwrap(),
        // Another process reading the runtime's sample back through the machine root.
        "read-recent" => {
            let recent = samples::recent(Duration::from_secs(600)).unwrap();
            assert!(
                recent.iter().any(|s| s.src == Source::Runtime && s.avail_mb == 1_234),
                "{recent:?}"
            );
        }
        // A runtime writing the 30 s window ending at EILEEN_BG_AT.
        "append-paging" => {
            let at_ms = std::env::var("EILEEN_BG_AT").unwrap().parse().unwrap();
            let window =
                PagingWindow { at_ms, paging_kb_s: 777, slot_held: false, src: Source::Runtime };
            samples::append_paging(&window).unwrap();
        }
        // A build holding slot 0 until it is killed.
        "hold-slot" => {
            let store = Store::machine().unwrap();
            let holder = Holder {
                pid: std::process::id(),
                worktree: "helper".to_owned(),
                command: "cargo xtask test".to_owned(),
                started_ms: RealClock.now_ms(),
            };
            let _held = slots::try_take_slot(store.root(), Slot::Zero, &holder).unwrap().unwrap();
            block();
        }
        // `cargo xtask` running a build: starts a stand-in cargo in its kill-on-close job.
        "fake-xtask" => {
            let _cargo = child("fake-cargo");
            block();
        }
        // The stand-in cargo: starts a stand-in rustc, writes both process ids, and waits.
        "fake-cargo" => {
            let rustc = child("fake-rustc");
            let pids = format!("{} {}", std::process::id(), rustc.id());
            std::fs::write(dir().join("pids.tmp"), pids).unwrap();
            std::fs::rename(dir().join("pids.tmp"), dir().join("pids")).unwrap();
            block();
            drop(rustc);
        }
        "fake-rustc" => block(),
        other => panic!("unknown helper {other}"),
    }
}

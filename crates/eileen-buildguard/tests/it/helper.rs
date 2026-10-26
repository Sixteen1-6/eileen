//! The helper processes the cross-process tests start: this test binary, run again with
//! `helper::entry --exact` and `EILEEN_BG_HELPER` naming what to do. Without the variable the
//! entry returns at once, so in an ordinary run it is an empty test.
//!
//! Helpers that must stay alive block on `block.lock`, which the test holds, instead of
//! sleeping; the test kills them or releases the lock. A helper the test must wait for says
//! `ready ...` on its stdout, which the test reads, so nothing polls.

use std::fs::OpenOptions;
use std::io::{BufReader, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use eileen_buildguard::clock::{Clock, RealClock};
use eileen_buildguard::samples::{self, PagingWindow, Sample, Source, Store};
use eileen_buildguard::slots::{self, Holder, Slot};
use eileen_buildguard::{CargoChild, CargoInvocation, spawn_cargo};

#[cfg(target_os = "linux")]
use crate::support::listen_for_end;
use crate::support::read_ready;

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

/// Says `ready <text>` on stdout, where the test (or the stand-in xtask) is reading.
fn say_ready(text: &str) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "ready {text}").unwrap();
    out.flush().unwrap();
}

/// This binary run again as the helper `mode`.
fn invocation(mode: &str) -> CargoInvocation {
    let exe = std::env::current_exe().unwrap();
    CargoInvocation::program(exe, ["helper::entry", "--exact", "--nocapture", "--quiet"])
        .with_env([("EILEEN_BG_HELPER", mode)])
}

/// Starts the stand-in rustc in this process's job or group and returns once it says ready
/// (on Linux, once it listens for a test to see it end).
fn fake_rustc() -> CargoChild {
    let mut rustc = spawn_cargo(&invocation("fake-rustc").piped_stdout().in_caller_job()).unwrap();
    assert_eq!(read_ready(&mut BufReader::new(rustc.take_stdout().unwrap())), "rustc");
    rustc
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
            say_ready("slot 0");
            block();
        }
        // `cargo xtask` running a build: starts a stand-in cargo in its kill-on-close job, as
        // `spawn_cargo` starts real cargo, and passes on the stand-in's ready line.
        "fake-xtask" => {
            let mut cargo = spawn_cargo(&invocation("fake-cargo").piped_stdout()).unwrap();
            let mut cargo_out = BufReader::new(cargo.take_stdout().unwrap());
            say_ready(&read_ready(&mut cargo_out));
            block();
        }
        // The stand-in cargo: starts a stand-in rustc the way real cargo does, as a plain child
        // with no job or group of its own (so only xtask's can take it), waits until it is
        // ready, says both process ids, and waits.
        "fake-cargo" => {
            #[cfg(target_os = "linux")]
            let _end = listen_for_end();
            let rustc = fake_rustc();
            say_ready(&format!("{} {}", std::process::id(), rustc.id()));
            block();
            drop(rustc);
        }
        // A stand-in cargo that exits once its rustc runs, leaving it running.
        "fake-cargo-leaves" => {
            let rustc = fake_rustc();
            say_ready(&format!("{} {}", std::process::id(), rustc.id()));
            // Never dropped, which would kill it.
            let _left_running = std::mem::ManuallyDrop::new(rustc);
        }
        "fake-rustc" => {
            #[cfg(target_os = "linux")]
            let _end = listen_for_end();
            say_ready("rustc");
            block();
        }
        other => panic!("unknown helper {other}"),
    }
}

//! Linux process tests: a child leads a session of its own, its whole process group dies when
//! it is dropped, when it exits and when the process that spawned it dies, `try_wait` reaps
//! through tokio, and children are forked on the supervisor's own thread. Processes the test
//! did not start itself are watched through pidfds, which name one process for good, and
//! killed by a guard when the test ends, however it ends.

use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, getpgrp, getsid, pidfd_open, pidfd_send_signal,
    waitid,
};
use tokio::io::AsyncWriteExt as _;

use super::{TempDir, current_thread, helper, hold_block, lock_file, read_ready, say_ready};
use crate::{ChildSpec, spawn};

/// The longest a test waits for a kill to land.
const KILL_LANDS: Duration = Duration::from_secs(10);

/// The helpers only these tests use.
pub(super) fn helper_entry(mode: &str, dir: &Path) {
    match mode {
        "session" => {
            let session = getsid(None).unwrap().as_raw_pid();
            say_ready(&format!("{session} {}", getpgrp().as_raw_pid()));
        }
        // A child that starts a grandchild the plain way, so it stays in the child's group.
        "parent" => {
            say_ready(&start_in_my_group(dir).id().to_string());
            lock_file(dir).lock().unwrap();
        }
        "parent-exits" => say_ready(&start_in_my_group(dir).id().to_string()),
        // Stands in for the core: spawns a supervised child and waits until it is killed. The
        // child's stdout is null, so the core's end closes no pipe it could die writing to.
        "core" => current_thread().block_on(async {
            let child = spawn(&ChildSpec { piped: false, ..helper("block", dir) }).unwrap();
            say_ready(&child.id().unwrap().to_string());
            lock_file(dir).lock().unwrap();
            drop(child);
        }),
        // Answers one line from stdin.
        "echo" => {
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).unwrap();
            say_ready(line.trim());
        }
        other => panic!("unknown helper {other}"),
    }
}

/// Starts a blocking grandchild with a plain spawn, which keeps it in this process's group.
fn start_in_my_group(dir: &Path) -> std::process::Child {
    crate::child::base_command(&helper("block", dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn pid(id: u32) -> Pid {
    Pid::from_raw(i32::try_from(id).unwrap()).unwrap()
}

/// A process watched through a pidfd and killed when the guard drops, so a failing test
/// leaves nothing running. Sending to a process that has ended does nothing.
struct Watched(OwnedFd);

impl Watched {
    /// Opens `id`, which must still be running (every watched helper blocks on `block.lock`).
    fn open(id: u32) -> Self {
        Self(pidfd_open(pid(id), PidfdFlags::empty()).unwrap())
    }

    /// Whether it ends within [`KILL_LANDS`]: a pidfd turns readable when its process exits.
    async fn ends(&self) -> bool {
        let pidfd = crate::ffi::watch_pidfd(self.0.try_clone().unwrap()).unwrap();
        tokio::time::timeout(KILL_LANDS, pidfd.readable()).await.is_ok()
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        let _ignored = pidfd_send_signal(&self.0, Signal::KILL);
    }
}

#[test]
fn a_child_leads_a_session_of_its_own() {
    let dir = TempDir::new("session");
    current_thread().block_on(async {
        let mut child = spawn(&helper("session", &dir.0)).unwrap();
        let id = i32::try_from(child.id().unwrap()).unwrap();
        let (said, _stdout) = read_ready(&mut child).await;
        let ids: Vec<i32> = said.split_whitespace().map(|n| n.parse().unwrap()).collect();
        assert_eq!(ids[1], id, "it leads its own process group");
        assert_ne!(ids[0], getsid(None).unwrap().as_raw_pid(), "it left the test's session");
        assert_eq!(ids[0], id, "it leads a new session, which has no controlling terminal");
        assert!(child.wait().await.unwrap().success(), "the helper ran to the end");
    });
}

#[test]
fn dropping_the_child_kills_its_whole_group() {
    let dir = TempDir::new("drop-group");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut child = spawn(&helper("parent", &dir.0)).unwrap();
        let (said, stdout) = read_ready(&mut child).await;
        let grandchild = Watched::open(said.parse().unwrap());
        assert_eq!(child.try_wait().unwrap(), None, "the child runs, blocked on the lock");
        drop(child);
        assert!(grandchild.ends().await, "the drop killed the grandchild in the child's group");
        drop(stdout);
    });
    drop(block);
}

#[test]
fn waiting_for_the_child_kills_what_is_left_of_its_group() {
    let dir = TempDir::new("wait-group");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut child = spawn(&helper("parent-exits", &dir.0)).unwrap();
        let (said, _stdout) = read_ready(&mut child).await;
        let grandchild = Watched::open(said.parse().unwrap());
        assert!(child.wait().await.unwrap().success(), "the child exited by itself");
        assert!(grandchild.ends().await, "its exit took the rest of its group with it");
        assert_eq!(child.id(), None, "reaped");
    });
    drop(block);
}

#[test]
fn try_wait_reaps_through_tokio_so_no_freed_pid_is_kept() {
    let dir = TempDir::new("try-wait");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut child = spawn(&helper("parent-exits", &dir.0)).unwrap();
        let id = child.id().unwrap();
        let (said, _stdout) = read_ready(&mut child).await;
        let grandchild = Watched::open(said.parse().unwrap());
        // Waits for the exit without reaping it, so the first try_wait finds it.
        waitid(WaitId::Pid(pid(id)), WaitIdOptions::EXITED | WaitIdOptions::NOWAIT).unwrap();
        let status = child.try_wait().unwrap().expect("the child has exited");
        assert!(status.success(), "{status:?}");
        assert_eq!(child.id(), None, "the freed pid is forgotten, so nothing signals it again");
        assert!(grandchild.ends().await, "the rest of its group was killed before the reap");
    });
    drop(block);
}

#[test]
fn a_child_dies_with_the_process_that_spawned_it() {
    let dir = TempDir::new("core-dies");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut core = spawn(&helper("core", &dir.0)).unwrap();
        let (said, _stdout) = read_ready(&mut core).await;
        let child = Watched::open(said.parse().unwrap());
        // SIGKILL, so nothing in the stand-in core runs: no destructor, no unwinding. The
        // child leads a session of its own, so this group kill does not reach it.
        core.start_kill().unwrap();
        core.wait().await.unwrap();
        assert!(child.ends().await, "the parent-death signal killed the core's child");
    });
    drop(block);
}

#[test]
fn a_child_outlives_the_thread_that_spawned_it() {
    let dir = TempDir::new("short-thread");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let runtime = tokio::runtime::Handle::current();
        let helper_dir = dir.0.clone();
        // On a thread that ends at once: the child, and a witness forked on that thread itself
        // with the same setup, so the thread's end kills the witness.
        let thread = eileen_stdx::thread::spawn_named("test.short-lived", move || {
            let _entered = runtime.enter();
            let child = spawn(&helper("echo", &helper_dir)).unwrap();
            let mut command = crate::child::base_command(&helper("block", &helper_dir));
            command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            crate::ffi::own_session_killed_with_parent(&mut command);
            (child, Watched::open(command.spawn().unwrap().id()))
        });
        let (mut child, witness) = thread.unwrap().join().unwrap();
        assert!(witness.ends().await, "the thread's end sent the witness its parent-death signal");
        // The thread's end sends every parent-death signal in one pass before the witness can
        // die, so a child forked on that thread would already be dying: it must answer.
        let mut stdin = child.take_stdin().unwrap();
        stdin.write_all(b"ping\n").await.unwrap();
        let (said, _stdout) = read_ready(&mut child).await;
        assert_eq!(said, "ping", "forked on the supervisor's own thread, the child lives on");
        assert!(child.wait().await.unwrap().success(), "and ends by itself");
    });
    drop(block);
}

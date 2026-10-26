//! The supervisor's only unsafe code on Linux: the setup a child runs between fork and exec,
//! and the registration of a child's pidfd with tokio's reactor. Every function here is safe
//! to call; each unsafe block holds one operation and says why its preconditions hold.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt as _;
use std::process::Command;

use rustix::io::Errno;
use rustix::process::{Signal, getpid, getppid, set_parent_process_death_signal, setsid};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;

/// Registers `pidfd` with the current tokio runtime's reactor for reading: a pidfd turns
/// readable when its process exits. Call it inside a runtime with I/O enabled.
pub(crate) fn watch_pidfd(pidfd: OwnedFd) -> io::Result<AsyncFd<OwnedFd>> {
    // SAFETY: an `OwnedFd` owns one open descriptor, and its `as_raw_fd` returns that same
    // descriptor until it is dropped, which only the `AsyncFd` that now owns it can do.
    unsafe { AsyncFd::register_with_interest(pidfd, Interest::READABLE) }.map_err(io::Error::from)
}

/// Makes the child of `command` lead a session of its own, which makes it the leader of a new
/// process group with no controlling terminal, and get `SIGKILL` when the thread that forks it
/// ends (the parent-death signal follows the forking thread, not the process).
pub(crate) fn own_session_killed_with_parent(command: &mut Command) {
    let parent = getpid();
    let setup = move || -> io::Result<()> {
        setsid()?;
        set_parent_process_death_signal(Some(Signal::KILL))?;
        // A parent that died before the signal was set has handed the child to another
        // process, so the signal would never come: the child must not run.
        if getppid() != Some(parent) {
            return Err(Errno::SRCH.into());
        }
        Ok(())
    };
    // SAFETY: `setup` runs in the forked child before exec, where only async-signal-safe
    // work is sound. It makes three raw system calls through rustix (setsid, prctl and
    // getppid) and turns a failure into an `io::Error` from its raw code; it allocates
    // nothing, takes no lock and reads only the pid it was given.
    unsafe { command.pre_exec(setup) };
}

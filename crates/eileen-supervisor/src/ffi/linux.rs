//! The supervisor's only unsafe code on Linux: the setup a child runs between fork and exec,
//! the registration of a child's pidfd with tokio's reactor, and, for the tests, reading a
//! process's I/O priority. Every function here is safe to call; each unsafe block holds one
//! operation and says why its preconditions hold.

use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt as _;
use std::process::Command;

use rustix::io::Errno;
use rustix::process::{
    Signal, getpid, getppid, getpriority_process, set_parent_process_death_signal,
    setpriority_process, setsid,
};
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;

/// Registers `pidfd` with the current tokio runtime's reactor for reading: a pidfd turns
/// readable when its process exits. Call it inside a runtime with I/O enabled.
pub(crate) fn watch_pidfd(pidfd: OwnedFd) -> io::Result<AsyncFd<OwnedFd>> {
    // SAFETY: an `OwnedFd` owns one open descriptor, and its `as_raw_fd` returns that same
    // descriptor until it is dropped, which only the `AsyncFd` that now owns it can do.
    unsafe { AsyncFd::register_with_interest(pidfd, Interest::READABLE) }.map_err(io::Error::from)
}

/// The nicest value a process can have.
const NICEST: i32 = 19;

// From the kernel's `include/uapi/linux/ioprio.h`, which libc 0.2.190 does not carry: the
// `which` value that names one task, where the class sits in an I/O priority, and the idle
// class, which has no levels.
const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_SHIFT: u32 = 13;
const IOPRIO_CLASS_IDLE: libc::c_int = 3;

/// The idle class as `ioprio_set` takes it and `ioprio_get` answers it.
pub(crate) const IDLE_IO_PRIORITY: libc::c_int = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;

/// How far below the thread that forks it a child runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lowered {
    /// Nice steps below the forking thread, at most at 19; zero keeps its value.
    pub(crate) nicer_by: i32,
    /// The idle I/O class instead of the forking thread's I/O priority.
    pub(crate) idle_io: bool,
}

impl Lowered {
    /// The forking thread's own priorities.
    pub(crate) const NONE: Self = Self { nicer_by: 0, idle_io: false };
}

/// Makes the child of `command` lead a session of its own, which makes it the leader of a new
/// process group with no controlling terminal, and get `SIGKILL` when the thread that forks it
/// ends (the parent-death signal follows the forking thread, not the process). With
/// `lowered.nicer_by` above zero it also runs that many nice steps below the forking thread, at
/// most at 19, and with `lowered.idle_io` in the idle I/O class: neither needs privilege (the
/// idle class has not since Linux 2.6.25), and everything the child starts inherits both.
pub(crate) fn own_session_killed_with_parent(command: &mut Command, lowered: Lowered) {
    let parent = getpid();
    let setup = move || -> io::Result<()> {
        setsid()?;
        set_parent_process_death_signal(Some(Signal::KILL))?;
        // A parent that died before the signal was set has handed the child to another
        // process, so the signal would never come: the child must not run.
        if getppid() != Some(parent) {
            return Err(Errno::SRCH.into());
        }
        if lowered.nicer_by > 0 {
            let now = getpriority_process(None)?;
            setpriority_process(None, now.saturating_add(lowered.nicer_by).min(NICEST))?;
        }
        if lowered.idle_io {
            set_idle_io_priority()?;
        }
        Ok(())
    };
    // SAFETY: `setup` runs in the forked child before exec, where only async-signal-safe
    // work is sound. It makes raw system calls (setsid, prctl, getppid, getpriority,
    // setpriority and ioprio_set) and turns a failure into an `io::Error` from its raw code;
    // it allocates nothing, takes no lock and reads only the two values it was given.
    unsafe { command.pre_exec(setup) };
}

/// Puts the calling thread, which in a child between fork and exec is the whole process, in
/// the idle I/O class. `ioprio_set` is a plain system call, so it is async-signal-safe.
fn set_idle_io_priority() -> io::Result<()> {
    // SAFETY: ioprio_set takes three integers and touches no memory of the caller's; a `who`
    // of 0 with IOPRIO_WHO_PROCESS names the calling thread, and the idle class with level 0
    // is a value the kernel accepts from any process.
    let done =
        unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, IDLE_IO_PRIORITY) };
    if done == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// The calling thread's I/O priority as `ioprio_get` answers it, for the helper children of
/// the tests.
#[cfg(test)]
pub(crate) fn io_priority() -> io::Result<libc::c_long> {
    // SAFETY: ioprio_get takes two integers and touches no memory of the caller's; a `who` of
    // 0 with IOPRIO_WHO_PROCESS names the calling thread.
    let priority = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, 0) };
    if priority < 0 { Err(io::Error::last_os_error()) } else { Ok(priority) }
}

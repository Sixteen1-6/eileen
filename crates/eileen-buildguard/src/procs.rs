//! Whether a process is still running, and (on Windows) waiting for one to end.

#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::time::Duration;

/// True while the process `pid` runs. Off Windows, from `/proc` (a zombie counts as gone).
#[must_use]
pub fn process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        crate::ffi::process_alive(pid)
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            // The state follows the parenthesised command name, which may itself hold spaces.
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .is_some_and(|s| s != "Z" && s != "X")
        })
    }
}

/// A process opened so its end can be waited on without polling: a test or a tool that did
/// not start the process (cargo's rustc, a slot holder) opens it while it runs, then blocks in
/// [`Self::wait_exit`]. Windows only; off Windows the guard's callers wait on their own
/// children.
#[cfg(windows)]
#[derive(Debug)]
pub struct Watched(std::os::windows::io::OwnedHandle);

#[cfg(windows)]
impl Watched {
    /// Opens `pid` for waiting. Fails when no such process runs.
    pub fn open(pid: u32) -> io::Result<Self> {
        crate::ffi::open_for_wait(pid).map(Self)
    }

    /// Waits up to `timeout` for the process to end; true once it has, false at the timeout.
    pub fn wait_exit(&self, timeout: Duration) -> io::Result<bool> {
        crate::ffi::wait_exit(&self.0, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::process_alive;

    #[test]
    fn this_process_is_alive_and_a_made_up_one_is_not() {
        assert!(process_alive(std::process::id()), "this process");
        assert!(!process_alive(u32::MAX - 1), "no such process");
    }

    #[cfg(windows)]
    #[test]
    fn a_process_that_exited_with_259_is_not_alive() {
        use crate::spawn::{CargoInvocation, spawn_cargo};

        // 259 is STILL_ACTIVE, which an exit-code check would read as "still running".
        let mut child =
            spawn_cargo(&CargoInvocation::program("cmd", ["/c", "exit", "259"]).quiet()).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(259), "exited with 259");
        // `child` still holds the process handle, so the exited process can still be opened.
        assert!(!process_alive(child.id()), "an exited process is not alive");
    }
}

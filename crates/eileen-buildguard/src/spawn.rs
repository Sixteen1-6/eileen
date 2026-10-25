//! `spawn_cargo`: cargo inside a container the guard kills it with, at a lower priority, so a
//! stopped build or a killed `cargo xtask` leaves no compiler running and the Claude app and the
//! voice loop stay responsive (section 6.1 rule 1).
//!
//! On Windows the child is created suspended, put into a fresh Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumed, so nothing it starts can escape the job.
//! The guard's process holds the only handle to the job, so when it ends, however it ends,
//! Windows closes the handle and terminates everything in the job.
//!
//! On Unix std has no job, no process-group kill and no `setpriority`, and the guard depends on
//! nothing but std, so it uses two programs every Unix has. A watchdog, `/bin/sh` running
//! `read _ignored; kill -s KILL 0`, starts first as the leader of a new process group, reading
//! a pipe whose only write end the guard keeps (close-on-exec, so cargo never holds it). Cargo
//! then joins that group through `nice -n 10`, so it and everything it starts run 10 nice
//! levels below the guard. When the write end closes, because the [`CargoChild`] is killed or
//! dropped or the guard's process ends however it ends, the watchdog's `read` returns and it
//! SIGKILLs the group: cargo, rustc, build scripts, test binaries and itself. The limits:
//!
//! - A process that makes a group or session of its own leaves cargo's: `cargo nextest` runs
//!   each test so, and a test that is running when the build stops runs to its end.
//! - The group is not the terminal's foreground group. Ctrl-C ends xtask, whose end stops the
//!   build (by SIGKILL, not SIGINT); Ctrl-Z pauses xtask and not the build; and under
//!   `stty tostop` the build stops at its first line to the terminal until xtask ends.
//!
//! [`CargoInvocation::in_caller_job`] starts an ordinary child instead, which stays in the
//! caller's own job (on Unix, its process group and nice value) the way real cargo's rustc
//! does; only a process already inside a guard's job uses it (the guard's tests, for their
//! stand-in rustc).

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::clock::{Clock, millis};

/// How often [`CargoChild::wait_ticking`] polls for the exit.
const POLL: Duration = Duration::from_millis(100);

/// The watchdog's script: wait for the guard's pipe to close, then SIGKILL its own process
/// group, which cargo joined. Nothing ever writes to the pipe, so `read` returns only then.
#[cfg(unix)]
const WATCHDOG: &str = "read _ignored; kill -s KILL 0";

/// The nice levels a guarded cargo runs below the guard (LINUX.md section 5).
#[cfg(unix)]
const NICE_INCREMENT: &str = "10";

/// One cargo run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoInvocation {
    /// The program: `cargo`, or a stand-in in tests.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Variables added to the inherited environment (the ticket's `CARGO_BUILD_JOBS` and
    /// `EILEEN_BUILD_SLOT_HELD`, the native-library paths).
    pub env: Vec<(OsString, OsString)>,
    /// The working folder, when not the current one.
    pub cwd: Option<PathBuf>,
    /// Null standard handles instead of the inherited ones.
    pub quiet: bool,
    /// A pipe for stdout ([`CargoChild::take_stdout`]), with null stdin and stderr.
    pub piped_stdout: bool,
    /// An ordinary child that stays in the caller's Job Object (on Unix, its process group and
    /// nice value) instead of one in a fresh container of its own (see [`Self::in_caller_job`]).
    pub in_caller_job: bool,
}

impl CargoInvocation {
    /// `cargo` with `args`.
    pub fn cargo<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self::program("cargo", args)
    }

    /// Any program with `args` (tests use their own binary as a stand-in for cargo).
    pub fn program<I, S>(program: impl Into<PathBuf>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            cwd: None,
            quiet: false,
            piped_stdout: false,
            in_caller_job: false,
        }
    }

    /// Adds environment variables.
    #[must_use]
    pub fn with_env<K, V>(mut self, vars: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.env.extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Null standard handles.
    #[must_use]
    pub const fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    /// Pipes the child's stdout to this process, with null stdin and stderr, so a caller can
    /// block on a line the child writes (a helper saying it is ready) instead of polling.
    #[must_use]
    pub const fn piped_stdout(mut self) -> Self {
        self.piped_stdout = true;
        self
    }

    /// Starts the program as an ordinary child, the way real cargo starts rustc: on Windows at
    /// below-normal priority in the caller's own Job Object, with none of its own, so the
    /// caller's kill-on-close job is what takes it; on Unix in the caller's process group at
    /// the caller's nice value, with no watchdog, so the caller's watchdog is what takes it.
    /// Only for a process that already runs inside a guard's job (the build guard's tests use
    /// it for their stand-in rustc, to prove the guard takes cargo's plain children); a guard
    /// never starts cargo this way.
    #[must_use]
    pub const fn in_caller_job(mut self) -> Self {
        self.in_caller_job = true;
        self
    }
}

/// A running cargo (or stand-in), killed with everything it started when dropped (on Unix,
/// everything still in its process group; see the module doc).
#[derive(Debug)]
pub struct CargoChild {
    child: Child,
    /// Its own kill-on-close job; `None` for a child started in the caller's job.
    #[cfg(windows)]
    job: Option<crate::ffi::Job>,
    /// The watchdog of its own process group, which kills the group when it is fired or
    /// dropped; `None` for a child started in the caller's group.
    #[cfg(unix)]
    watchdog: Option<Watchdog>,
}

/// Starts `invocation` in a kill-on-close Job Object at below-normal priority; on Unix, in a
/// process group of its own that a watchdog kills when the [`CargoChild`] or the guard's
/// process ends, through `nice -n 10` (see the module doc). On Unix a program `nice` cannot
/// start is not an error here: the child exits with 127 (126 when it is not executable).
#[expect(
    clippy::disallowed_methods,
    reason = "spawn_cargo: cargo inside the build guard's own kill-on-close Job Object (section 4b.2)"
)]
pub fn spawn_cargo(invocation: &CargoInvocation) -> io::Result<CargoChild> {
    #[cfg(unix)]
    let own_group = !invocation.in_caller_job;
    #[cfg(unix)]
    let mut command = if own_group {
        let mut nice = Command::new("nice");
        nice.args(["-n", NICE_INCREMENT, "--"]).arg(&invocation.program);
        nice
    } else {
        Command::new(&invocation.program)
    };
    #[cfg(not(unix))]
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.args);
    command.envs(invocation.env.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str())));
    if let Some(cwd) = &invocation.cwd {
        command.current_dir(cwd);
    }
    if invocation.quiet || invocation.piped_stdout {
        let stdout = if invocation.piped_stdout { Stdio::piped() } else { Stdio::null() };
        command.stdin(Stdio::null()).stdout(stdout).stderr(Stdio::null());
    }
    #[cfg(windows)]
    {
        start(command, !invocation.in_caller_job)
    }
    #[cfg(unix)]
    {
        start(command, own_group.then(|| Command::new("/bin/sh")))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Ok(CargoChild { child: command.spawn()? })
    }
}

#[cfg(windows)]
fn start(mut command: Command, own_job: bool) -> io::Result<CargoChild> {
    use std::os::windows::io::AsRawHandle as _;
    use std::os::windows::process::CommandExt as _;

    use windows_sys::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_SUSPENDED};

    if !own_job {
        command.creation_flags(BELOW_NORMAL_PRIORITY_CLASS);
        return Ok(CargoChild { child: command.spawn()?, job: None });
    }
    command.creation_flags(CREATE_SUSPENDED | BELOW_NORMAL_PRIORITY_CLASS);
    let job = crate::ffi::Job::kill_on_close()?;
    let mut child = command.spawn()?;
    let placed =
        job.assign(child.as_raw_handle()).and_then(|()| crate::ffi::resume_threads(child.id()));
    if let Err(e) = placed {
        let _killed: io::Result<()> = child.kill();
        let _reaped: io::Result<ExitStatus> = child.wait();
        return Err(e);
    }
    Ok(CargoChild { child, job: Some(job) })
}

/// Starts `command` as it is when there is no `watchdog`. Otherwise starts the watchdog first,
/// as the leader of a new process group reading a pipe only this process writes to, then
/// `command` in that group.
#[cfg(unix)]
fn start(mut command: Command, watchdog: Option<Command>) -> io::Result<CargoChild> {
    use std::os::unix::process::CommandExt as _;

    let Some(mut sh) = watchdog else {
        return Ok(CargoChild { child: command.spawn()?, watchdog: None });
    };
    let (reader, writer) = io::pipe()?;
    sh.args(["-c", WATCHDOG]).stdin(reader).stdout(Stdio::null()).stderr(Stdio::null());
    sh.process_group(0);
    let child = sh.spawn()?;
    // Closes this process's copy of the read end, which `sh` held; the watchdog has its own,
    // and the write end stays this process's alone.
    drop(sh);
    // Dropped, and so fired, if anything below fails: the group dies and the watchdog is reaped.
    let watchdog = Watchdog { child, writer: Some(writer) };
    let group = i32::try_from(watchdog.child.id()).map_err(io::Error::other)?;
    command.process_group(group);
    Ok(CargoChild { child: command.spawn()?, watchdog: Some(watchdog) })
}

/// A started watchdog and the write end of the pipe it reads.
#[cfg(unix)]
#[derive(Debug)]
struct Watchdog {
    child: Child,
    writer: Option<io::PipeWriter>,
}

#[cfg(unix)]
impl Watchdog {
    /// Closes the pipe and waits for the watchdog, which has killed its group, itself
    /// included, by the time it ends. Firing it again only reads the status back.
    fn fire(&mut self) -> io::Result<()> {
        drop(self.writer.take());
        self.child.wait().map(|_status| ())
    }
}

#[cfg(unix)]
impl Drop for Watchdog {
    fn drop(&mut self) {
        let _fired: io::Result<()> = self.fire();
    }
}

impl CargoChild {
    /// The child's process id.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Its stdout, the first time it is asked for, when it was started with
    /// [`CargoInvocation::piped_stdout`].
    pub const fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// The exit status, if it has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Waits for the exit.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait()
    }

    /// Waits for the exit, calling `tick` every `every` (a running build's 10 s re-check
    /// appends a memory sample there).
    pub fn wait_ticking(
        &mut self,
        every: Duration,
        clock: &dyn Clock,
        tick: &mut dyn FnMut(),
    ) -> io::Result<ExitStatus> {
        let mut next = clock.mono_ms().saturating_add(millis(every));
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if clock.mono_ms() >= next {
                tick();
                next = clock.mono_ms().saturating_add(millis(every));
            }
            clock.sleep(POLL);
        }
    }

    /// Kills the child and everything it started (its job, or on Unix its process group, and
    /// for a child in the caller's job or group the child alone).
    pub fn kill(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            match &self.job {
                Some(job) => job.terminate(),
                None => self.child.kill(),
            }
        }
        #[cfg(not(windows))]
        {
            #[cfg(unix)]
            if let Some(watchdog) = &mut self.watchdog {
                watchdog.fire()?;
            }
            self.child.kill()
        }
    }

    /// Kills only the child itself, as a crash of it would; on Windows its own Job Objects
    /// then close, and on Unix its watchdogs see their pipes close, and take what it started
    /// with them.
    pub fn kill_root(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

impl Drop for CargoChild {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _killed: io::Result<()> = self.kill();
            let _reaped: io::Result<ExitStatus> = self.child.wait();
        }
        // After this, on Windows the job's handle closes, and on Unix the watchdog is dropped
        // and so fired; either terminates anything left.
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn a_program_that_cannot_start_exits_127_through_nice() {
        use super::{CargoInvocation, spawn_cargo};

        let missing = CargoInvocation::program("/nonexistent/eileen-buildguard-test", [""; 0]);
        let mut child = spawn_cargo(&missing.quiet()).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(127), "nice could not start it");
    }
}

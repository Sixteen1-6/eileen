//! `spawn_cargo`: cargo inside the guard's own kill-on-close Job Object, at below-normal
//! priority, so a killed `cargo xtask` leaves no compiler running and the Claude app and the
//! voice loop stay responsive (section 6.1 rule 1).
//!
//! On Windows the child is created suspended, put into a fresh Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumed, so nothing it starts can escape the job.
//! The guard's process holds the only handle to the job, so when it ends, however it ends,
//! Windows closes the handle and terminates everything in the job. Off Windows (CI and
//! contributors) cargo is an ordinary child.
//!
//! [`CargoInvocation::in_caller_job`] starts an ordinary child instead, which stays in the
//! caller's own job the way real cargo's rustc does; only a process already inside a guard's
//! job uses it (the guard's tests, for their stand-in rustc).

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::clock::{Clock, millis};

/// How often [`CargoChild::wait_ticking`] polls for the exit.
const POLL: Duration = Duration::from_millis(100);

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
    /// On Windows, an ordinary child that stays in the caller's Job Object instead of one in a
    /// fresh kill-on-close job of its own (see [`Self::in_caller_job`]).
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

    /// Starts the program as an ordinary child at below-normal priority, the way real cargo
    /// starts rustc: on Windows it stays in the caller's own Job Object and gets none of its
    /// own, so the caller's kill-on-close job is what takes it. Only for a process that already
    /// runs inside a guard's job (the build guard's tests use it for their stand-in rustc, to
    /// prove the guard's job takes cargo's plain children); a guard never starts cargo this way.
    #[must_use]
    pub const fn in_caller_job(mut self) -> Self {
        self.in_caller_job = true;
        self
    }
}

/// A running cargo (or stand-in), killed with everything it started when dropped.
#[derive(Debug)]
pub struct CargoChild {
    child: Child,
    /// Its own kill-on-close job; `None` for a child started in the caller's job.
    #[cfg(windows)]
    job: Option<crate::ffi::Job>,
}

/// Starts `invocation` in a kill-on-close Job Object at below-normal priority.
#[expect(
    clippy::disallowed_methods,
    reason = "spawn_cargo: cargo inside the build guard's own kill-on-close Job Object (section 4b.2)"
)]
pub fn spawn_cargo(invocation: &CargoInvocation) -> io::Result<CargoChild> {
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
    #[cfg(not(windows))]
    {
        start(command)
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

#[cfg(not(windows))]
fn start(mut command: Command) -> io::Result<CargoChild> {
    Ok(CargoChild { child: command.spawn()? })
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

    /// Kills the child and everything it started (its job, or for a child in the caller's job
    /// the child alone).
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
            self.child.kill()
        }
    }

    /// Kills only the child itself, as a crash of it would; on Windows its own Job Objects
    /// then close and take what it started with them.
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
        // On Windows the job's handle closes after this, which terminates anything left.
    }
}

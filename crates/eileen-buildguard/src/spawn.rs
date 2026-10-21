//! `spawn_cargo`: cargo inside the guard's own kill-on-close Job Object, at below-normal
//! priority, so a killed `cargo xtask` leaves no compiler running and the Claude app and the
//! voice loop stay responsive (section 6.1 rule 1).
//!
//! On Windows the child is created suspended, put into a fresh Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, then resumed, so nothing it starts can escape the job.
//! The guard's process holds the only handle to the job, so when it ends, however it ends,
//! Windows closes the handle and terminates everything in the job. Off Windows (CI and
//! contributors) cargo is an ordinary child.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
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
}

/// A running cargo (or stand-in), killed with everything it started when dropped.
#[derive(Debug)]
pub struct CargoChild {
    child: Child,
    #[cfg(windows)]
    job: crate::ffi::Job,
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
    if invocation.quiet {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    }
    start(command)
}

#[cfg(windows)]
fn start(mut command: Command) -> io::Result<CargoChild> {
    use std::os::windows::io::AsRawHandle as _;
    use std::os::windows::process::CommandExt as _;

    use windows_sys::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_SUSPENDED};

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
    Ok(CargoChild { child, job })
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

    /// Kills the child and everything it started.
    pub fn kill(&mut self) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.job.terminate()
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

//! Supervised children: every child of the running product starts here (section 2.1 rule 1).
//!
//! A child is placed in a kill-on-close Job Object with no console window (process-wrap's
//! `JobObject`, `CreationFlags(CREATE_NO_WINDOW)` and `KillOnDrop`), so killing it kills
//! everything it started, a crash of Eileen takes every child with it, and dropping the
//! [`Child`] kills the tree. Claude sessions also run at `BELOW_NORMAL_PRIORITY_CLASS`, so a
//! worker's build never starves the voice loop. On Unix the child leads its own process group.
//! Stdout carries the protocol; [`Child::forward_stderr`] turns stderr into `child.stderr`.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};

use eileen_trace::names;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::task::JoinHandle;

use crate::SupervisorError;

/// The longest stderr line forwarded; longer lines are cut.
pub const MAX_STDERR_LINE: usize = 4 * 1024;

/// What kind of child, which sets its priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildClass {
    /// A sidecar (`eileen-speech`, `eileen-desktop`, `eileen-apps`, `eileen-hud`): normal priority.
    Sidecar,
    /// A Claude session (brain, spare, pooled or job worker, librarian): below normal.
    ClaudeSession,
    /// A short tool run (`fuscript`, a probe): normal priority.
    Tool,
}

impl ChildClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sidecar => "sidecar",
            Self::ClaudeSession => "claude_session",
            Self::Tool => "tool",
        }
    }
}

/// How to start one child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSpec {
    /// A short constant name for logs (`eileen-speech`, `claude.worker`).
    pub name: &'static str,
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Start from an empty environment (workers get exactly the one the harness builds).
    pub env_clear: bool,
    /// Variables to set.
    pub env: Vec<(OsString, OsString)>,
    /// The working folder, when not the current one.
    pub cwd: Option<PathBuf>,
    /// Its class.
    pub class: ChildClass,
    /// Pipe stdin and stdout (the protocol); otherwise they are null.
    pub piped: bool,
}

impl ChildSpec {
    /// A sidecar-class child with piped stdio and the inherited environment.
    pub fn new(name: &'static str, program: impl Into<PathBuf>) -> Self {
        Self {
            name,
            program: program.into(),
            args: Vec::new(),
            env_clear: false,
            env: Vec::new(),
            cwd: None,
            class: ChildClass::Sidecar,
            piped: true,
        }
    }

    /// Adds arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Adds environment variables.
    #[must_use]
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        self.env.extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Sets the class.
    #[must_use]
    pub const fn class(mut self, class: ChildClass) -> Self {
        self.class = class;
        self
    }
}

/// The one place Eileen creates a child command. Every other `Command::new` is banned by
/// clippy (check 14 lists the three exemptions outside this crate).
#[expect(
    clippy::disallowed_methods,
    reason = "the one Command::new for every child of the running product (BUILD.md section 4b.2)"
)]
pub(crate) fn base_command(spec: &ChildSpec) -> std::process::Command {
    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args);
    if spec.env_clear {
        command.env_clear();
    }
    command.envs(spec.env.iter().map(|(k, v)| (k.as_os_str(), v.as_os_str())));
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    command
}

/// A running supervised child.
#[derive(Debug)]
pub struct Child {
    name: &'static str,
    inner: Box<dyn ChildWrapper>,
}

/// Starts `spec` supervised: in a kill-on-close Job Object, without a console window, killed
/// when the returned [`Child`] is dropped.
pub fn spawn(spec: &ChildSpec) -> Result<Child, SupervisorError> {
    let mut command = tokio::process::Command::from(base_command(spec));
    if spec.piped {
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
    } else {
        command.stdin(Stdio::null()).stdout(Stdio::null());
    }
    command.stderr(Stdio::piped());
    let mut wrap = CommandWrap::from(command);
    wrap_platform(&mut wrap, spec.class);
    wrap.wrap(KillOnDrop);
    let inner = wrap.spawn().map_err(|source| SupervisorError::Spawn {
        program: spec.program.display().to_string(),
        source,
    })?;
    let child = Child { name: spec.name, inner };
    tracing::info!(name: names::CHILD_SPAWNED, child = spec.name, pid = child.id(), class = spec.class.as_str());
    Ok(child)
}

#[cfg(windows)]
fn wrap_platform(wrap: &mut CommandWrap, class: ChildClass) {
    use process_wrap::tokio::{CreationFlags, JobObject};
    use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};

    let flags = match class {
        ChildClass::ClaudeSession => CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS,
        ChildClass::Sidecar | ChildClass::Tool => CREATE_NO_WINDOW,
    };
    wrap.wrap(CreationFlags(flags)).wrap(JobObject);
}

#[cfg(unix)]
fn wrap_platform(wrap: &mut CommandWrap, _class: ChildClass) {
    wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
}

impl Child {
    /// The child's process id, while it runs.
    #[must_use]
    pub fn id(&self) -> Option<u32> {
        self.inner.id()
    }

    /// Its stdin, the first time it is asked for.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.inner.stdin().take()
    }

    /// Its stdout, the first time it is asked for.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.inner.stdout().take()
    }

    /// Its stderr, the first time it is asked for (unless [`Self::forward_stderr`] took it).
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.inner.stderr().take()
    }

    /// Forwards stderr into tracing, one `child.stderr` event per line, until it closes.
    pub fn forward_stderr(&mut self) -> Option<JoinHandle<()>> {
        let stderr = self.take_stderr()?;
        let child = self.name;
        Some(eileen_rt::task::spawn("supervisor.stderr", async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(mut line)) = lines.next_line().await {
                if line.len() > MAX_STDERR_LINE {
                    let cut = (0..=MAX_STDERR_LINE)
                        .rev()
                        .find(|&i| line.is_char_boundary(i))
                        .unwrap_or(0);
                    line.truncate(cut);
                }
                tracing::info!(name: names::CHILD_STDERR, child, line = %line);
            }
        }))
    }

    /// Starts killing the child and everything it started (its Job Object, or its process
    /// group); [`Self::wait`] reaps it.
    pub fn start_kill(&mut self) -> io::Result<()> {
        self.inner.start_kill()
    }

    /// Waits for the child (and, on Windows, every process in its job) to exit.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.inner.wait().await?;
        tracing::info!(name: names::CHILD_EXITED, child = self.name, code = status.code());
        Ok(status)
    }

    /// The exit status, if it has exited.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }
}

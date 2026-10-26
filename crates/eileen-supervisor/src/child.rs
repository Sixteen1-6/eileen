//! Supervised children: every child of the running product starts here (section 2.1 rule 1).
//!
//! On Windows a child is placed in a kill-on-close Job Object with no console window
//! (process-wrap's `JobObject`, `CreationFlags(CREATE_NO_WINDOW)` and `KillOnDrop`), so killing
//! it kills everything it started, a crash of Eileen takes every child with it, and dropping
//! the [`Child`] kills the tree. Claude sessions also run at `BELOW_NORMAL_PRIORITY_CLASS`, so a
//! worker's build never starves the voice loop.
//!
//! On Linux (docs/LINUX.md section 5) a child leads a session of its own: it leads its own
//! process group and has no controlling terminal, so a prompt on `/dev/tty` fails at once
//! instead of stopping it. Its whole group is killed when the [`Child`] drops, on
//! [`Child::start_kill`], and when [`Child::wait`] or [`Child::try_wait`] sees it exit, each
//! time before it is reaped, so no reused pid is ever signalled. It is forked on the
//! supervisor's own thread, which lives as long as the process, with `SIGKILL` as its
//! parent-death signal, so a crash of Eileen kills every child. Their descendants are not
//! reached: one that outlives its child in a crash, or leaves the group (`setsid`, `setpgid`),
//! is for the cgroup scope of LINUX.md section 5, which this crate does not have yet. A Claude
//! session runs ten nice steps below the core, which its builds inherit. On other Unix systems
//! the child leads its own process group.
//!
//! Stdout carries the protocol; [`Child::forward_stderr`] turns stderr into `child.stderr`, and
//! on Linux [`Child::wait`] does so for a pipe nobody took, so a full pipe cannot hang the wait.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};

use eileen_trace::names;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _, BufReader};
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
    /// A Claude session (brain, spare, pooled or job worker, librarian): below normal on
    /// Windows, ten nice steps below the core on Linux; other Unix systems keep the core's.
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

/// How to start one child. Its `Debug` lists the environment's names, never their values,
/// which may carry a session token or a key the harness hands a worker.
#[derive(Clone, PartialEq, Eq)]
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

impl fmt::Debug for ChildSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env_names: Vec<&OsString> = self.env.iter().map(|(name, _)| name).collect();
        f.debug_struct("ChildSpec")
            .field("name", &self.name)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env_clear", &self.env_clear)
            .field("env_names", &env_names)
            .field("cwd", &self.cwd)
            .field("class", &self.class)
            .field("piped", &self.piped)
            .finish_non_exhaustive()
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
/// when the returned [`Child`] is dropped. On Linux, in a session of its own, forked on the
/// supervisor's own thread inside the caller's tokio runtime.
///
/// Stderr is always a pipe, and a child that fills it blocks until it is read: take it or
/// forward it before reading stdout to its end. On Linux [`Child::wait`] forwards a pipe that
/// nobody took; on Windows nothing reads it unless the caller does.
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
    #[cfg(not(target_os = "linux"))]
    let spawned = wrap.spawn();
    #[cfg(target_os = "linux")]
    let spawned = crate::linux::spawn(wrap);
    let inner = spawned.map_err(|source| SupervisorError::Spawn {
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

/// On Linux a Claude session runs this many nice steps below the core (at most at 19), the
/// counterpart of Windows' `BELOW_NORMAL_PRIORITY_CLASS`, so its builds yield to the voice loop.
#[cfg(target_os = "linux")]
pub(crate) const CLAUDE_SESSION_NICER_BY: i32 = 10;

#[cfg(target_os = "linux")]
fn wrap_platform(wrap: &mut CommandWrap, class: ChildClass) {
    let nicer_by = match class {
        ChildClass::ClaudeSession => CLAUDE_SESSION_NICER_BY,
        ChildClass::Sidecar | ChildClass::Tool => 0,
    };
    crate::ffi::own_session_killed_with_parent(wrap.command_mut().as_std_mut(), nicer_by);
}

#[cfg(all(unix, not(target_os = "linux")))]
fn wrap_platform(wrap: &mut CommandWrap, _class: ChildClass) {
    wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
}

/// Kills the whole group while the child is unreaped (tokio then kills and reaps the leader);
/// after a reap there is nothing left to signal.
#[cfg(target_os = "linux")]
impl Drop for Child {
    fn drop(&mut self) {
        let _ignored = self.inner.start_kill();
    }
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
    ///
    /// The line goes in the `pii.line` field: a Claude session's stderr can quote paths, file
    /// names and prompt text, and a sidecar's panic message can quote anything, so the log
    /// files keep only its length and keyed hash while the flight recorder and the console
    /// keep the text.
    ///
    /// Lines are read as bytes and decoded lossily, so a tool writing in the ANSI code page
    /// does not stop the forwarding (which would leave the child writing into a closed pipe);
    /// at most [`MAX_STDERR_LINE`] bytes of a line are kept, so a child that writes megabytes
    /// without a newline costs no more memory than one line. Only the end of the stream or a
    /// read error ends it.
    pub fn forward_stderr(&mut self) -> Option<JoinHandle<()>> {
        let stderr = self.take_stderr()?;
        let child = self.name;
        Some(eileen_rt::task::spawn("supervisor.stderr", async move {
            let mut reader = BufReader::new(stderr);
            let mut line = Vec::with_capacity(256);
            while matches!(
                read_line_capped(&mut reader, &mut line, MAX_STDERR_LINE).await,
                Ok(true)
            ) {
                let text = decode_line(&line);
                tracing::info!(name: names::CHILD_STDERR, child, "pii.line" = %text);
            }
        }))
    }

    /// Starts killing the child and everything it started (its Job Object, or its process
    /// group); [`Self::wait`] reaps it. On Linux, once the child is reaped it does nothing:
    /// its group was killed before the reap.
    pub fn start_kill(&mut self) -> io::Result<()> {
        self.inner.start_kill()
    }

    /// Waits for the child (and, on Windows, every process in its job) to exit. On Linux it
    /// first forwards stderr into tracing unless [`Self::take_stderr`] or
    /// [`Self::forward_stderr`] took it, so a chatty child cannot block on a full pipe, and it
    /// kills what is left of the child's process group once the child has exited, then reaps
    /// it.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        // A child that fills the stderr pipe blocks in write(2) and never exits, so on Linux a
        // pipe nobody took is forwarded from here; the task outlives this future.
        #[cfg(target_os = "linux")]
        let _ignored = self.forward_stderr();
        let status = self.inner.wait().await?;
        tracing::info!(name: names::CHILD_EXITED, child = self.name, code = status.code());
        Ok(status)
    }

    /// The exit status, if it has exited. On Linux the call that first sees the exit kills
    /// what is left of the group and has tokio reap the child, so [`Self::id`] is `None` from
    /// then on.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait()
    }
}

/// Reads one line into `line` (cleared first), keeping at most `cap` bytes of it and
/// discarding the rest up to the newline. False at the end of the stream with nothing read.
async fn read_line_capped<R>(reader: &mut R, line: &mut Vec<u8>, cap: usize) -> io::Result<bool>
where
    R: AsyncBufRead + Unpin,
{
    line.clear();
    let mut read_any = false;
    loop {
        let buffer = match reader.fill_buf().await {
            Ok(buffer) => buffer,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buffer.is_empty() {
            return Ok(read_any);
        }
        read_any = true;
        let (chunk, used, ended) = match buffer.iter().position(|b| *b == b'\n') {
            Some(at) => (buffer.get(..at).unwrap_or_default(), at + 1, true),
            None => (buffer, buffer.len(), false),
        };
        let room = cap.saturating_sub(line.len());
        line.extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
        reader.consume(used);
        if ended {
            return Ok(true);
        }
    }
}

/// A forwarded line as text: invalid UTF-8 replaced, a trailing `\r` dropped, and cut to
/// [`MAX_STDERR_LINE`] bytes at a character boundary.
fn decode_line(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes.strip_suffix(b"\r").unwrap_or(bytes)).into_owned();
    if text.len() > MAX_STDERR_LINE {
        let cut = (0..=MAX_STDERR_LINE).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        text.truncate(cut);
    }
    text
}

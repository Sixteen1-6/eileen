//! The Claude worker (sections 2.5, 3.3.8 and 6.5): one `claude -p` session in stream-json that
//! runs one job, without the warm pool, which comes later.
//!
//! - `start` writes the session's run folder (its working folder, the settings file and the MCP
//!   config of `session_files`), spawns the located executable through `eileen_supervisor` in
//!   the Claude class (a kill-on-close Job Object at below-normal priority) with exactly the
//!   session's environment, and waits, within `HarnessConfig::init_deadline`, for
//!   `system/init`. A session whose init does not list `eileen` as connected, or reports an
//!   auto-memory folder, is killed and never bound (section 2.5, D77). Every session spawns with
//!   vendor memory off, so a memory folder in init means the CLI did not honour the variable:
//!   the session is started once more, and refused if it reports one again.
//! - `bind` writes the first user line: the task, `Follow skill eileen-learned:<id>` with any
//!   inline skill text, and the facts block.
//! - `send` writes later user turns, the interrupt as a `control_request` when init offers it,
//!   and cancels by killing the Job Object and waiting for the job to empty.
//! - `next_event` gives the first init's `Init`, then each line's worker events, and `Exited`
//!   with the exit code once stdout closes, or after a cancel. The CLI writes `system/init`
//!   again at each user turn: those are checked for a memory folder and not passed on. A crash,
//!   or any exit before the turn's `TurnDone`, is how the core learns the job failed.

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::harness::{JobBinding, SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;
use eileen_supervisor::watch::with_deadline;
use eileen_supervisor::{Child, ChildClass, ChildSpec, spawn};
use eileen_trace::Redactor;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use super::{WorkerDriver, WorkerSession};
use crate::session::{EILEEN_SERVER, SessionFiles, session_files};
use crate::stream::control::{Message, Requests};
use crate::stream::{self, Init, Line, MAX_LINE_BYTES};
use crate::{DriverError, HarnessConfig};

/// How long one write to the session's stdin may take. A CLI that stops reading its stdin
/// would otherwise hold the driver once the pipe fills.
const WRITE_DEADLINE: Duration = Duration::from_secs(10);

/// How long the session's processes have to exit after its stdout closed, or after the kill.
const EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// How many times `start` spawns a session whose init reports a memory folder: once more with
/// memory off, then the start fails.
const MEMORY_STARTS: u32 = 2;

/// The name the supervisor logs the session under.
const CHILD_NAME: &str = "claude.worker";

/// The Claude worker driver: `claude -p` in stream-json, one session at a time.
#[derive(Debug)]
pub struct ClaudeWorker {
    cfg: HarnessConfig,
    redactor: Arc<Redactor>,
    session: Option<Session>,
    /// Events read and not yet given out: those of one stdout line, or the init, or the exit.
    /// A line's events are given out before the next line is read, so this holds at most one
    /// line's blocks and an exit.
    pending: VecDeque<WorkerEvent>,
}

impl ClaudeWorker {
    /// A driver with no session yet. Its `claude` is `cfg.claude_exe`, which the runtime fills
    /// from `locate`, and every tool input is redacted with a `Redactor` of its own until
    /// [`Self::with_redactor`] gives it the process's.
    #[must_use]
    pub fn new(cfg: HarnessConfig) -> Self {
        Self { cfg, redactor: Arc::new(Redactor::new()), session: None, pending: VecDeque::new() }
    }

    /// Uses `redactor`, the process's shared one, for every line this driver parses.
    #[must_use]
    pub fn with_redactor(mut self, redactor: Arc<Redactor>) -> Self {
        self.redactor = redactor;
        self
    }

    fn session_mut(&mut self) -> Result<&mut Session, DriverError> {
        self.session.as_mut().ok_or(DriverError::NoSession)
    }

    /// Kills the session, if there is one, and queues its `Exited`.
    async fn kill(&mut self) {
        if let Some(session) = self.session.take() {
            let code = session.process.kill().await;
            self.pending.push_back(WorkerEvent::Exited { code });
        }
    }
}

impl WorkerDriver for ClaudeWorker {
    fn route(&self) -> Route {
        Route::Claude
    }

    fn languages(&self) -> LanguageSupport {
        LanguageSupport::Any
    }

    async fn start(&mut self, spec: SessionSpec) -> Result<WorkerSession, DriverError> {
        if self.session.is_some() {
            return Err(DriverError::AlreadyStarted);
        }
        if !self.cfg.claude_exe.is_absolute() {
            return Err(DriverError::NoExecutable);
        }
        let files = session_files(&spec, &self.cfg, &self.cfg.host)?;
        write_files(&files).await?;
        let mut starts = 0;
        loop {
            starts += 1;
            let mut process = Process::spawn(&files, &self.cfg.claude_exe)?;
            let init = match process.wait_init(&self.redactor, self.cfg.init_deadline).await {
                Ok(init) => init,
                Err(error) => {
                    process.kill().await;
                    return Err(error);
                }
            };
            match check_init(&init) {
                Ok(()) => {
                    let started = WorkerSession {
                        session: spec.session,
                        vendor_session: init.session_id.clone(),
                        model: init.model.clone(),
                    };
                    let first = Line::Init(Box::new(init.clone()));
                    self.pending.extend(stream::worker_events(first, process.pid));
                    self.session = Some(Session {
                        process,
                        init,
                        requests: Requests::default(),
                        bound: false,
                    });
                    return Ok(started);
                }
                Err(DriverError::MemoryPathReported) if starts < MEMORY_STARTS => {
                    process.kill().await;
                }
                Err(error) => {
                    process.kill().await;
                    return Err(error);
                }
            }
        }
    }

    async fn bind(&mut self, binding: JobBinding) -> Result<(), DriverError> {
        let session = self.session_mut()?;
        if session.bound {
            return Err(DriverError::AlreadyBound);
        }
        session.process.write(&user_line(&first_turn(&binding))).await?;
        session.bound = true;
        Ok(())
    }

    async fn send(&mut self, cmd: WorkerCmd) -> Result<(), DriverError> {
        match cmd {
            WorkerCmd::Start(spec) => self.start(*spec).await.map(drop),
            WorkerCmd::Bind(binding) => self.bind(binding).await,
            WorkerCmd::Prompt { text } => {
                let session = self.session_mut()?;
                if !session.bound {
                    return Err(DriverError::NotBound);
                }
                session.process.write(&user_line(&text)).await
            }
            WorkerCmd::Interrupt => {
                let session = self.session_mut()?;
                let line = session.requests.interrupt(&session.init)?;
                session.process.write(&line).await
            }
            WorkerCmd::Cancel => {
                self.kill().await;
                Ok(())
            }
            WorkerCmd::Recycle => Err(DriverError::Unsupported { command: "recycle" }),
            WorkerCmd::RemoveWorktree => {
                Err(DriverError::Unsupported { command: "remove_worktree" })
            }
            WorkerCmd::RestoreSnapshot { .. } => {
                Err(DriverError::Unsupported { command: "restore_snapshot" })
            }
        }
    }

    async fn next_event(&mut self) -> Option<WorkerEvent> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Some(event);
            }
            let session = self.session.as_mut()?;
            let Some(line) = session.process.next_line(&self.redactor).await else {
                let ended = self.session.take()?;
                let code = ended.process.finish().await;
                self.pending.push_back(WorkerEvent::Exited { code });
                continue;
            };
            match line {
                // The CLI writes init again at each user turn; the first was given out at start.
                Line::Init(init) => {
                    if init.memory_path.is_some() {
                        self.kill().await;
                    } else {
                        session.init = *init;
                    }
                }
                Line::Control(Message::Response { request_id, error }) => {
                    // A reply to a request that is not open settles nothing.
                    let _settled: Result<_, _> = session.requests.settle(&request_id, error);
                }
                other => self.pending.extend(stream::worker_events(other, session.process.pid)),
            }
        }
    }
}

/// A started session.
#[derive(Debug)]
struct Session {
    process: Process,
    /// The newest `system/init`: its capabilities say whether an interrupt may be sent.
    init: Init,
    requests: Requests,
    /// Whether the job's first user line was written.
    bound: bool,
}

/// The CLI's process and its stdio.
#[derive(Debug)]
struct Process {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pid: u32,
    /// The line being read, kept so its buffer is reused.
    line: Vec<u8>,
}

impl Process {
    fn spawn(files: &SessionFiles, exe: &Path) -> Result<Self, DriverError> {
        let spec = ChildSpec {
            env_clear: true,
            cwd: Some(files.cwd.clone()),
            ..ChildSpec::new(CHILD_NAME, exe)
                .args(&files.argv)
                .envs(files.env.vars())
                .class(ChildClass::ClaudeSession)
        };
        let mut child = spawn(&spec).map_err(|error| match error {
            eileen_supervisor::SupervisorError::Spawn { source, .. } => {
                DriverError::Spawn { kind: source.kind() }
            }
        })?;
        let pid = child.id().unwrap_or_default();
        let (Some(stdin), Some(stdout)) = (child.take_stdin(), child.take_stdout()) else {
            return Err(DriverError::Spawn { kind: io::ErrorKind::BrokenPipe });
        };
        Ok(Self { child, stdin, stdout: BufReader::new(stdout), pid, line: Vec::new() })
    }

    /// Reads lines until `system/init`, within `deadline`. Nothing the CLI writes before it
    /// belongs to a job, so it is passed over.
    async fn wait_init(
        &mut self,
        redactor: &Redactor,
        deadline: Duration,
    ) -> Result<Init, DriverError> {
        let read = async {
            loop {
                match self.next_line(redactor).await {
                    Some(Line::Init(init)) => return Ok(*init),
                    Some(_) => {}
                    None => return Err(()),
                }
            }
        };
        match with_deadline("claude.init", deadline, read).await {
            Ok(Ok(init)) => Ok(init),
            Ok(Err(())) => {
                let code = self.exit_code().await;
                Err(DriverError::EndedBeforeInit { code })
            }
            Err(_) => Err(DriverError::InitDeadline { secs: deadline.as_secs() }),
        }
    }

    /// The next stdout line, parsed, or `None` once stdout closed or failed. Blank lines are
    /// passed over. A line longer than [`MAX_LINE_BYTES`] is read to its end and kept only to
    /// one byte past the cap, which the parser then takes as malformed, so a runaway line costs
    /// no more memory than the cap.
    async fn next_line(&mut self, redactor: &Redactor) -> Option<Line> {
        loop {
            self.line.clear();
            let mut ended = false;
            loop {
                let Ok(buf) = self.stdout.fill_buf().await else { return None };
                if buf.is_empty() {
                    ended = true;
                    break;
                }
                let newline = buf.iter().position(|&byte| byte == b'\n');
                let take = newline.map_or(buf.len(), |at| at + 1);
                let room = (MAX_LINE_BYTES + 1).saturating_sub(self.line.len());
                self.line.extend(buf.iter().take(take.min(room)));
                self.stdout.consume(take);
                if newline.is_some() {
                    break;
                }
            }
            if ended && self.line.is_empty() {
                return None;
            }
            let body = self.line.strip_suffix(b"\n").unwrap_or(&self.line);
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            if !body.trim_ascii().is_empty() {
                return Some(stream::parse(body, redactor));
            }
            if ended {
                return None;
            }
        }
    }

    /// Writes `line` and its line ending, within [`WRITE_DEADLINE`].
    async fn write(&mut self, line: &str) -> Result<(), DriverError> {
        let write = async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        };
        match with_deadline("claude.write", WRITE_DEADLINE, write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(DriverError::Write { kind: error.kind() }),
            Err(_) => Err(DriverError::WriteDeadline),
        }
    }

    /// The exit code once the process and its job have exited, waited for within
    /// [`EXIT_DEADLINE`]; past it, the job is killed and waited for once more.
    async fn exit_code(&mut self) -> Option<i32> {
        match with_deadline("claude.exit", EXIT_DEADLINE, self.child.wait()).await {
            Ok(Ok(status)) => status.code(),
            Ok(Err(_)) => None,
            Err(_) => self.kill_and_wait().await,
        }
    }

    /// Waits for the exit after stdout closed.
    async fn finish(mut self) -> Option<i32> {
        self.exit_code().await
    }

    /// Kills the Job Object (on Linux the process group) and waits for it to empty.
    async fn kill(mut self) -> Option<i32> {
        self.kill_and_wait().await
    }

    async fn kill_and_wait(&mut self) -> Option<i32> {
        // It may have exited already, which leaves nothing to kill.
        let _killed: io::Result<()> = self.child.start_kill();
        match with_deadline("claude.kill", EXIT_DEADLINE, self.child.wait()).await {
            Ok(Ok(status)) => status.code(),
            // Dropping the child kills its job again: the supervisor's last word.
            Ok(Err(_)) | Err(_) => None,
        }
    }
}

/// Whether a started session may be bound: `eileen` connected (section 2.5) and no auto-memory
/// folder reported (D77: every session runs with vendor memory off until the memory gate
/// passes).
fn check_init(init: &Init) -> Result<(), DriverError> {
    if !init.connected(EILEEN_SERVER) {
        return Err(DriverError::EileenNotConnected);
    }
    if init.memory_path.is_some() {
        return Err(DriverError::MemoryPathReported);
    }
    Ok(())
}

/// Writes the session's run folder: its working folder, the settings file and the MCP config.
async fn write_files(files: &SessionFiles) -> Result<(), DriverError> {
    let failed = |what: &'static str| {
        move |error: io::Error| DriverError::Files { what, kind: error.kind() }
    };
    tokio::fs::create_dir_all(&files.run_dir).await.map_err(failed("run folder"))?;
    tokio::fs::create_dir_all(&files.cwd).await.map_err(failed("working folder"))?;
    for (path, value, what) in [
        (&files.settings_path, &files.settings, "settings file"),
        (&files.mcp_path, &files.mcp_config, "MCP config"),
    ] {
        let text = serde_json::to_vec_pretty(value).map_err(|error| failed(what)(error.into()))?;
        tokio::fs::write(path, text).await.map_err(failed(what))?;
    }
    Ok(())
}

/// The job's first user turn (section 3.3.8): the task, then `Follow skill
/// eileen-learned:<id>` when a skill is set, with its text when the snapshot lacks it (section
/// 2.10), then the facts block.
fn first_turn(binding: &JobBinding) -> String {
    let mut text = binding.task.clone();
    if let Some(skill) = &binding.skill {
        text.push_str("\n\nFollow skill eileen-learned:");
        text.push_str(skill.id.as_str());
        if let Some(inline) = &binding.inline_skill {
            text.push_str("\n\n");
            text.push_str(inline);
        }
    }
    if !binding.facts.is_empty() {
        text.push_str("\n\nFacts:");
        for fact in &binding.facts {
            text.push_str("\n- ");
            text.push_str(fact);
        }
    }
    text
}

/// One stream-json user line, without its line ending, in the shape WP3's approval contract
/// test wrote to the CLI (`worker-command.json`'s `first_user_line`).
fn user_line(text: &str) -> String {
    let line: Value = json!({
        "type": "user",
        "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        "parent_tool_use_id": null,
        "session_id": "",
    });
    line.to_string()
}

#[cfg(test)]
mod tests;

//! The Claude worker (sections 2.5, 3.3.8 and 6.5): one `claude -p` session in stream-json that
//! runs one job, without the warm pool, which comes later.
//!
//! - `start` refuses a spec whose snapshot, worktree, build slot or memory limit it does not
//!   carry out yet, registers the session's hook and shim tokens with the redactor for the
//!   session's life, writes the session's run folder (its working folder, the settings file and
//!   the MCP config of `session_files`), and spawns the located executable through
//!   `eileen_supervisor` in the Claude class (a kill-on-close Job Object at below-normal
//!   priority) with exactly the session's environment. The CLI writes nothing on stdout,
//!   `system/init` included, until its first user line (CLI 2.1.316), so readiness comes from
//!   the stdio control protocol, which it answers before any user line: `initialize`, then
//!   `mcp_status` until `eileen` is `connected`, all within `HarnessConfig::init_deadline`. A
//!   session whose `eileen` is `failed` (or any status but `pending`) is killed and never bound
//!   (section 2.5).
//! - `bind` writes the first user line: the task, `Follow skill eileen-learned:<id>` with any
//!   inline skill text, and the facts block.
//! - `send` writes later user turns, the interrupt as a `control_request` once the first turn's
//!   init offers it, and cancels by killing the Job Object and waiting for the job to empty. A
//!   write that fails or passes its deadline may have left part of a line on the CLI's stdin,
//!   so it ends the session.
//! - `next_event` gives the first turn's `Init`, then each line's worker events, and `Exited`
//!   with the exit code once stdout closes, or after a cancel. Every init, the first included,
//!   is checked: one that does not list `eileen` as connected, or reports an auto-memory folder
//!   (D77: every session runs with memory off), kills the session at that line, which the CLI
//!   writes before the model's first message, so no tool runs. Later inits are not passed on. A
//!   crash, or any exit before the turn's `TurnDone`, is how the core learns the job failed.
//! - Control requests still open when a turn ends or a new init arrives are abandoned, so a CLI
//!   that drops a reply never fills `stream::control::MAX_OPEN_REQUESTS`.
//! - `next_event` is cancel-safe: a partly read line stays buffered, and a session being ended
//!   stays in `ending` until its exit is seen.

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::harness::{JobBinding, SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::WorkerSessionId;
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;
use eileen_supervisor::watch::with_deadline;
use eileen_supervisor::{Child, ChildClass, ChildSpec, spawn};
use eileen_trace::Redactor;
use eileen_trace::names;
use eileen_trace::redact::LiteralGuard;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use super::{WorkerDriver, WorkerSession};
use crate::session::{EILEEN_SERVER, SessionFiles, session_files};
use crate::stream::control::{Message, Reply, Requests, SendError};
use crate::stream::{self, Init, Line, MAX_LINE_BYTES};
use crate::{DriverError, HarnessConfig};

/// How long one write to the session's stdin may take. A CLI that stops reading its stdin
/// would otherwise hold the driver once the pipe fills.
const WRITE_DEADLINE: Duration = Duration::from_secs(10);

/// How long the session's processes have to exit after its stdout closed, or after the kill.
const EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// How long `start` waits between two `mcp_status` requests while `eileen` is still pending.
const STATUS_POLL: Duration = Duration::from_millis(250);

/// The name the supervisor logs the session under.
const CHILD_NAME: &str = "claude.worker";

/// The Claude worker driver: `claude -p` in stream-json, one session at a time.
#[derive(Debug)]
pub struct ClaudeWorker {
    cfg: HarnessConfig,
    redactor: Arc<Redactor>,
    session: Option<Session>,
    /// A session being ended, kept here while its exit is awaited, so a dropped `next_event` or
    /// `send` loses neither the process nor its `Exited`: the next call finishes it.
    ending: Option<Ending>,
    /// Events read and not yet given out: those of one stdout line, or the init, or the exit.
    /// A line's events are given out before the next line is read, so this holds at most one
    /// line's blocks and an exit.
    pending: VecDeque<WorkerEvent>,
}

impl ClaudeWorker {
    /// A driver with no session yet. Its `claude` is `cfg.claude_exe`, which the runtime fills
    /// from `locate`, and `redactor`, the process's shared one, redacts every line it parses;
    /// each session's tokens are registered with it while the session lives.
    #[must_use]
    pub fn new(cfg: HarnessConfig, redactor: Arc<Redactor>) -> Self {
        Self { cfg, redactor, session: None, ending: None, pending: VecDeque::new() }
    }

    fn session_mut(&mut self) -> Result<&mut Session, DriverError> {
        self.session.as_mut().ok_or(DriverError::NoSession)
    }

    /// Kills the session, if there is one, and queues its `Exited`.
    async fn kill(&mut self) {
        self.end(true).await;
    }

    /// Moves the session to `ending`, killing it when `kill` is set or waiting for its exit
    /// otherwise, and queues its `Exited`.
    async fn end(&mut self, kill: bool) {
        if let Some(session) = self.session.take() {
            self.ending = Some(Ending { id: session.id, process: session.process, kill });
        }
        self.finish_ending().await;
    }

    /// Finishes the session in `ending`, if there is one: its exit is awaited, then `Exited` is
    /// queued. Cancel-safe: the session stays in `ending` until the exit is seen.
    async fn finish_ending(&mut self) {
        let Some(ending) = self.ending.as_mut() else { return };
        let exit = if ending.kill {
            ending.process.kill_and_wait().await
        } else {
            ending.process.exit_code().await
        };
        let code = exit.code(ending.id);
        self.ending = None;
        self.pending.push_back(WorkerEvent::Exited { code });
    }

    /// Writes one line to the session. A write that fails or passes its deadline may have left
    /// part of the line on the CLI's stdin, where the next line would join it, so it ends the
    /// session; a write after one that was dropped partway does the same.
    async fn write(&mut self, line: &str) -> Result<(), DriverError> {
        let session = self.session_mut()?;
        let id = session.id;
        let written = if session.process.torn {
            Err(DriverError::Write { kind: io::ErrorKind::Interrupted })
        } else {
            session.process.write(line).await
        };
        if let Err(error) = written {
            tracing::warn!(name: names::WORKER_FAULT, ws_id = %id, fault = %error);
            self.kill().await;
            return Err(error);
        }
        Ok(())
    }

    /// Kills the session for a policy reason (section 2.5, D77) and logs why.
    async fn refuse(&mut self, reason: DriverError) {
        if let Some(session) = &self.session {
            tracing::warn!(name: names::WORKER_REFUSED, ws_id = %session.id, reason = %reason);
        }
        // FIXME(#161): queue `WorkerEvent::Refused` with the reason before the `Exited`, once
        // the amendment's contract change lands, so the core fails the job with it.
        self.kill().await;
    }

    /// Acts on one parsed stdout line of the session.
    async fn on_line(&mut self, line: Line) {
        let Some(session) = self.session.as_mut() else { return };
        match line {
            // The CLI writes init at each user turn: every one is checked, the first given out.
            Line::Init(init) => {
                abandon_open(session);
                if let Err(reason) = check_init(&init) {
                    self.refuse(reason).await;
                    return;
                }
                if session.init.is_none() {
                    let event = Line::Init(init.clone());
                    self.pending.extend(stream::worker_events(event, session.process.pid));
                }
                session.init = Some(*init);
            }
            Line::Control(Message::Response { request_id, error }) => {
                let fault = match session.requests.settle(&request_id, error) {
                    Ok(Reply { error: None, .. }) => None,
                    Ok(Reply { error: Some(_), .. }) => {
                        Some("an interrupt was answered with an error")
                    }
                    Err(_) => Some("a control reply to a request that is not open"),
                };
                if let Some(fault) = fault {
                    tracing::warn!(name: names::WORKER_FAULT, ws_id = %session.id, fault);
                }
            }
            // v0.1 answers no request the CLI makes.
            Line::Control(
                Message::Interrupt { .. }
                | Message::CanUseTool(_)
                | Message::Request { .. }
                | Message::Cancel { .. },
            ) => {}
            other @ (Line::Assistant(_)
            | Line::ToolResults(_)
            | Line::TextDelta(_)
            | Line::Partial
            | Line::RateLimit(_)
            | Line::HookStarted(_)
            | Line::HookResponse(..)
            | Line::PermissionDenied(_)
            | Line::Result(_)
            | Line::Unknown { .. }
            | Line::Malformed { .. }) => {
                if matches!(other, Line::Result(_)) {
                    abandon_open(session);
                }
                self.pending.extend(stream::worker_events(other, session.process.pid));
            }
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
        unsupported(&spec)?;
        if !self.cfg.claude_exe.is_absolute() {
            return Err(DriverError::NoExecutable);
        }
        // An earlier session's exit and its unread events belong to it, not to this one.
        self.finish_ending().await;
        self.pending.clear();
        let files = session_files(&spec, &self.cfg, &self.cfg.host)?;
        let secrets = register_tokens(&self.redactor, &spec)?;
        write_files(&files).await?;
        let mut process = Process::spawn(&files, &self.cfg.claude_exe)?;
        if let Err(error) = process.wait_ready(self.cfg.init_deadline).await {
            if error == DriverError::EileenNotConnected {
                tracing::warn!(name: names::WORKER_REFUSED, ws_id = %spec.session, reason = %error);
            }
            process.kill_and_wait().await.code(spec.session);
            return Err(error);
        }
        let pid = process.pid;
        self.session = Some(Session {
            id: spec.session,
            process,
            init: None,
            requests: Requests::default(),
            bound: false,
            _secrets: secrets,
        });
        // FIXME(#161): queue `WorkerEvent::Ready { pid }` once the amendment's contract change
        // lands; the core binds on it, and until then it binds on `Init`, which the CLI writes
        // only after the first user line.
        Ok(WorkerSession { session: spec.session, pid })
    }

    async fn bind(&mut self, binding: JobBinding) -> Result<(), DriverError> {
        if self.session_mut()?.bound {
            return Err(DriverError::AlreadyBound);
        }
        self.write(&user_line(&first_turn(&binding))).await?;
        self.session_mut()?.bound = true;
        Ok(())
    }

    async fn send(&mut self, cmd: WorkerCmd) -> Result<(), DriverError> {
        match cmd {
            WorkerCmd::Start(spec) => self.start(*spec).await.map(drop),
            WorkerCmd::Bind(binding) => self.bind(binding).await,
            WorkerCmd::Prompt { text } => {
                if !self.session_mut()?.bound {
                    return Err(DriverError::NotBound);
                }
                self.write(&user_line(&text)).await
            }
            WorkerCmd::Interrupt => {
                let session = self.session_mut()?;
                // Before the first turn's init nothing says the CLI takes an interrupt.
                let Some(init) = &session.init else {
                    return Err(SendError::NotOffered.into());
                };
                let line = session.requests.interrupt(init)?;
                self.write(&line).await
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
            if self.ending.is_some() {
                self.finish_ending().await;
                continue;
            }
            let session = self.session.as_mut()?;
            let line = match session.process.stdout.next().await {
                Some(body) => stream::parse(body, &self.redactor),
                None => {
                    self.end(false).await;
                    continue;
                }
            };
            self.on_line(line).await;
        }
    }
}

/// A started session.
#[derive(Debug)]
struct Session {
    /// Eileen's id for it, for the logs.
    id: WorkerSessionId,
    process: Process,
    /// The newest `system/init`, `None` before the first turn's: its capabilities say whether
    /// an interrupt may be sent.
    init: Option<Init>,
    requests: Requests,
    /// Whether the job's first user line was written.
    bound: bool,
    /// The session's tokens, registered with the redactor while it lives.
    _secrets: Vec<LiteralGuard>,
}

/// A session being ended.
#[derive(Debug)]
struct Ending {
    id: WorkerSessionId,
    process: Process,
    /// Whether it is killed, or was seen to close its stdout and is waited for.
    kill: bool,
}

/// How a wait for the session's exit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    /// It exited, with this code when it had one.
    Exited(Option<i32>),
    /// It was not seen to exit within [`EXIT_DEADLINE`] after the kill; dropping the child
    /// kills its job once more.
    Missed,
}

impl Exit {
    /// The exit code for `Exited`, logging a missed exit.
    fn code(self, id: WorkerSessionId) -> Option<i32> {
        match self {
            Self::Exited(code) => code,
            Self::Missed => {
                let fault = "the session was not seen to exit after its kill";
                tracing::warn!(name: names::WORKER_FAULT, ws_id = %id, fault);
                None
            }
        }
    }

    /// The code, with a missed exit as none, unlogged: for a session that never started.
    const fn code_or_none(self) -> Option<i32> {
        match self {
            Self::Exited(code) => code,
            Self::Missed => None,
        }
    }
}

/// The CLI's process and its stdio.
#[derive(Debug)]
struct Process {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    pid: u32,
    /// Whether a write was dropped partway, which may have left part of a line on stdin.
    torn: bool,
    /// The number of the last readiness request written.
    asked: u32,
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
        Ok(Self {
            child,
            stdin,
            stdout: Lines::new(BufReader::new(stdout)),
            pid,
            torn: false,
            asked: 0,
        })
    }

    /// Waits, within `deadline`, until the session is ready for its job: `initialize`
    /// answered, then `mcp_status` asked until `eileen` is connected.
    async fn wait_ready(&mut self, deadline: Duration) -> Result<(), DriverError> {
        let ready = async {
            self.ask("initialize").await?;
            loop {
                match eileen_status(&self.ask("mcp_status").await?) {
                    Status::Connected => return Ok(()),
                    Status::Pending => tokio::time::sleep(STATUS_POLL).await,
                    Status::Refused => return Err(Asked::Failed(DriverError::EileenNotConnected)),
                }
            }
        };
        match with_deadline("claude.ready", deadline, ready).await {
            Ok(Err(Asked::Ended)) => {
                let code = self.exit_code().await.code_or_none();
                Err(DriverError::EndedBeforeReady { code })
            }
            Ok(Err(Asked::Failed(error))) => Err(error),
            Ok(Ok(())) => Ok(()),
            Err(_) => Err(DriverError::ReadyDeadline { secs: deadline.as_secs() }),
        }
    }

    /// Writes the control request `subtype` and reads stdout to its reply, whose `response`
    /// it returns. Nothing else the CLI writes before the job belongs to one, so it is passed
    /// over.
    async fn ask(&mut self, subtype: &'static str) -> Result<Value, Asked> {
        self.asked = self.asked.saturating_add(1);
        let request_id = format!("eileen_ready_{}", self.asked);
        let line = json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": subtype },
        });
        self.write(&line.to_string()).await.map_err(Asked::Failed)?;
        loop {
            let body = self.stdout.next().await.ok_or(Asked::Ended)?;
            match reply_to(body, &request_id) {
                Some(Ok(response)) => return Ok(response),
                Some(Err(())) => {
                    return Err(Asked::Failed(DriverError::ControlRefused { request: subtype }));
                }
                None => {}
            }
        }
    }

    /// Writes `line` and its line ending, within [`WRITE_DEADLINE`]. `torn` is set while the
    /// write runs, so a write dropped partway is known to the next one.
    async fn write(&mut self, line: &str) -> Result<(), DriverError> {
        self.torn = true;
        let write = async {
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.write_all(b"\n").await?;
            self.stdin.flush().await
        };
        let written = match with_deadline("claude.write", WRITE_DEADLINE, write).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(DriverError::Write { kind: error.kind() }),
            Err(_) => Err(DriverError::WriteDeadline),
        };
        self.torn = written.is_err();
        written
    }

    /// The exit once the process and its job have exited, waited for within
    /// [`EXIT_DEADLINE`]; past it, the job is killed and waited for once more.
    async fn exit_code(&mut self) -> Exit {
        match with_deadline("claude.exit", EXIT_DEADLINE, self.child.wait()).await {
            Ok(Ok(status)) => Exit::Exited(status.code()),
            Ok(Err(_)) => Exit::Exited(None),
            Err(_) => self.kill_and_wait().await,
        }
    }

    /// Kills the Job Object (on Linux the process group) and waits for it to empty.
    async fn kill_and_wait(&mut self) -> Exit {
        // It may have exited already, which leaves nothing to kill.
        let _killed: io::Result<()> = self.child.start_kill();
        match with_deadline("claude.kill", EXIT_DEADLINE, self.child.wait()).await {
            Ok(Ok(status)) => Exit::Exited(status.code()),
            Ok(Err(_)) => Exit::Exited(None),
            Err(_) => Exit::Missed,
        }
    }
}

/// Why a readiness request got no reply.
#[derive(Debug)]
enum Asked {
    /// Stdout closed first.
    Ended,
    /// The write failed, or the CLI answered with an error.
    Failed(DriverError),
}

/// The session's stdout, line by line.
#[derive(Debug)]
struct Lines<R> {
    reader: R,
    /// The line being read. It keeps what was read across calls until the line is whole, so a
    /// dropped `next` loses nothing.
    line: Vec<u8>,
    /// Whether `line` holds a whole line already given out, to be cleared by the next call.
    done: bool,
}

impl<R: AsyncBufRead + Unpin> Lines<R> {
    const fn new(reader: R) -> Self {
        Self { reader, line: Vec::new(), done: false }
    }

    /// The next line that is not blank, without its line ending, or `None` once stdout closed
    /// or failed. A line longer than [`MAX_LINE_BYTES`] is read to its end and kept only to one
    /// byte past the cap, which the parser then takes as malformed, so a runaway line costs no
    /// more memory than the cap. Cancel-safe: bytes are consumed from the reader only once they
    /// are in `line`, and `line` is kept until it is whole.
    async fn next(&mut self) -> Option<&[u8]> {
        let end = loop {
            if self.done {
                self.line.clear();
                self.done = false;
            }
            let ended = loop {
                let Ok(buf) = self.reader.fill_buf().await else { return None };
                if buf.is_empty() {
                    break true;
                }
                let newline = buf.iter().position(|&byte| byte == b'\n');
                let take = newline.map_or(buf.len(), |at| at + 1);
                let room = (MAX_LINE_BYTES + 1).saturating_sub(self.line.len());
                self.line.extend(buf.iter().take(take.min(room)));
                self.reader.consume(take);
                if newline.is_some() {
                    break false;
                }
            };
            self.done = true;
            if ended && self.line.is_empty() {
                return None;
            }
            let body = self.line.strip_suffix(b"\n").unwrap_or(&self.line);
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            if !body.trim_ascii().is_empty() {
                break body.len();
            }
            if ended {
                return None;
            }
        };
        self.line.get(..end)
    }
}

/// Where `eileen` stands in an `mcp_status` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Connected,
    /// Still connecting, or not listed yet.
    Pending,
    /// Any other status: `failed`, `needs-auth` and the like.
    Refused,
}

/// Where `eileen` stands in the `response` of an `mcp_status` reply, `{"mcpServers": [{"name",
/// "status"}, ...]}`. The servers' shape is the one `system/init` lists them in.
fn eileen_status(response: &Value) -> Status {
    let status = response
        .get("mcpServers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|server| server.get("name").and_then(Value::as_str) == Some(EILEEN_SERVER))
        .and_then(|server| server.get("status"))
        .and_then(Value::as_str);
    match status {
        Some("connected") => Status::Connected,
        None | Some("pending") => Status::Pending,
        Some(_) => Status::Refused,
    }
}

/// The reply to the request `request_id` when `line` is one: its `response` (null when it has
/// none) for a success, `Err` for an error.
fn reply_to(line: &[u8], request_id: &str) -> Option<Result<Value, ()>> {
    let value = serde_json::from_slice::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("control_response") {
        return None;
    }
    let response = value.get("response")?;
    if response.get("request_id").and_then(Value::as_str) != Some(request_id) {
        return None;
    }
    match response.get("subtype").and_then(Value::as_str) {
        Some("success") => Some(Ok(response.get("response").cloned().unwrap_or(Value::Null))),
        _ => Some(Err(())),
    }
}

/// Abandons every control request still open on `session`: at a turn's end or a new init its
/// reply is no longer coming.
fn abandon_open(session: &mut Session) {
    if session.requests.abandon_all() > 0 {
        let fault = "control requests were left unanswered at the end of a turn";
        tracing::warn!(name: names::WORKER_FAULT, ws_id = %session.id, fault);
    }
}

/// Refuses what the driver does not carry out yet, rather than starting a session without it.
fn unsupported(spec: &SessionSpec) -> Result<(), DriverError> {
    let asked = [
        (spec.snapshot.is_some(), "a plugin snapshot check"),
        (spec.snapshot_ref.is_some(), "a snapshot restore"),
        (spec.worktree.is_some(), "a worktree"),
        (spec.build_slot.is_some(), "a build slot"),
        (spec.memory_limit_mb.is_some(), "a job memory limit"),
    ];
    match asked.into_iter().find(|(set, _)| *set) {
        Some((_, command)) => Err(DriverError::Unsupported { command }),
        None => Ok(()),
    }
}

/// Registers the session's hook and shim tokens with the redactor, so a tool input that quotes
/// one is redacted. An empty token is no secret and is passed over.
fn register_tokens(
    redactor: &Arc<Redactor>,
    spec: &SessionSpec,
) -> Result<Vec<LiteralGuard>, DriverError> {
    [&spec.hook_endpoint.token, &spec.shim.token]
        .into_iter()
        .map(|token| token.expose())
        .filter(|token| !token.is_empty())
        .map(|token| redactor.add_literal(token).map_err(|_| DriverError::SecretNotRegistered))
        .collect()
}

/// Whether a session may run on: `eileen` connected (section 2.5) and no auto-memory folder
/// reported (D77: every session runs with vendor memory off until the memory gate passes).
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

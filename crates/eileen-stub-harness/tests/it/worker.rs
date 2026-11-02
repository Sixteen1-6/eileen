//! `ClaudeWorker` running jobs against this stub in place of the claude CLI (BUILD.md section
//! 6.5, acceptance 2 without the pool and the approval path, acceptance 4's denylist and
//! pass-through part, and acceptance 8's memory checks as the amendment of 2026-11-01 moved
//! them). The tests live here, not in `eileen-harness`, because Cargo names the stub's binary
//! (`CARGO_BIN_EXE_eileen-stub-harness`) only to this package's tests.
//!
//! Each test gives the worker the stub as its executable, a script through `EILEEN_STUB_SCRIPT`
//! in the user's environment (the driver passes it on, as it is not denylisted) and a data
//! folder of its own, removed when the test ends. The scripts follow the real CLI's order: no
//! `system/init` before the first user line, so `start` returning shows the session was found
//! ready through `initialize` and `mcp_status` alone. Nothing here sleeps: the stub holds at
//! `await_user` until the driver writes, and `HANG` only guards against a hang.

use std::collections::BTreeMap;
use std::fs;
use std::future::Future;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_contracts::harness::{
    EnvPolicy, HookEndpoint, JobBinding, SessionSpec, SessionToken, ShimEndpoint, WorkerCmd,
    WorkerEvent,
};
use eileen_contracts::ids::{JobId, TraceCtx, WorkerSessionId};
use eileen_contracts::jobs::{JobKind, Route, Toolset};
use eileen_harness::session::{HostEnv, MCP_FILE, SETTINGS_FILE, is_denylisted};
use eileen_harness::{ClaudeWorker, DriverError, HarnessConfig, WorkerDriver as _};
use eileen_stub_harness::{ENV_FILE_ENV, SCRIPT_ENV};
use eileen_trace::test_support::{Captured, capture};
use eileen_trace::{Redactor, names};
use serde_json::Value;
use tokio::time::timeout;

/// How long a call may take before the test calls the driver hung: a guard, far above anything
/// a working stub needs, never a measurement.
const HANG: Duration = Duration::from_secs(20);

/// The vendor session id every script's `system/init` carries.
const VENDOR_SESSION: &str = "a3b57c79-73b3-485d-8511-c5ae9b1cdac5";

/// The shim token the test session carries, which `driver-token.ndjson` quotes.
const SHIM_TOKEN: &str = "shim-token-for-the-stub";

/// The `PATH` the user's environment holds, which a worker gets as it is.
const USER_PATH: &str = if cfg!(windows) { r"C:\eileen-test\bin" } else { "/eileen-test/bin" };

/// Variables the user's environment holds that the worker denylist removes (section 2.12).
const DENIED: [(&str, &str); 6] = [
    ("ANTHROPIC_API_KEY", "not-a-real-key"),
    ("CLAUDE_CONFIG_DIR", "/nowhere/claude-config"),
    ("GH_TOKEN", "not-a-real-token"),
    ("AWS_PROFILE", "not-a-real-profile"),
    ("STRIPE_API_KEY", "not-a-real-key"),
    ("SERVICE_SECRET", "not-a-real-secret"),
];

/// The variables Eileen sets in every session after the policy has run, whatever the user's
/// environment held (`eileen_harness::session`).
const SESSION_VARS: [&str; 8] = [
    "ENABLE_CLAUDEAI_MCP_SERVERS",
    "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
    "CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING",
    "EILEEN_SESSION_ID",
    "EILEEN_HOOK_TOKEN",
    "EILEEN_SHIM_TOKEN",
    "MCP_TOOL_TIMEOUT",
    "TRACEPARENT",
];

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/harness-scripts").join(name)
}

/// A fresh folder of this test's own under the system's temporary folder, removed on drop, so
/// no run leaves its settings file or the stub's environment record behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-worker-{}-{n}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a failed removal leaves only a folder in the temporary folder.
        let _removed: std::io::Result<()> = fs::remove_dir_all(&self.0);
    }
}

/// A worker whose `claude` is the stub playing `name`, with the denylisted variables, a
/// pass-through proxy, a `PATH` and the user's own attempt to turn vendor memory on in its
/// environment. The stub appends its environment to `<dir>/env.ndjson`.
fn worker(dir: &Path, name: &str) -> ClaudeWorker {
    let mut vars: Vec<(String, String)> =
        DENIED.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
    vars.extend([
        ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:3128".to_owned()),
        ("PATH".to_owned(), USER_PATH.to_owned()),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_owned(), "0".to_owned()),
        (SCRIPT_ENV.to_owned(), script(name).display().to_string()),
        (ENV_FILE_ENV.to_owned(), dir.join("env.ndjson").display().to_string()),
    ]);
    // Windows loads system libraries from SystemRoot; the stub needs nothing else.
    if let Some(root) = std::env::var_os("SystemRoot") {
        vars.push(("SystemRoot".to_owned(), root.to_string_lossy().into_owned()));
    }
    let cfg = HarnessConfig {
        data_dir: dir.join("data"),
        shim_exe: dir.join("eileen-shim"),
        claude_exe: PathBuf::from(env!("CARGO_BIN_EXE_eileen-stub-harness")),
        host: HostEnv { vars, names_ignore_case: cfg!(windows), ..HostEnv::default() },
        init_deadline: HANG,
        ..HarnessConfig::default()
    };
    ClaudeWorker::new(cfg, Arc::new(Redactor::new()))
}

fn session_id() -> WorkerSessionId {
    WorkerSessionId::from_parts(1_700_000_000_000, 7)
}

/// A coding job's session, as the core builds one, with its endpoints filled in.
fn spec() -> SessionSpec {
    let session = session_id();
    SessionSpec {
        session,
        pool: None,
        kind: JobKind::Coding,
        route: Route::Claude,
        model: "claude-haiku-5-5".to_owned(),
        effort: None,
        cwd: format!("run/{session}/cwd"),
        worktree: None,
        snapshot_ref: None,
        append_system_prompt: "Start progress lines with \"> \".".to_owned(),
        toolsets: vec![Toolset::Worker],
        allowed_tools: vec!["Bash".to_owned(), "mcp__eileen__*".to_owned()],
        extra_mcp_servers: Vec::new(),
        builtin_tools: vec!["Bash".to_owned()],
        disallowed_tools: Vec::new(),
        plugin_dirs: Vec::new(),
        snapshot: None,
        resume_session: None,
        chrome: false,
        env: EnvPolicy::WorkerDenylist,
        build_slot: None,
        memory_limit_mb: None,
        trace: TraceCtx::default(),
        hook_endpoint: HookEndpoint {
            port: 47_001,
            path: "hooks".to_owned(),
            token: SessionToken::new("hook-token-for-the-stub".to_owned()),
        },
        shim: ShimEndpoint {
            pipe: "eileen-test-pipe".to_owned(),
            token: SessionToken::new(SHIM_TOKEN.to_owned()),
        },
    }
}

fn binding() -> JobBinding {
    JobBinding {
        job: JobId::from_parts(1_700_000_000_000, 9),
        kind: JobKind::Coding,
        task: "Create hello.txt containing hi in the scratch repository".to_owned(),
        skill: None,
        inline_skill: None,
        facts: Vec::new(),
        trace: TraceCtx::default(),
    }
}

/// Starts a session of `worker` and binds the job to it. The stub writes nothing before the
/// first user line, so `start` answers on `initialize` and `mcp_status` alone.
async fn start_and_bind(worker: &mut ClaudeWorker) {
    let session = timeout(HANG, worker.start(spec())).await.unwrap().unwrap();
    assert_eq!(session.session, session_id(), "Eileen's id for the session");
    assert_ne!(session.pid, 0, "the CLI's process id");
    timeout(HANG, worker.bind(binding())).await.unwrap().unwrap();
}

/// A worker on `name` with its job bound.
async fn started(dir: &Path, name: &str) -> ClaudeWorker {
    let mut worker = worker(dir, name);
    start_and_bind(&mut worker).await;
    worker
}

async fn next(worker: &mut ClaudeWorker) -> Option<WorkerEvent> {
    timeout(HANG, worker.next_event()).await.expect("the worker hung")
}

/// Events up to and including the first that `last` matches.
async fn events_until(
    worker: &mut ClaudeWorker,
    last: impl Fn(&WorkerEvent) -> bool,
) -> Vec<WorkerEvent> {
    let mut events = Vec::new();
    while let Some(event) = next(worker).await {
        let done = last(&event);
        events.push(event);
        if done {
            return events;
        }
    }
    panic!("the session ended before the expected event: {events:?}");
}

/// Every event left, to the end of the session.
async fn rest(worker: &mut ClaudeWorker) -> Vec<WorkerEvent> {
    let mut events = Vec::new();
    while let Some(event) = next(worker).await {
        events.push(event);
    }
    events
}

/// Each event written short: its kind and the fields a script sets.
fn summary(event: &WorkerEvent) -> String {
    match event {
        WorkerEvent::Init { session_id, mcp_servers, .. } => {
            format!("init {session_id} {}", mcp_servers.join(","))
        }
        WorkerEvent::Narration { line } => format!("narration {}", line.get()),
        WorkerEvent::Text { delta } => format!("text {}", delta.get()),
        WorkerEvent::ToolUse { name, .. } => format!("tool_use {name}"),
        WorkerEvent::ToolResult { ok, .. } => format!("tool_result ok={ok}"),
        WorkerEvent::SkillInvoked { name, args } => format!("skill {name} {}", args.get()),
        WorkerEvent::TurnDone { is_error, summary, .. } => {
            format!("turn_done error={is_error} {}", summary.get())
        }
        WorkerEvent::Exited { code } => format!("exited {code:?}"),
        other @ (WorkerEvent::MemoryLimit { .. }
        | WorkerEvent::RateLimit(_)
        | WorkerEvent::AuthFailed { .. }
        | WorkerEvent::Malformed { .. }) => format!("{other:?}"),
    }
}

fn summaries(events: &[WorkerEvent]) -> Vec<String> {
    events.iter().map(summary).collect()
}

/// Cancels the session and checks it ends: any events already read come first, then one
/// `Exited`, then nothing. Gives the exit code.
async fn cancel(worker: &mut ClaudeWorker) -> Option<i32> {
    timeout(HANG, worker.send(WorkerCmd::Cancel)).await.unwrap().unwrap();
    let rest = rest(worker).await;
    let exits = rest.iter().filter(|e| matches!(e, WorkerEvent::Exited { .. })).count();
    assert_eq!(exits, 1, "one exit: {rest:?}");
    let Some(WorkerEvent::Exited { code }) = rest.last() else {
        panic!("the exit is last: {rest:?}");
    };
    *code
}

/// The environments the stub recorded, one per process it ran as.
fn recorded_envs(dir: &Path) -> Vec<BTreeMap<String, String>> {
    let text = fs::read_to_string(dir.join("env.ndjson")).unwrap_or_default();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// Runs `test` on a runtime of its own with tracing captured, for the tests that check a log.
fn with_logs<F: Future>(test: F) -> (F::Output, Captured) {
    capture(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(test)
    })
}

/// The `reason` of each `worker.refused` event captured.
fn refusals(captured: &Captured) -> Vec<String> {
    captured
        .events_named(names::WORKER_REFUSED)
        .iter()
        .map(|event| event.fields.get("reason").and_then(Value::as_str).unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn a_job_runs_to_completion_with_its_events_in_order() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-job.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::TurnDone { .. })).await;
    assert_eq!(
        summaries(&events),
        [
            format!("init {VENDOR_SESSION} eileen:connected"),
            "narration Creating hello.txt in the scratch repository".to_owned(),
            "tool_use Bash".to_owned(),
            "tool_result ok=true".to_owned(),
            "text Created hello.txt containing hi.".to_owned(),
            "turn_done error=false Created hello.txt containing hi.".to_owned(),
        ],
        "the job's events, the first turn's init first"
    );
    cancel(&mut worker).await;
}

#[tokio::test]
async fn narration_lines_become_narration_and_other_text_stays_text() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-job.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::TurnDone { .. })).await;
    let narration: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if let WorkerEvent::Narration { line } = e { Some(line.get().as_str()) } else { None }
        })
        .collect();
    assert_eq!(narration, ["Creating hello.txt in the scratch repository"], "without the prefix");
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, WorkerEvent::Text { delta } if delta.get().starts_with("> "))),
        "no text event keeps a narration line: {events:?}"
    );
    cancel(&mut worker).await;
}

#[tokio::test]
async fn a_skill_call_becomes_skill_invoked() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-skill.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::TurnDone { .. })).await;
    let skills: Vec<String> = events
        .iter()
        .filter(|e| matches!(e, WorkerEvent::SkillInvoked { .. }))
        .map(summary)
        .collect();
    assert_eq!(skills, ["skill eileen-learned:resolve-goto-offset 15:30"], "one skill call");
    cancel(&mut worker).await;
}

#[tokio::test]
async fn a_crash_ends_the_job_with_its_exit_code_and_no_turn_done() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-crash.ndjson").await;
    let events = rest(&mut worker).await;
    let summaries = summaries(&events);
    assert_eq!(summaries.last().map(String::as_str), Some("exited Some(3)"), "{summaries:?}");
    assert!(
        !events.iter().any(|e| matches!(e, WorkerEvent::TurnDone { .. })),
        "the turn never finished, so the core fails the job: {summaries:?}"
    );
    assert_eq!(
        worker.bind(binding()).await,
        Err(DriverError::NoSession),
        "a crashed session takes no job"
    );
}

#[tokio::test]
async fn cancel_kills_the_session_and_its_process_is_gone() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-hold.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    let Some(WorkerEvent::Init { pid, .. }) = events.first() else {
        panic!("the first turn's init comes first: {events:?}");
    };
    assert_eq!(summary(&events[1]), "narration Waiting for the build to finish", "held mid-job");
    // The stub now holds at `await_user`: only the kill ends it. The code is the kill's, not
    // the `None` the driver gives when the exit is not seen within its deadline: on Windows the
    // Job Object's termination code 1, on Linux none, since a signal ended it.
    let code = cancel(&mut worker).await;
    if cfg!(windows) {
        assert_eq!(code, Some(1), "terminated with its Job Object");
    } else {
        assert_eq!(code, None, "ended by a signal");
        let proc = PathBuf::from(format!("/proc/{pid}"));
        assert!(!proc.exists(), "the stub's process {pid} is gone and reaped");
    }
    assert_eq!(
        worker.send(WorkerCmd::Prompt { text: "go on".to_owned() }).await,
        Err(DriverError::NoSession),
        "a cancelled session takes no turn"
    );
}

#[tokio::test]
async fn a_new_session_after_a_cancel_gives_none_of_the_old_ones_events() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-hold.ndjson").await;
    events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    // Cancelled, and its `Exited` never read.
    timeout(HANG, worker.send(WorkerCmd::Cancel)).await.unwrap().unwrap();
    start_and_bind(&mut worker).await;
    let first = next(&mut worker).await;
    assert!(
        matches!(first, Some(WorkerEvent::Init { .. })),
        "the new session's init, not the old one's exit: {first:?}"
    );
    cancel(&mut worker).await;
    assert_eq!(recorded_envs(&dir).len(), 2, "two sessions");
}

#[tokio::test]
async fn a_later_turn_and_an_interrupt_reach_the_session() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-hold.ndjson").await;
    events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    // The first turn's init offers `interrupt_receipt_v1`, so the interrupt is written, and the
    // stub's reply settles it without an event.
    timeout(HANG, worker.send(WorkerCmd::Interrupt)).await.unwrap().unwrap();
    let text = "Carry on".to_owned();
    timeout(HANG, worker.send(WorkerCmd::Prompt { text })).await.unwrap().unwrap();
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    assert_eq!(
        summaries(&events),
        ["narration Carrying on after the next user turn"],
        "the second turn plays, and the init the CLI writes again at it is not passed on"
    );
    cancel(&mut worker).await;
}

#[tokio::test]
async fn a_session_without_eileen_connected_is_never_bound() {
    let dir = Scratch::new();
    let mut worker = worker(&dir, "driver-no-eileen.ndjson");
    let started = timeout(HANG, worker.start(spec())).await.unwrap();
    assert_eq!(started.err(), Some(DriverError::EileenNotConnected), "refused at mcp_status");
    assert_eq!(worker.bind(binding()).await, Err(DriverError::NoSession), "never bound");
    assert_eq!(next(&mut worker).await, None, "no event from a refused session");
    assert_eq!(recorded_envs(&dir).len(), 1, "started once: a missing server is not retried");
}

#[test]
fn a_session_whose_first_init_reports_a_memory_path_is_killed_at_that_line() {
    let dir = Scratch::new();
    let (events, logs) = with_logs(async {
        let mut worker = started(&dir, "driver-memory-path.ndjson").await;
        let events = rest(&mut worker).await;
        let prompt = WorkerCmd::Prompt { text: "go on".to_owned() };
        assert_eq!(worker.send(prompt).await, Err(DriverError::NoSession), "no further turn");
        events
    });
    assert_eq!(events.len(), 1, "no init, no narration, only the exit: {events:?}");
    assert!(matches!(events[0], WorkerEvent::Exited { .. }), "{events:?}");
    assert_eq!(
        refusals(&logs),
        [DriverError::MemoryPathReported.to_string()],
        "the kill is logged with its reason"
    );
    let envs = recorded_envs(&dir);
    assert_eq!(envs.len(), 1, "not restarted: the task already reached it");
    assert_eq!(
        envs[0].get("CLAUDE_CODE_DISABLE_AUTO_MEMORY").map(String::as_str),
        Some("1"),
        "it ran with vendor memory off, and reported a folder anyway"
    );
}

#[test]
fn a_later_init_that_reports_a_memory_path_kills_the_session() {
    let dir = Scratch::new();
    let (events, logs) = with_logs(async {
        let mut worker = started(&dir, "driver-memory-later.ndjson").await;
        events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
        let text = "Carry on".to_owned();
        timeout(HANG, worker.send(WorkerCmd::Prompt { text })).await.unwrap().unwrap();
        let events = rest(&mut worker).await;
        let prompt = WorkerCmd::Prompt { text: "go on".to_owned() };
        assert_eq!(worker.send(prompt).await, Err(DriverError::NoSession), "no further turn");
        events
    });
    assert_eq!(events.len(), 1, "nothing after the second init but the exit: {events:?}");
    assert!(matches!(events[0], WorkerEvent::Exited { .. }), "{events:?}");
    assert_eq!(
        refusals(&logs),
        [DriverError::MemoryPathReported.to_string()],
        "the kill is logged with its reason"
    );
}

#[tokio::test]
async fn a_tool_input_that_quotes_a_session_token_is_redacted() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-token.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::ToolUse { .. })).await;
    let Some(WorkerEvent::ToolUse { input, .. }) = events.last() else { panic!("{events:?}") };
    let text = input.get().to_string();
    assert!(!text.contains(SHIM_TOKEN), "the token is redacted: {text}");
    assert!(text.contains("echo [redacted"), "the rest of the command stays: {text}");
    cancel(&mut worker).await;
}

#[tokio::test]
async fn the_session_starts_in_its_folder_with_the_denylist_applied_and_memory_off() {
    let dir = Scratch::new();
    let mut worker = started(&dir, "driver-job.ndjson").await;
    let run = dir.join("data").join("run").join(session_id().to_string());
    assert!(run.join("cwd").is_dir(), "the session's working folder");
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(run.join(SETTINGS_FILE)).unwrap()).unwrap();
    assert_eq!(settings["fileCheckpointingEnabled"], Value::Bool(false), "checkpointing off");
    let mcp: Value =
        serde_json::from_str(&fs::read_to_string(run.join(MCP_FILE)).unwrap()).unwrap();
    assert!(mcp["mcpServers"]["eileen"].is_object(), "Eileen's server in the MCP config");
    for file in [SETTINGS_FILE, MCP_FILE] {
        let text = fs::read_to_string(run.join(file)).unwrap();
        assert!(!text.contains("token-for-the-stub"), "{file} holds no token");
    }

    let envs = recorded_envs(&dir);
    assert_eq!(envs.len(), 1, "one process");
    let env = &envs[0];
    // The session's own variables are added after the denylist and some match it by name
    // (`EILEEN_HOOK_TOKEN`, `MCP_TOOL_TIMEOUT`); nothing else that matches it may be there.
    let denied: Vec<&String> = env
        .keys()
        .filter(|name| is_denylisted(name) && !SESSION_VARS.contains(&name.as_str()))
        .collect();
    assert!(denied.is_empty(), "no denylisted variable of the user's reaches the CLI: {denied:?}");
    for (name, _) in DENIED {
        assert!(!env.keys().any(|key| key.eq_ignore_ascii_case(name)), "{name} is removed");
    }
    let get = |name: &str| env.get(name).map(String::as_str);
    assert_eq!(get("CLAUDE_CODE_DISABLE_AUTO_MEMORY"), Some("1"), "vendor memory off (D77)");
    assert_eq!(get("CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING"), Some("1"), "checkpointing off");
    assert_eq!(get("ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"), "connectors off");
    assert_eq!(get("HTTPS_PROXY"), Some("http://127.0.0.1:3128"), "the pass-through list");
    assert_eq!(get("PATH"), Some(USER_PATH), "the user's PATH, as it is");
    assert_eq!(get("EILEEN_SESSION_ID"), Some(session_id().to_string().as_str()), "its id");
    cancel(&mut worker).await;
}

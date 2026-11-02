//! `ClaudeWorker` running jobs against this stub in place of the claude CLI (BUILD.md section
//! 6.5, acceptance 2 without the pool and the approval path, the stub half of acceptance 4 and
//! the init part of acceptance 8). The tests live here, not in `eileen-harness`, because Cargo
//! names the stub's binary (`CARGO_BIN_EXE_eileen-stub-harness`) only to this package's tests.
//!
//! Each test gives the worker the stub as its executable, a script through `EILEEN_STUB_SCRIPT`
//! in the user's environment (the driver passes it on, as it is not denylisted) and a data
//! folder of its own. Nothing here sleeps: the stub holds at `await_user` until the driver
//! writes, and `HANG` only guards against a hang.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
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
use serde_json::Value;
use tokio::time::timeout;

/// How long a call may take before the test calls the driver hung: a guard, far above anything
/// a working stub needs, never a measurement.
const HANG: Duration = Duration::from_secs(20);

/// The vendor session id every script's `system/init` carries.
const VENDOR_SESSION: &str = "a3b57c79-73b3-485d-8511-c5ae9b1cdac5";

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

/// A fresh folder of this test's own under the system's temporary folder.
fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("eileen-worker-{}-{n}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A worker whose `claude` is the stub playing `name`, with the denylisted variables, a
/// pass-through proxy and the user's own attempt to turn vendor memory on in its environment.
/// The stub appends its environment to `<dir>/env.ndjson`.
fn worker(dir: &Path, name: &str) -> ClaudeWorker {
    let mut vars: Vec<(String, String)> =
        DENIED.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
    vars.extend([
        ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:3128".to_owned()),
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
    ClaudeWorker::new(cfg)
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
            token: SessionToken::new("shim-token-for-the-stub".to_owned()),
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

/// Starts a session on `name` and binds the job to it.
async fn started(dir: &Path, name: &str) -> ClaudeWorker {
    let mut worker = worker(dir, name);
    let session = timeout(HANG, worker.start(spec())).await.unwrap().unwrap();
    assert_eq!(session.session, session_id(), "Eileen's id for the session");
    assert_eq!(session.vendor_session, VENDOR_SESSION, "the CLI's id, from system/init");
    assert_eq!(session.model, "claude-haiku-5-5", "the resolved model");
    timeout(HANG, worker.bind(binding())).await.unwrap().unwrap();
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

/// Cancels the session and checks it ends: one `Exited`, then nothing.
async fn cancel(worker: &mut ClaudeWorker) {
    timeout(HANG, worker.send(WorkerCmd::Cancel)).await.unwrap().unwrap();
    assert!(matches!(next(worker).await, Some(WorkerEvent::Exited { .. })), "the exit");
    assert_eq!(next(worker).await, None, "nothing after the exit");
}

/// The environments the stub recorded, one per process it ran as.
fn recorded_envs(dir: &Path) -> Vec<BTreeMap<String, String>> {
    let text = fs::read_to_string(dir.join("env.ndjson")).unwrap_or_default();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[tokio::test]
async fn a_job_runs_to_completion_with_its_events_in_order() {
    let dir = scratch();
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
        "the job's events, the init first"
    );
    cancel(&mut worker).await;
}

#[tokio::test]
async fn narration_lines_become_narration_and_other_text_stays_text() {
    let dir = scratch();
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
    let dir = scratch();
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
    let dir = scratch();
    let mut worker = started(&dir, "driver-crash.ndjson").await;
    let mut events = Vec::new();
    while let Some(event) = next(&mut worker).await {
        events.push(event);
    }
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
async fn cancel_kills_the_session_and_leaves_no_child() {
    let dir = scratch();
    let mut worker = started(&dir, "driver-hold.ndjson").await;
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    assert_eq!(summary(&events[1]), "narration Waiting for the build to finish", "held mid-job");
    // The stub now holds at `await_user`: only the kill of its Job Object ends it. `cancel`
    // checks that the driver waited for the exit, which on Windows is every process in the job.
    cancel(&mut worker).await;
    assert_eq!(
        worker.send(WorkerCmd::Prompt { text: "go on".to_owned() }).await,
        Err(DriverError::NoSession),
        "a cancelled session takes no turn"
    );
}

#[tokio::test]
async fn a_later_turn_and_an_interrupt_reach_the_session() {
    let dir = scratch();
    let mut worker = started(&dir, "driver-hold.ndjson").await;
    events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    // `system/init` offers `interrupt_receipt_v1`, so the interrupt is written, and the stub's
    // reply settles it without an event.
    timeout(HANG, worker.send(WorkerCmd::Interrupt)).await.unwrap().unwrap();
    let text = "Carry on".to_owned();
    timeout(HANG, worker.send(WorkerCmd::Prompt { text })).await.unwrap().unwrap();
    let events = events_until(&mut worker, |e| matches!(e, WorkerEvent::Narration { .. })).await;
    assert_eq!(
        summaries(&events),
        ["narration Carrying on after the next user turn"],
        "the second turn plays, and its system/init is not passed on twice"
    );
    cancel(&mut worker).await;
}

#[tokio::test]
async fn a_session_without_eileen_connected_is_never_bound() {
    let dir = scratch();
    let mut worker = worker(&dir, "driver-no-eileen.ndjson");
    let started = timeout(HANG, worker.start(spec())).await.unwrap();
    assert_eq!(started.err(), Some(DriverError::EileenNotConnected), "refused at init");
    assert_eq!(worker.bind(binding()).await, Err(DriverError::NoSession), "never bound");
    assert_eq!(next(&mut worker).await, None, "no event from a refused session");
    assert_eq!(recorded_envs(&dir).len(), 1, "started once: a missing server is not retried");
}

#[tokio::test]
async fn a_session_that_reports_a_memory_path_is_restarted_once_then_refused() {
    let dir = scratch();
    let mut worker = worker(&dir, "driver-memory-path.ndjson");
    let started = timeout(HANG, worker.start(spec())).await.unwrap();
    assert_eq!(started.err(), Some(DriverError::MemoryPathReported), "refused at init");
    assert_eq!(worker.bind(binding()).await, Err(DriverError::NoSession), "never bound");
    assert_eq!(next(&mut worker).await, None, "no event from a refused session");
    let envs = recorded_envs(&dir);
    assert_eq!(envs.len(), 2, "the session was restarted once with memory off");
    for env in &envs {
        assert_eq!(
            env.get("CLAUDE_CODE_DISABLE_AUTO_MEMORY").map(String::as_str),
            Some("1"),
            "each start has vendor memory off"
        );
    }
}

#[tokio::test]
async fn the_session_starts_in_its_folder_with_the_denylist_applied_and_memory_off() {
    let dir = scratch();
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
    assert_eq!(get("EILEEN_SESSION_ID"), Some(session_id().to_string().as_str()), "its id");
    cancel(&mut worker).await;
}

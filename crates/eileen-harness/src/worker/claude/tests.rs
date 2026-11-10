use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use eileen_contracts::harness::{
    BuildSlotTicket, JobBinding, SessionSpec, WorkerCmd, WorkerEvent, WorktreeSpec,
};
use eileen_contracts::ids::{JobId, Personal, SkillId, TraceCtx, WorkerSessionId};
use eileen_contracts::jobs::JobKind;
use eileen_contracts::learn::{SkillRef, SnapshotManifest};
use eileen_supervisor::ChildSpec;
use eileen_trace::Redactor;
use eileen_trace::redact::LiteralGuard;
use eileen_trace::test_support::capture;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader, duplex};
use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

use super::{
    Asked, ClaudeWorker, EXIT_DEADLINE, KEPT_LINE_CAPACITY, Lines, Pipes, Process, STATUS_POLL,
    Session, Status, check_init, eileen_status, first_turn, reply_to, unsupported, user_line,
    write_files,
};
use crate::session::session_files;
use crate::stream::control::Requests;
use crate::stream::{Init, Line, MAX_LINE_BYTES, parse};
use crate::{DriverError, HarnessConfig, WorkerDriver as _};

/// How long a call may take before a test calls it hung: a guard above every deadline the
/// driver has, never a measurement.
const HANG: Duration = Duration::from_secs(30);

/// The output of a future that answers without waiting, as every refusal before a spawn does.
fn at_once<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a refusal answers at once"),
    }
}

/// The coding worker's spec without its build slot, which the driver does not hold yet.
fn coding_spec() -> SessionSpec {
    let mut spec = crate::session::tests::coding_worker();
    spec.build_slot = None;
    spec
}

fn binding() -> JobBinding {
    JobBinding {
        job: JobId::from_parts(1_700_000_000_000, 1),
        kind: JobKind::Coding,
        task: "Rename the build script".to_owned(),
        skill: None,
        inline_skill: None,
        facts: Vec::new(),
        trace: TraceCtx::default(),
    }
}

/// A `system/init` line with `eileen` at `status`, and `memory_paths` when `memory` is set.
fn init(status: &str, memory: Option<&str>) -> Init {
    let mut line = json!({
        "type": "system",
        "subtype": "init",
        "session_id": "a3b57c79-73b3-485d-8511-c5ae9b1cdac5",
        "model": "claude-haiku-5-5",
        "tools": ["Bash"],
        "mcp_servers": [{ "name": "eileen", "status": status }],
        "capabilities": ["interrupt_receipt_v1"],
    });
    if let (Some(path), Value::Object(fields)) = (memory, &mut line) {
        fields.insert("memory_paths".to_owned(), json!({ "auto": path }));
    }
    let parsed = parse(line.to_string().as_bytes(), &Redactor::new());
    let Line::Init(init) = &parsed else { panic!("not an init: {parsed:?}") };
    init.as_ref().clone()
}

/// [`binding`] with the skill `cargo-rename-bin` and a facts block.
fn skill_binding() -> JobBinding {
    let mut binding = binding();
    binding.skill = Some(SkillRef { id: SkillId::new("cargo-rename-bin").unwrap(), version: 3 });
    binding.facts =
        vec!["The repository builds with cargo xtask".to_owned(), "Rust 1.95".to_owned()];
    binding
}

#[test]
fn the_first_turn_is_the_task_alone_without_a_skill_or_facts() {
    assert_eq!(first_turn(&binding()), Ok("Rename the build script".to_owned()), "the task");
}

#[test]
fn the_first_turn_names_a_skill_in_the_snapshot_and_carries_the_facts_block() {
    assert_eq!(
        first_turn(&skill_binding()).unwrap(),
        "Rename the build script\n\n\
         Follow skill eileen-learned:cargo-rename-bin\n\n\
         Facts:\n\
         - The repository builds with cargo xtask\n\
         - Rust 1.95",
        "task, skill line, facts"
    );
}

#[test]
fn the_first_turn_names_the_skill_carries_its_inline_text_and_the_facts_block() {
    let mut binding = skill_binding();
    binding.inline_skill = Some("1. Edit Cargo.toml's [[bin]] name.".to_owned());
    let turn = first_turn(&binding).unwrap();
    assert_eq!(
        turn,
        "Rename the build script\n\n\
         Follow these steps (Eileen skill cargo-rename-bin):\n\n\
         1. Edit Cargo.toml's [[bin]] name.\n\n\
         Facts:\n\
         - The repository builds with cargo xtask\n\
         - Rust 1.95",
        "task, the skill's text under a heading naming it, facts"
    );
    assert!(
        !turn.contains("eileen-learned:"),
        "the snapshot lacks the skill, so the model is not sent to call it (section 2.10)"
    );
}

#[test]
fn a_skills_text_without_its_skill_is_refused() {
    let mut binding = binding();
    binding.inline_skill = Some("1. Edit Cargo.toml's [[bin]] name.".to_owned());
    assert_eq!(first_turn(&binding), Err(DriverError::UnnamedInlineSkill), "nothing names it");
}

#[test]
fn a_user_line_has_the_shape_the_contract_test_wrote() {
    let line: Value = serde_json::from_str(&user_line("Write hi to hello.txt")).unwrap();
    assert_eq!(
        line,
        json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": "Write hi to hello.txt" }],
            },
            "parent_tool_use_id": null,
            "session_id": "",
        }),
        "worker-command.json's first_user_line"
    );
    assert!(!user_line("a\nb").contains('\n'), "one line, whatever the text holds");
}

#[test]
fn only_an_init_with_eileen_connected_and_no_memory_folder_may_be_bound() {
    assert_eq!(check_init(&init("connected", None)), Ok(()), "ready");
    for status in ["failed", "pending", "needs-auth"] {
        assert_eq!(
            check_init(&init(status, None)),
            Err(DriverError::EileenNotConnected),
            "eileen {status}"
        );
    }
    assert_eq!(
        check_init(&init("connected", Some("/data/eileen/memory/claude"))),
        Err(DriverError::MemoryPathReported),
        "any memory folder at all, while every session runs with memory off"
    );
}

#[test]
fn a_worker_without_a_located_cli_refuses_to_start() {
    let mut worker = ClaudeWorker::new(HarnessConfig::default(), Arc::new(Redactor::new()));
    let spec = coding_spec();
    assert_eq!(at_once(worker.start(spec)).err(), Some(DriverError::NoExecutable), "no CLI");
}

#[test]
fn a_session_that_was_never_started_takes_no_job_and_no_turn() {
    let cfg = HarnessConfig {
        claude_exe: PathBuf::from("/opt/claude/claude"),
        ..HarnessConfig::default()
    };
    let mut worker = ClaudeWorker::new(cfg, Arc::new(Redactor::new()));
    assert_eq!(at_once(worker.bind(binding())), Err(DriverError::NoSession), "no job");
    let prompt = WorkerCmd::Prompt { text: "go on".to_owned() };
    assert_eq!(at_once(worker.send(prompt)), Err(DriverError::NoSession), "no turn");
    assert_eq!(at_once(worker.send(WorkerCmd::Interrupt)), Err(DriverError::NoSession), "none");
    assert_eq!(at_once(worker.send(WorkerCmd::Cancel)), Ok(()), "cancel of nothing is done");
    assert_eq!(
        at_once(worker.send(WorkerCmd::Recycle)),
        Err(DriverError::Unsupported { command: "recycle" }),
        "the pool's commands come later"
    );
    assert!(at_once(worker.next_event()).is_none(), "no event");
}

#[test]
fn a_spec_asking_for_what_the_driver_does_not_carry_out_is_refused() {
    let refused = |change: fn(&mut SessionSpec)| {
        let mut spec = coding_spec();
        change(&mut spec);
        unsupported(&spec)
    };
    assert_eq!(refused(|_| {}), Ok(()), "the spec the core builds today");
    let expect_refused = |command| Err(DriverError::Unsupported { command });
    assert_eq!(
        refused(|spec| {
            spec.snapshot = Some(SnapshotManifest { session: spec.session, files: Vec::new() });
        }),
        expect_refused("a plugin snapshot check"),
        "the snapshot is not re-hashed or checked against init yet"
    );
    assert_eq!(
        refused(|spec| spec.snapshot_ref = Some("refs/eileen/1".to_owned())),
        expect_refused("a snapshot restore"),
        "no snapshot is restored yet"
    );
    assert_eq!(
        refused(|spec| {
            spec.worktree = Some(WorktreeSpec {
                repo: Personal::new("repo".to_owned()),
                path: Personal::new("wt".to_owned()),
                branch: "eileen/job".to_owned(),
                base: "main".to_owned(),
            });
        }),
        expect_refused("a worktree"),
        "no worktree is made yet, so the job would run in the run folder"
    );
    assert_eq!(
        refused(|spec| spec.build_slot = Some(BuildSlotTicket { slot: 0, cargo_jobs: 4 })),
        expect_refused("a build slot"),
        "no build slot is held yet"
    );
    assert_eq!(
        refused(|spec| spec.memory_limit_mb = Some(2048)),
        expect_refused("a job memory limit"),
        "no JOB_OBJECT_LIMIT_JOB_MEMORY yet"
    );
}

#[test]
fn a_spec_the_driver_cannot_carry_out_is_refused_before_a_spawn() {
    let cfg = HarnessConfig {
        claude_exe: PathBuf::from("/opt/claude/claude"),
        ..HarnessConfig::default()
    };
    let mut worker = ClaudeWorker::new(cfg, Arc::new(Redactor::new()));
    let mut spec = coding_spec();
    spec.memory_limit_mb = Some(2048);
    assert_eq!(
        at_once(worker.start(spec)).err(),
        Some(DriverError::Unsupported { command: "a job memory limit" }),
        "refused at once"
    );
}

#[test]
fn eileen_must_be_connected_in_mcp_status() {
    let status = |servers: Value| eileen_status(&json!({ "mcpServers": servers }));
    let eileen = |state: &str| json!([{ "name": "other", "status": "connected" }, { "name": "eileen", "status": state }]);
    assert_eq!(status(eileen("connected")), Status::Connected, "ready");
    assert_eq!(status(eileen("pending")), Status::Pending, "asked again");
    assert_eq!(status(json!([])), Status::Pending, "not listed yet: asked again");
    assert_eq!(eileen_status(&Value::Null), Status::Pending, "no list at all: asked again");
    for state in ["failed", "needs-auth", "disabled"] {
        assert_eq!(status(eileen(state)), Status::Refused, "eileen {state}");
    }
}

#[test]
fn only_the_reply_to_the_request_asked_is_taken() {
    let reply = |id: &str, subtype: &str| {
        json!({
            "type": "control_response",
            "response": { "subtype": subtype, "request_id": id, "response": { "mcpServers": [] } },
        })
        .to_string()
    };
    assert_eq!(
        reply_to(reply("eileen_ready_2", "success").as_bytes(), "eileen_ready_2"),
        Some(Ok(json!({ "mcpServers": [] }))),
        "its response"
    );
    assert_eq!(
        reply_to(reply("eileen_ready_2", "error").as_bytes(), "eileen_ready_2"),
        Some(Err(())),
        "an error reply"
    );
    let other = reply("eileen_ready_1", "success");
    assert_eq!(reply_to(other.as_bytes(), "eileen_ready_2"), None, "another request's reply");
    let init = br#"{"type":"system","subtype":"init"}"#;
    assert_eq!(reply_to(init, "eileen_ready_2"), None, "not a reply");
    assert_eq!(reply_to(b"not json", "eileen_ready_2"), None, "not JSON");
}

#[test]
fn a_read_dropped_partway_keeps_the_part_of_the_line_it_had_read() {
    let (mut writer, reader) = duplex(1024);
    let mut lines = Lines::new(BufReader::new(reader));
    let whole =
        br#"{"type":"control_response","response":{"subtype":"success","request_id":"eileen_1"}}"#;
    let (head, tail) = whole.split_at(30);
    at_once(writer.write_all(head)).unwrap();
    // Dropped after one poll, as a `select!` drops it when another branch wins.
    let mut read = Box::pin(lines.next());
    let polled = read.as_mut().poll(&mut Context::from_waker(Waker::noop()));
    assert!(polled.is_pending(), "half a line is not a line");
    drop(read);
    at_once(writer.write_all(tail)).unwrap();
    at_once(writer.write_all(b"\r\n\n")).unwrap();
    let line = at_once(lines.next()).map(<[u8]>::to_vec);
    assert_eq!(line.as_deref(), Some(&whole[..]), "the whole line, once, without its ending");
    drop(writer);
    assert_eq!(at_once(lines.next()), None, "the blank line is passed over, then stdout ends");
}

/// An email-shaped string in a CLI's `initialize` reply, which carries the signed-in account
/// (section 3.3.8). It names no real account.
const ACCOUNT_EMAIL: &str = "someone.signed.in@example.com";

/// A success `control_response` to `request_id` with `response`.
fn control_reply(request_id: &str, response: &Value) -> String {
    json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    })
    .to_string()
}

/// An error `control_response` to `request_id`, in the CLI's shape: `error` beside `subtype`,
/// where the control parser reads it.
fn control_error(request_id: &str, error: &str) -> String {
    json!({
        "type": "control_response",
        "response": { "subtype": "error", "request_id": request_id, "error": error },
    })
    .to_string()
}

/// An `mcp_status` response listing `eileen` at `status`.
fn eileen_at(status: &str) -> Value {
    json!({ "mcpServers": [{ "name": "eileen", "status": status }] })
}

/// What one readiness exchange did.
#[derive(Debug)]
struct Exchange {
    ready: Result<(), Asked>,
    /// The subtype of each control request the driver wrote, in order.
    asked: Vec<String>,
    /// How long it took on the test's paused clock, which moves only when the driver sleeps.
    waited: Duration,
    /// Whether anything captured (a span, an event, any field) holds [`ACCOUNT_EMAIL`].
    leaked: bool,
    /// How many spans and events were captured.
    logged: usize,
}

/// The readiness exchange over an in-memory pipe whose CLI side has already written `replies`,
/// run with tracing captured on a paused clock, so the waits between `mcp_status` requests
/// cost nothing. A driver that waits for a reply that never comes fails at [`HANG`].
fn ready_against(replies: &[String]) -> Exchange {
    let (driver_stdin, mut cli_stdin) = duplex(64 * 1024);
    let (mut cli_stdout, driver_stdout) = duplex(64 * 1024);
    let mut pipes = Pipes::new(driver_stdin, BufReader::new(driver_stdout));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    let ((ready, waited), captured) = capture(|| {
        runtime.block_on(async {
            for reply in replies {
                cli_stdout.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            }
            let started = Instant::now();
            let ready = timeout(HANG, pipes.ready()).await.expect("the driver hung on a reply");
            (ready, started.elapsed())
        })
    });
    // Closes the driver's end of stdin, so what it wrote reads to its end.
    drop(pipes);
    let mut written = String::new();
    runtime.block_on(cli_stdin.read_to_string(&mut written)).unwrap();
    let asked = written
        .lines()
        .map(|line| {
            let request: Value = serde_json::from_str(line).unwrap();
            request["request"]["subtype"].as_str().unwrap().to_owned()
        })
        .collect();
    let leaked = format!("{captured:?}").contains(ACCOUNT_EMAIL);
    let logged = captured.events.len() + captured.spans.len();
    Exchange { ready, asked, waited, leaked, logged }
}

#[test]
fn the_initialize_reply_is_dropped_unread_and_never_logged() {
    let account = json!({
        "account": { "email": ACCOUNT_EMAIL, "organization": "Example Org" },
        "pid": 4242,
        "claude_code_version": "2.1.316",
    });
    let exchange = ready_against(&[
        // A line that is no reply to the request is passed over unread as well.
        json!({ "type": "system", "subtype": "notice", "account": ACCOUNT_EMAIL }).to_string(),
        control_reply("eileen_ready_1", &account),
        control_reply("eileen_ready_2", &eileen_at("connected")),
    ]);
    assert!(matches!(exchange.ready, Ok(())), "ready once eileen is connected: {exchange:?}");
    assert_eq!(exchange.asked, ["initialize", "mcp_status"], "the two requests, in order");
    assert!(!exchange.leaked, "no span, event or field holds the account");
    assert_eq!(exchange.logged, 0, "the readiness exchange logs nothing at all");

    // An error reply that quotes the account is refused by the request's name alone.
    let error = format!("signed in as {ACCOUNT_EMAIL}");
    let exchange = ready_against(&[control_error("eileen_ready_1", &error)]);
    let Err(Asked::Failed(refused)) = &exchange.ready else { panic!("refused: {exchange:?}") };
    assert_eq!(*refused, DriverError::ControlRefused { request: "initialize" }, "by name");
    assert!(!refused.to_string().contains(ACCOUNT_EMAIL), "the error quotes none of the reply");
    assert!(!exchange.leaked, "no span, event or field holds the account");
    assert_eq!(exchange.logged, 0, "nothing logged");
}

#[test]
fn mcp_status_is_asked_again_while_eileen_is_not_listed_or_pending_until_it_connects() {
    let exchange = ready_against(&[
        control_reply("eileen_ready_1", &json!({})),
        control_reply("eileen_ready_2", &json!({ "mcpServers": [] })),
        control_reply("eileen_ready_3", &eileen_at("pending")),
        control_reply("eileen_ready_4", &eileen_at("connected")),
    ]);
    assert!(matches!(exchange.ready, Ok(())), "ready once eileen is connected: {exchange:?}");
    assert_eq!(
        exchange.asked,
        ["initialize", "mcp_status", "mcp_status", "mcp_status"],
        "asked again after the reply that does not list it and the one where it is pending"
    );
    assert!(
        exchange.waited >= STATUS_POLL * 2 && exchange.waited < STATUS_POLL * 3,
        "one poll interval before each request asked again: {:?}",
        exchange.waited
    );
}

/// A runtime for a test that starts a process or writes files, on the real clock: on Windows
/// tokio waits for a child on threads of its own, which a paused clock does not wait for.
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// A process that runs for a minute and writes nothing on stderr, started through the
/// supervisor as the CLI is. It stands in for a session whose stdout has closed while its job
/// still runs: a background process a `Bash` call left running holds a Windows job open.
fn quiet_process() -> Process {
    let spec = if cfg!(windows) {
        ChildSpec::new("claude.test", "ping").args(["-n", "60", "127.0.0.1"])
    } else {
        ChildSpec::new("claude.test", "sleep").args(["60"])
    };
    Process::start(&spec).unwrap()
}

/// A bound session of `process` holding `secrets`, as `start` and `bind` leave one.
fn session_of(process: Process, secrets: Vec<LiteralGuard>) -> Session {
    Session {
        id: WorkerSessionId::from_parts(1_700_000_000_000, 21),
        process,
        init: None,
        requests: Requests::default(),
        bound: true,
        _secrets: secrets,
    }
}

/// A worker holding `session`, as if it had started it.
fn worker_with(session: Session, redactor: Arc<Redactor>) -> ClaudeWorker {
    let mut worker = ClaudeWorker::new(HarnessConfig::default(), redactor);
    worker.session = Some(session);
    worker
}

#[test]
fn cancel_kills_a_session_that_a_dropped_call_left_waiting_for_its_own_exit() {
    runtime().block_on(async {
        let mut worker = worker_with(session_of(quiet_process(), Vec::new()), Arc::default());
        // Its stdout closed, so `next_event` ends it without a kill and waits for its exit; a
        // `select!` whose other branch won drops that call inside the wait.
        let dropped = timeout(Duration::from_millis(100), worker.end(false)).await;
        assert!(dropped.is_err(), "still waiting for it to exit on its own");
        let cancelled = Instant::now();
        timeout(HANG, worker.send(WorkerCmd::Cancel)).await.unwrap().unwrap();
        let took = cancelled.elapsed();
        assert!(took < EXIT_DEADLINE / 2, "killed at once, not after its wait: {took:?}");
        let exited = timeout(HANG, worker.next_event()).await.unwrap();
        assert!(matches!(exited, Some(WorkerEvent::Exited { .. })), "its exit: {exited:?}");
        assert_eq!(timeout(HANG, worker.next_event()).await.unwrap(), None, "then nothing");
    });
}

#[test]
fn a_wait_for_its_exit_dropped_again_and_again_still_kills_it_at_its_deadline() {
    runtime().block_on(async {
        let mut worker = worker_with(session_of(quiet_process(), Vec::new()), Arc::default());
        let closed = Instant::now();
        let first = timeout(Duration::from_millis(250), worker.end(false)).await;
        assert!(first.is_err(), "still waiting for it to exit on its own");
        // Each call dropped well inside the wait, as a `select!` drops it when another of its
        // branches fires more often than the deadline.
        let exited = loop {
            if let Ok(event) = timeout(Duration::from_millis(250), worker.next_event()).await {
                break event;
            }
            assert!(closed.elapsed() < EXIT_DEADLINE * 2, "the wait restarted at every call");
        };
        let took = closed.elapsed();
        assert!(matches!(exited, Some(WorkerEvent::Exited { .. })), "its exit: {exited:?}");
        assert!(took >= EXIT_DEADLINE, "its whole wait for its own exit: {took:?}");
        assert!(took < EXIT_DEADLINE + EXIT_DEADLINE / 2, "then the kill, at once: {took:?}");
    });
}

#[test]
fn a_session_ends_only_once_its_stderr_forwarding_has_ended_with_its_tokens_registered() {
    runtime().block_on(async {
        let redactor = Arc::new(Redactor::new());
        let token = redactor.add_literal("token-of-a-session-being-ended").unwrap();
        let mut process = quiet_process();
        // Stands in for the forwarding while the CLI's last stderr lines are still being logged.
        let (logged, still_logging) = oneshot::channel::<()>();
        process.stderr = Some(eileen_rt::task::spawn("claude.test.stderr", async move {
            let _released: Result<(), oneshot::error::RecvError> = still_logging.await;
        }));
        let mut worker = worker_with(session_of(process, vec![token]), Arc::clone(&redactor));
        let dropped = timeout(Duration::from_millis(250), worker.send(WorkerCmd::Cancel)).await;
        assert!(dropped.is_err(), "killed, and not ended while its stderr is still logged");
        assert_eq!(redactor.literal_count(), 1, "its token is still redacted");
        logged.send(()).unwrap();
        let exited = timeout(HANG, worker.next_event()).await.unwrap();
        assert!(matches!(exited, Some(WorkerEvent::Exited { .. })), "its exit: {exited:?}");
        assert_eq!(redactor.literal_count(), 0, "its token is let go after its last line");
    });
}

/// A fresh folder under the system's temporary folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-claude-{}-{n}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a failed removal leaves only a folder in the temporary folder.
        let _removed: io::Result<()> = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_the_scratch_working_folder_is_made_and_a_missing_repository_is_refused() {
    let dir = Scratch::new();
    let cfg = HarnessConfig {
        data_dir: dir.path().join("data"),
        shim_exe: dir.path().join("eileen-shim"),
        ..HarnessConfig::default()
    };
    let spec = coding_spec();
    let files = session_files(&spec, &cfg, &cfg.host).unwrap();
    let in_folder = |cwd: PathBuf| {
        let mut files = files.clone();
        files.cwd = cwd;
        files
    };
    let not_found =
        Err(DriverError::Files { what: "working folder", kind: io::ErrorKind::NotFound });
    runtime().block_on(async {
        // A repository path that names no folder, absolute as the core gives one.
        let missing = dir.path().join("no-such-repository");
        assert_eq!(write_files(&in_folder(missing.clone())).await, not_found, "refused");
        assert!(!missing.exists(), "never made empty in the user's tree");
        assert!(!files.run_dir.exists(), "refused before anything is written");
        // A path that starts in the run folder and climbs out of it is no scratch folder.
        let climbing = files.run_dir.join("cwd").join("..").join("..").join("elsewhere");
        assert_eq!(write_files(&in_folder(climbing)).await, not_found, "refused too");
        assert!(!files.run_dir.parent().unwrap().join("elsewhere").exists(), "not made");
        // A file is no working folder.
        let file = dir.path().join("a-file");
        fs::write(&file, "not a folder").unwrap();
        let not_a_folder =
            DriverError::Files { what: "working folder", kind: io::ErrorKind::NotADirectory };
        assert_eq!(write_files(&in_folder(file)).await, Err(not_a_folder), "refused");

        // The session's scratch folder, `run/<session>/cwd`, is made with the run folder.
        assert_eq!(write_files(&files).await, Ok(()), "{:?}", files.cwd);
        assert!(files.cwd.is_dir(), "the scratch working folder");
        assert!(files.settings_path.is_file() && files.mcp_path.is_file(), "the run files");
        // A repository that exists is used as it is.
        let repository = dir.path().join("repository");
        fs::create_dir_all(&repository).unwrap();
        assert_eq!(write_files(&in_folder(repository)).await, Ok(()), "an existing repository");
    });
}

#[test]
fn a_line_past_the_cap_is_given_one_byte_past_it_and_leaves_no_big_buffer() {
    let (mut writer, reader) = duplex(64 * 1024);
    let mut lines = Lines::new(BufReader::new(reader));
    let write = async move {
        // Past the cap; exactly at the cap with `\r\n`; past the cap with a `\r` just after it.
        let long: [(u8, usize, &[u8]); 3] = [
            (b'a', MAX_LINE_BYTES + 10, b"\n"),
            (b'b', MAX_LINE_BYTES, b"\r\n"),
            (b'c', MAX_LINE_BYTES, b"\rcc\n"),
        ];
        for (byte, len, ending) in long {
            writer.write_all(&vec![byte; len]).await.unwrap();
            writer.write_all(ending).await.unwrap();
        }
        writer.write_all(b"{\"type\":\"keep_alive\"}\n").await.unwrap();
    };
    let read = async {
        let redactor = Redactor::new();
        let over_cap = |line: &[u8]| {
            matches!(
                parse(line, &redactor),
                Line::Malformed { reason: "the line is longer than MAX_LINE_BYTES", .. }
            )
        };
        let line = lines.next().await.unwrap();
        assert_eq!(line.len(), MAX_LINE_BYTES + 1, "cut one byte past the cap");
        assert!(over_cap(line), "so the parser takes it as malformed");
        assert!(lines.line.capacity() <= MAX_LINE_BYTES + 1, "it cost no more than the cap");
        let line = lines.next().await.unwrap();
        assert_eq!(line.len(), MAX_LINE_BYTES, "a line exactly at the cap stays whole");
        assert!(!over_cap(line), "and is parsed");
        let line = lines.next().await.unwrap();
        assert_eq!(line.len(), MAX_LINE_BYTES + 1, "the `\\r` after the cap is no line ending");
        assert!(over_cap(line), "malformed too");
        let line = lines.next().await.map(<[u8]>::to_vec);
        assert_eq!(line.as_deref(), Some(&br#"{"type":"keep_alive"}"#[..]), "the next, whole");
        assert!(lines.line.capacity() <= KEPT_LINE_CAPACITY, "the big buffer was given back");
    };
    runtime().block_on(async { tokio::join!(write, read) });
}

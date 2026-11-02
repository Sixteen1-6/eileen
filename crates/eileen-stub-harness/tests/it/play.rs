//! The binary started through the supervisor and spoken to on its stdio as `ClaudeWorker` and
//! the brain driver speak to the claude CLI: each script's stdout read back through
//! `eileen_harness::stream`, the crash, the hold at `AwaitUser`, the interrupt reply, and the
//! typed errors.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::TurnId;
use eileen_harness::stream::control::{Message, Requests};
use eileen_harness::stream::{BrainAt, Line, brain_events, parse, worker_events};
use eileen_stub_harness::{SCRIPT_ARG, SCRIPT_ENV};
use eileen_supervisor::{Child, ChildSpec, spawn};
use eileen_trace::Redactor;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::time::timeout;

/// How long a line or an exit may take before the test calls the stub hung: a guard against a
/// hang, far above anything a working stub needs, never a measurement.
const HANG: Duration = Duration::from_secs(20);

/// The pid the worker mapping is given; the events compared leave it out.
const PID: u32 = 7;

/// The first user line `ClaudeWorker` writes, in the shape the contract test recorded
/// (`worker-command.json`'s `first_user_line`).
const USER_LINE: &str = concat!(
    r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Write hi "#,
    r#"to hello.txt"}]},"parent_tool_use_id":null,"session_id":""}"#,
    "\n",
);

/// The worker's argv after the executable, as the approval contract test recorded it
/// (`worker-command.json`), with its paths left out.
const CLAUDE_ARGV: &[&str] = &[
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-hook-events",
    "--model",
    "haiku",
    "--strict-mcp-config",
    "--tools",
    "Bash",
    "--permission-mode",
    "dontAsk",
    "--no-session-persistence",
    "--disallowedTools",
    "Task,Agent",
    "--permission-prompt-tool",
    "mcp__eileen__approve",
];

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/harness-scripts").join(name)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// The stub, started through the supervisor with its stdio piped.
struct Stub {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Stub {
    /// Starts the stub on `script`, named by the argument.
    fn start(script: &Path) -> Self {
        Self::from_spec(
            &ChildSpec::new("eileen-stub-harness", env!("CARGO_BIN_EXE_eileen-stub-harness"))
                .args([SCRIPT_ARG.into(), script.as_os_str().to_owned()]),
        )
    }

    fn from_spec(spec: &ChildSpec) -> Self {
        let mut child = spawn(spec).unwrap();
        let stdin = child.take_stdin();
        let lines = BufReader::new(child.take_stdout().unwrap()).lines();
        Self { child, stdin, lines }
    }

    async fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
    }

    /// Closes stdin, as the driver does when a session ends.
    fn close(&mut self) {
        drop(self.stdin.take());
    }

    async fn next(&mut self) -> Option<String> {
        timeout(HANG, self.lines.next_line()).await.expect("the stub hung").unwrap()
    }

    /// Every line left on stdout, until the stub closes it.
    async fn rest(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(line) = self.next().await {
            lines.push(line);
        }
        lines
    }

    async fn exit_code(&mut self) -> Option<i32> {
        timeout(HANG, self.child.wait()).await.expect("the stub did not exit").unwrap().code()
    }
}

/// Plays `name` with one user line per turn written up front and stdin then closed, and gives
/// its stdout's lines and its exit code.
async fn play(name: &str, turns: usize) -> (Vec<String>, Option<i32>) {
    let mut stub = Stub::start(&script(name));
    for _ in 0..turns {
        stub.send(USER_LINE).await;
    }
    stub.close();
    let lines = stub.rest().await;
    (lines, stub.exit_code().await)
}

fn parsed(line: &str) -> Line {
    parse(line.as_bytes(), &Redactor::new())
}

/// The worker events of `lines`, each written short: its kind and the fields a script sets.
fn worker(lines: &[String]) -> Vec<String> {
    lines.iter().flat_map(|line| worker_events(parsed(line), PID)).map(worker_summary).collect()
}

fn worker_summary(event: WorkerEvent) -> String {
    match event {
        WorkerEvent::Init { session_id, pid, .. } => format!("init {session_id} pid={pid}"),
        WorkerEvent::Narration { line } => format!("narration {}", line.get()),
        WorkerEvent::Text { delta } => format!("text {}", delta.get()),
        WorkerEvent::ToolUse { tool_use_id, name, input } => {
            format!("tool_use {name} {tool_use_id} {}", input.get())
        }
        WorkerEvent::ToolResult { tool_use_id, ok, .. } => {
            format!("tool_result {tool_use_id} ok={ok}")
        }
        WorkerEvent::SkillInvoked { name, args } => format!("skill {name} {}", args.get()),
        WorkerEvent::TurnDone { is_error, summary, .. } => {
            format!("turn_done is_error={is_error} {}", summary.get())
        }
        WorkerEvent::Malformed { line_len } => format!("malformed {line_len}"),
        other @ (WorkerEvent::MemoryLimit { .. }
        | WorkerEvent::RateLimit(_)
        | WorkerEvent::AuthFailed { .. }
        | WorkerEvent::Exited { .. }) => format!("{other:?}"),
    }
}

/// The brain events of `lines` for one turn in progress, each written short.
fn brain(lines: &[String]) -> Vec<String> {
    let at =
        BrainAt { which: BrainSlot::Primary, instance: 1, turn: Some(TurnId::from_parts(1, 1)) };
    lines.iter().flat_map(|line| brain_events(parsed(line), at)).map(brain_summary).collect()
}

fn brain_summary(event: BrainEvent) -> String {
    match event {
        BrainEvent::Ready { model, .. } => format!("ready {model}"),
        BrainEvent::TextDelta { text, .. } => format!("delta {}", text.get()),
        BrainEvent::TurnDone { is_error, .. } => format!("turn_done is_error={is_error}"),
        other @ (BrainEvent::RateLimit(_) | BrainEvent::Exited { .. }) => format!("{other:?}"),
    }
}

const SESSION: &str = "init a3b57c79-73b3-485d-8511-c5ae9b1cdac5 pid=7";

#[tokio::test]
async fn the_hello_job_narrates_writes_and_finishes() {
    let (lines, code) = play("worker-hello.ndjson", 1).await;
    let id = "toolu_01HelloTxtWriteScript01";
    let input = r#"{"command":"echo hi > hello.txt","description":"Write hi to hello.txt"}"#;
    let expected = [
        SESSION.to_owned(),
        "narration Creating hello.txt in the scratch repository".to_owned(),
        format!("tool_use Bash {id} {input}"),
        format!("tool_result {id} ok=true"),
        "text Created hello.txt containing hi.".to_owned(),
        "turn_done is_error=false Created hello.txt containing hi.".to_owned(),
    ];
    assert_eq!(worker(&lines), expected, "the worker events of the hello job");
    assert_eq!(code, Some(0), "it exits cleanly once stdin closes after the script");
}

#[tokio::test]
async fn the_skill_call_becomes_skill_invoked() {
    let (lines, code) = play("worker-skill.ndjson", 1).await;
    let id = "toolu_01SkillCallScript0001";
    let expected = [
        SESSION.to_owned(),
        "skill eileen-learned:resolve-goto-offset 15:30".to_owned(),
        format!("tool_result {id} ok=true"),
        "text Moved the playhead to 15:30.".to_owned(),
        "turn_done is_error=false Moved the playhead to 15:30.".to_owned(),
    ];
    assert_eq!(worker(&lines), expected, "the worker events of the skill call");
    assert_eq!(code, Some(0), "it exits cleanly");
}

#[tokio::test]
async fn a_crash_mid_turn_gives_its_exit_code() {
    let (lines, code) = play("worker-crash.ndjson", 1).await;
    let id = "toolu_01CrashMidTurnScript01";
    let input = r#"{"command":"cargo build","description":"Build the project"}"#;
    let expected = [
        SESSION.to_owned(),
        "narration Running the build".to_owned(),
        format!("tool_use Bash {id} {input}"),
    ];
    assert_eq!(worker(&lines), expected, "the lines before the crash, and no result");
    assert_eq!(code, Some(3), "the script's exit code");
}

#[tokio::test]
async fn a_malformed_line_is_written_as_is() {
    let (lines, code) = play("worker-malformed.ndjson", 1).await;
    let bytes = r#"{"type":"assistant","message":{"content":[{"type":"text","te"#;
    assert_eq!(lines.get(1).map(String::as_str), Some(bytes), "the bytes, unchanged");
    let expected = [
        SESSION.to_owned(),
        format!("malformed {}", bytes.len()),
        "text Done.".to_owned(),
        "turn_done is_error=false Done.".to_owned(),
    ];
    assert_eq!(worker(&lines), expected, "the worker events around the malformed line");
    assert_eq!(code, Some(0), "it exits cleanly");
}

#[tokio::test]
async fn the_brain_conversation_streams_two_turns_of_deltas() {
    let (lines, code) = play("brain-two-turns.ndjson", 2).await;
    let expected = [
        "ready claude-haiku-5-5",
        "delta It is ",
        "delta half past three.",
        "turn_done is_error=false",
        "ready claude-haiku-5-5",
        "delta You have ",
        "delta one meeting ",
        "delta at four.",
        "turn_done is_error=false",
    ];
    assert_eq!(brain(&lines), expected, "the brain events of both turns");
    assert_eq!(code, Some(0), "it exits cleanly");
}

/// `AwaitUser` holds: before the user line, an interrupt is answered and nothing else is
/// written; after the turn, the next interrupt, written by the harness's own `Requests`, is
/// answered with a reply `Requests::settle` accepts, again before the next turn starts.
#[tokio::test]
async fn await_user_holds_until_a_user_line_and_interrupts_are_answered() {
    let mut stub = Stub::start(&script("brain-two-turns.ndjson"));
    let early =
        r#"{"type":"control_request","request_id":"early_1","request":{"subtype":"interrupt"}}"#;
    stub.send(&format!("{early}\n")).await;
    let reply = stub.next().await.unwrap();
    let Line::Control(Message::Response { request_id, error }) = parsed(&reply) else {
        panic!("the first line is the interrupt's reply, so nothing was played: {reply}");
    };
    assert_eq!((request_id.as_str(), error), ("early_1", None), "a success for that request");

    stub.send(USER_LINE).await;
    let Line::Init(init) = parsed(&stub.next().await.unwrap()) else {
        panic!("the user line starts the turn with system/init");
    };
    loop {
        let line = stub.next().await.unwrap();
        if matches!(parsed(&line), Line::Result(_)) {
            break;
        }
    }

    let mut requests = Requests::default();
    let interrupt = requests.interrupt(&init).unwrap();
    stub.send(&format!("{interrupt}\n")).await;
    let reply = stub.next().await.unwrap();
    let Line::Control(Message::Response { request_id, error }) = parsed(&reply) else {
        panic!("the stub still holds before the second turn: {reply}");
    };
    let settled = requests.settle(&request_id, error).unwrap();
    assert_eq!(settled.request_id, "eileen_1", "the reply settles the harness's interrupt");
    assert_eq!(settled.error, None, "as a success");

    stub.send(USER_LINE).await;
    assert!(matches!(parsed(&stub.next().await.unwrap()), Line::Init(_)), "the second turn");
    stub.close();
    let rest = stub.rest().await;
    assert!(matches!(rest.last().map(|line| parsed(line)), Some(Line::Result(_))), "it ends");
    assert_eq!(stub.exit_code().await, Some(0), "it exits cleanly");
}

/// `ClaudeWorker` starts the stub with the claude CLI's own argv, so the script can come from
/// the environment instead.
#[tokio::test]
async fn the_script_comes_from_the_environment_under_claudes_argv() {
    let spec = ChildSpec::new("eileen-stub-harness", env!("CARGO_BIN_EXE_eileen-stub-harness"))
        .args(CLAUDE_ARGV.iter().copied())
        .envs([(SCRIPT_ENV, script("worker-hello.ndjson"))]);
    let mut stub = Stub::from_spec(&spec);
    stub.send(USER_LINE).await;
    stub.close();
    let lines = stub.rest().await;
    assert_eq!(worker(&lines).first().map(String::as_str), Some(SESSION), "it played the script");
    assert_eq!(stub.exit_code().await, Some(0), "it exits cleanly");
}

/// Starts the stub on `script` with stderr handed over, writes a user line when `user` (a stub
/// that fails before it reads stdin may have closed it already), and gives its stdout, stderr
/// and exit code once it ends.
async fn failing(script: &Path, user: bool) -> (Vec<String>, String, Option<i32>) {
    let spec = ChildSpec {
        raw_stderr: true,
        ..ChildSpec::new("eileen-stub-harness", env!("CARGO_BIN_EXE_eileen-stub-harness"))
            .args([SCRIPT_ARG.into(), script.as_os_str().to_owned()])
    };
    let mut stub = Stub::from_spec(&spec);
    let mut stderr = stub.child.take_stderr().unwrap();
    if user {
        stub.send(USER_LINE).await;
    }
    stub.close();
    let lines = stub.rest().await;
    let mut text = String::new();
    timeout(HANG, stderr.read_to_string(&mut text)).await.expect("stderr stayed open").unwrap();
    (lines, text, stub.exit_code().await)
}

#[tokio::test]
async fn a_script_that_does_not_parse_is_an_error_before_anything_is_played() {
    let (lines, stderr, code) = failing(&fixture("not-a-script.ndjson"), false).await;
    assert!(lines.is_empty(), "nothing was played: {lines:?}");
    assert!(stderr.contains("line 2 is not a StubScriptLine"), "the line is named: {stderr}");
    assert_eq!(code, Some(1), "a failure");
}

#[tokio::test]
async fn a_permission_request_ends_the_run_and_is_never_allowed() {
    let (lines, stderr, code) = failing(&fixture("permission-request.ndjson"), true).await;
    let events = worker(&lines);
    assert_eq!(events, ["narration Starting"], "the lines before it, and nothing after");
    assert!(stderr.contains("line 3 is a PermissionRequest"), "the line is named: {stderr}");
    assert_eq!(code, Some(1), "a failure, not a played call");
}

#[tokio::test]
async fn no_script_is_an_error() {
    let spec = ChildSpec {
        raw_stderr: true,
        ..ChildSpec::new("eileen-stub-harness", env!("CARGO_BIN_EXE_eileen-stub-harness"))
            .args(CLAUDE_ARGV.iter().copied())
    };
    let mut stub = Stub::from_spec(&spec);
    let mut stderr = stub.child.take_stderr().unwrap();
    stub.close();
    let mut text = String::new();
    timeout(HANG, stderr.read_to_string(&mut text)).await.expect("stderr stayed open").unwrap();
    assert!(text.contains(SCRIPT_ENV), "it says how to name a script: {text}");
    assert_eq!(stub.exit_code().await, Some(1), "a failure");
}

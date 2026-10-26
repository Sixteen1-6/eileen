//! The shim binary started the way the runtime starts children, through the supervisor, and
//! spoken to on its stdio as the claude CLI speaks to it.

use std::time::Duration;

use eileen_stdx::clock::{Clock as _, SystemClock};
use eileen_supervisor::{ChildSpec, spawn};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::time::timeout;

const INITIALIZE: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","#,
    r#""capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
    "\n",
);

const TOOLS_LIST: &str = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";

const PING: &str = "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ping\"}\n";

/// How long a reply may take before the test calls the shim hung: a guard against a hang, far
/// above anything a working shim needs, never a measurement.
const HANG: Duration = Duration::from_secs(20);

/// Writes one request line and reads one reply line.
async fn ask(
    stdin: &mut ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    line: &str,
) -> Value {
    stdin.write_all(line.as_bytes()).await.unwrap();
    let reply = timeout(HANG, lines.next_line()).await.expect("the shim hung").unwrap().unwrap();
    serde_json::from_str(&reply).unwrap()
}

#[tokio::test]
async fn the_binary_answers_initialize_and_lists_no_tools() {
    let spec = ChildSpec::new("eileen-shim", env!("CARGO_BIN_EXE_eileen-shim"));
    let mut child = spawn(&spec).unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let mut lines = BufReader::new(child.take_stdout().unwrap()).lines();

    let hello = ask(&mut stdin, &mut lines, INITIALIZE).await;
    assert_eq!(hello["id"], 1, "the request's id");
    assert_eq!(hello["result"]["serverInfo"]["name"], "eileen", "the server name");

    let tools = ask(&mut stdin, &mut lines, TOOLS_LIST).await;
    assert_eq!(tools["result"]["tools"], json!([]), "no tools");

    drop(stdin);
    let status = timeout(HANG, child.wait()).await.expect("the shim did not exit").unwrap();
    assert!(status.success(), "it exits cleanly when its stdin closes");
}

/// WP0 acceptance 3: the stub shim answers `initialize` within 100 ms. A `ping` first shows the
/// process is up, so the timed round trip is the answer, not the process start.
#[tokio::test]
async fn the_binary_answers_initialize_within_100_ms() {
    let spec = ChildSpec::new("eileen-shim", env!("CARGO_BIN_EXE_eileen-shim"));
    let mut child = spawn(&spec).unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let mut lines = BufReader::new(child.take_stdout().unwrap()).lines();

    let pong = ask(&mut stdin, &mut lines, PING).await;
    assert_eq!(pong["id"], 0, "the shim is up");

    let clock = SystemClock::new();
    let start = clock.mono();
    let hello = ask(&mut stdin, &mut lines, INITIALIZE).await;
    let elapsed_ms = clock.mono().0.saturating_sub(start.0) / 1_000_000;
    assert_eq!(hello["id"], 1, "the answer to initialize");
    assert!(elapsed_ms < 100, "initialize answered in {elapsed_ms} ms");

    drop(stdin);
    let status = timeout(HANG, child.wait()).await.expect("the shim did not exit").unwrap();
    assert!(status.success(), "it exits cleanly when its stdin closes");
}

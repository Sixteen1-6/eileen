//! The shim binary started the way the runtime starts children, through the supervisor, and
//! spoken to on its stdio as the claude CLI speaks to it.

use eileen_supervisor::{ChildSpec, spawn};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

const INITIALIZE: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","#,
    r#""capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
    "\n",
);

const TOOLS_LIST: &str = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";

#[tokio::test]
async fn the_binary_answers_initialize_and_lists_no_tools() {
    let spec = ChildSpec::new("eileen-shim", env!("CARGO_BIN_EXE_eileen-shim"));
    let mut child = spawn(&spec).unwrap();
    let mut stdin = child.take_stdin().unwrap();
    let mut lines = BufReader::new(child.take_stdout().unwrap()).lines();

    stdin.write_all(INITIALIZE.as_bytes()).await.unwrap();
    let hello: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(hello["id"], 1, "the request's id");
    assert_eq!(hello["result"]["serverInfo"]["name"], "eileen", "the server name");

    stdin.write_all(TOOLS_LIST.as_bytes()).await.unwrap();
    let tools: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(tools["result"]["tools"], json!([]), "no tools");

    drop(stdin);
    assert!(child.wait().await.unwrap().success(), "it exits cleanly when its stdin closes");
}

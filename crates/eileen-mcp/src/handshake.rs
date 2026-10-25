//! The handshake the stub shim answers (BUILD.md section 3.3.9): the claude CLI's
//! `server/discover` probe gets method-not-found, `initialize` gets
//! [`PROTOCOL_VERSION`] with the tools capability, `tools/list` gets no tools and `ping` an
//! empty result. Notifications get no answer. WP2 replaces this with rmcp's server, which
//! forwards calls to the core's dispatcher.

use serde_json::{Value, json};

/// The MCP server name Claude sees, as `mcp__eileen__<tool>`.
pub const SERVER_NAME: &str = "eileen";

/// The protocol version the shim speaks: the CLI falls back to `initialize` with it when its
/// `server/discover` probe fails with method-not-found.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// JSON-RPC's code for a line that is not JSON.
const PARSE_ERROR: i64 = -32_700;
/// JSON-RPC's code for JSON that is not a request.
const INVALID_REQUEST: i64 = -32_600;
/// JSON-RPC's code for a method the server does not have.
const METHOD_NOT_FOUND: i64 = -32_601;

/// The answer to one line from the client, as one line of JSON, or `None` when the line needs
/// no answer (a notification, or a response to a request the shim never sends).
#[must_use]
pub fn answer(line: &str) -> Option<String> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(failure(&Value::Null, PARSE_ERROR, "parse error"));
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        if message.get("result").is_some() || message.get("error").is_some() {
            return None;
        }
        let id = message.get("id").unwrap_or(&Value::Null);
        return Some(failure(id, INVALID_REQUEST, "invalid request"));
    };
    let id = message.get("id")?;
    Some(match method {
        "initialize" => success(
            id,
            &json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            }),
        ),
        "tools/list" => success(id, &json!({ "tools": [] })),
        "ping" => success(id, &json!({})),
        _ => failure(id, METHOD_NOT_FOUND, "method not found"),
    })
}

fn success(id: &Value, result: &Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn failure(id: &Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{PROTOCOL_VERSION, SERVER_NAME, answer};

    fn reply(line: &str) -> Value {
        serde_json::from_str(&answer(line).expect("an answer")).expect("JSON")
    }

    #[test]
    fn initialize_names_the_server_and_the_protocol_version() {
        let reply = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
        );
        assert_eq!(reply["id"], 1, "the request's id");
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION, "the version");
        assert_eq!(reply["result"]["serverInfo"]["name"], SERVER_NAME, "the server name");
        assert!(reply["result"]["capabilities"]["tools"].is_object(), "the tools capability");
    }

    #[test]
    fn the_tool_list_is_empty() {
        let reply = reply(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#);
        assert_eq!(reply["id"], "a", "the request's id");
        assert_eq!(reply["result"]["tools"], serde_json::json!([]), "no tools");
    }

    #[test]
    fn the_discover_probe_gets_method_not_found() {
        let reply = reply(r#"{"jsonrpc":"2.0","id":0,"method":"server/discover"}"#);
        assert_eq!(reply["error"]["code"], -32_601, "method not found");
        assert!(reply.get("result").is_none(), "no result");
    }

    #[test]
    fn notifications_and_responses_get_no_answer() {
        let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert_eq!(answer(initialized), None, "a notification");
        assert_eq!(answer(r#"{"jsonrpc":"2.0","id":3,"result":{}}"#), None, "a response");
    }

    #[test]
    fn a_line_that_is_not_json_gets_a_parse_error() {
        let reply = reply("{not json");
        assert_eq!(reply["error"]["code"], -32_700, "parse error");
        assert!(reply["id"].is_null(), "no id to answer");
    }
}

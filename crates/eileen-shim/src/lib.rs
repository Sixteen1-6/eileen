//! The MCP server the claude CLI starts as `eileen` (BUILD.md section 3.3.9): one shim per
//! session, which says hello on the dispatcher's pipe with its role and token and forwards every
//! call, so Claude sees `mcp__eileen__<tool>`. A lib and the `eileen-shim` binary. A stub until
//! WP2 builds it: it answers the handshake and an empty `tools/list` ([`serve`]).
//!
//! # Owns
//!
//! - The stdio side of the protocol and the pipe side of `ShimLine` and `DispatcherLine`.
//! - The `--notify <kind>` mode that posts to the loopback endpoint and always exits 0, and the
//!   session role for the owner's own Claude sessions.
//! - [`serve`], the stub's loop over stdin.
//!
//! # Does not own
//!
//! - MCP itself and the handshake's answers (`eileen-mcp`), the dispatcher and caller
//!   verification (`eileen-runtime`), or the tools (the core and the sidecars).
//!
//! # Invariants
//!
//! - The stub binary answers `initialize` within 100 ms once it is up and lists no tools
//!   (integration tests `it::process::the_binary_answers_initialize_within_100_ms` and
//!   `it::process::the_binary_answers_initialize_and_lists_no_tools`).
//! - A shim started inside a worker's Job Object with the session token is refused (WP2
//!   acceptance 4); the dispatcher decides, not the shim.
//!
//! # Boundaries
//!
//! - Started by the claude CLI from the generated `--mcp-config`, and by the plugin's
//!   `.mcp.json` with `--role session`.
//! - May depend on `eileen-mcp`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`; its process test starts it through `eileen-supervisor`, a dev-dependency.
//! - Must not depend on `eileen-core` or `eileen-runtime`, which it reaches only over the
//!   dispatcher's pipe, on `eileen-supervisor` outside its tests, or on rmcp except through
//!   `eileen-mcp`, nor know vendor CLI flags (section 3.1); nor on a `-win` or `-linux` crate
//!   (check 15).
//! - Wire types: MCP's JSON-RPC on stdio; `tools::ShimLine` and `DispatcherLine` on the pipe.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-shim` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-shim`).
//! - Layers: a unit test of the loop, and the binary end to end through the supervisor.
//! - A bug here starts as a session whose `system/init` lists `eileen` as failed.
//!
//! # Traps
//!
//! - The CLI reads only stdout for the protocol; anything else written there breaks the session.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 logs the `_meta` the CLI sends on its first call.
//!
//! # Do not change without a decision record
//!
//! - The hello's fields and the token rules (section 2.12).

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};

use eileen_mcp::handshake;

/// Answers each line the client writes on `input` with one line on `output`, until `input`
/// ends. Blank lines are skipped, and a line that is not UTF-8 is read lossily (bad bytes
/// become U+FFFD), so it is still answered, with a parse error when it is then not JSON,
/// instead of ending the session.
pub fn serve(mut input: impl BufRead, output: &mut impl Write) -> io::Result<()> {
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if input.read_until(b'\n', &mut raw)? == 0 {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&raw);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handshake::answer(line.trim_end()) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::serve;

    #[test]
    fn initialize_gets_one_answer_line() {
        let request = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"#,
            r#""2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
            "\n",
        );
        let mut output = Vec::new();
        serve(request.as_bytes(), &mut output).unwrap();
        assert_eq!(String::from_utf8(output.clone()).unwrap().lines().count(), 1, "one line");
        let reply: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(reply["id"], 1, "the request's id");
        assert_eq!(reply["result"]["protocolVersion"], "2025-11-25", "the protocol version");
    }

    #[test]
    fn notifications_and_blank_lines_get_no_answer() {
        let input = "\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\n";
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output).unwrap();
        assert!(output.is_empty(), "nothing written");
    }

    #[test]
    fn a_line_that_is_not_utf8_gets_a_parse_error_and_the_session_goes_on() {
        let mut input = b"{\"id\":1,\"method\":\xff}\n".to_vec();
        input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");
        let mut output = Vec::new();
        serve(input.as_slice(), &mut output).unwrap();
        let replies: Vec<serde_json::Value> = output
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 2, "both lines answered");
        assert_eq!(replies[0]["error"]["code"], -32_700, "a parse error for the bad line");
        assert_eq!(replies[1]["id"], 2, "the next request still answered");
    }
}

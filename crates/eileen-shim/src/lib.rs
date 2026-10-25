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
//! - The stub answers `initialize` within 100 ms (unit test
//!   `tests::initialize_is_answered_within_100_ms`) and lists no tools (integration test
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
/// ends. Blank lines are skipped.
pub fn serve(input: impl BufRead, output: &mut impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handshake::answer(&line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use eileen_stdx::clock::{Clock as _, SystemClock};

    use super::serve;

    #[test]
    fn initialize_is_answered_within_100_ms() {
        let request = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"#,
            r#""2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.304"}}}"#,
            "\n",
        );
        let clock = SystemClock::new();
        let mut output = Vec::new();
        let start = clock.mono();
        serve(request.as_bytes(), &mut output).unwrap();
        let elapsed_ms = clock.mono().0.saturating_sub(start.0) / 1_000_000;
        assert!(elapsed_ms < 100, "answered in {elapsed_ms} ms");
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
}

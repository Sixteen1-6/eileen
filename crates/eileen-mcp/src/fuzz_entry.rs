//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `mcp_client_message` (JSON-RPC from sidecars), `mcp_tool_arguments` (tool arguments from
//! workers) and `dispatch_line` (the shim's pipe lines). Stubs until WP2 builds the parsers; each
//! then returns `Err` naming the invariant an input broke, and its seed corpus replays as a test.

/// One JSON-RPC message from a sidecar. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's client, whose checks can fail"
)]
pub fn mcp_client_message(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

/// One tool call's arguments from a worker. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's dispatcher, whose checks can fail"
)]
pub fn mcp_tool_arguments(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

/// One line on the shim's pipe. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's pipe server, whose checks can fail"
)]
pub fn dispatch_line(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

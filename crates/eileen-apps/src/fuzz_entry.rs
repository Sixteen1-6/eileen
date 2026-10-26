//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7): `resolve_reply`
//! (Resolve bridge replies) and `registry_reply` (MCP registry replies for `routes_lookup`).
//! Stubs until WP6 builds the parsers; each then returns `Err` naming the invariant an input
//! broke, and its seed corpus replays as a test.

/// One reply from the Resolve bridge. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP6's bridge parser, whose checks can fail"
)]
pub fn resolve_reply(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

/// One reply from the MCP registry. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP6's registry parser, whose checks can fail"
)]
pub fn registry_reply(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

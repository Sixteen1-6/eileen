//! The entry point of the `command_grammar` fuzz target (BUILD.md section 4.7): transcripts into
//! the fast path. A stub until WP7 builds the grammar; it then returns `Err` naming the invariant
//! an input broke, and its seed corpus replays as a test.

/// One transcript. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP7's grammar, whose checks can fail"
)]
pub fn command_grammar(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

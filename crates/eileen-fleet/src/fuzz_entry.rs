//! The entry point of the `fleet_result` fuzz target (BUILD.md section 4.7): fleet result lines
//! and fleet issue comments. A stub until WP9 builds the parser; it then returns `Err` naming the
//! invariant an input broke, and its seed corpus replays as a test.

/// One fleet result line or issue comment. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP9's parser, whose checks can fail"
)]
pub fn fleet_result(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

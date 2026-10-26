//! The entry point of the `recording` fuzz target (BUILD.md section 4.7): recording input lines
//! into the reader. A stub until WP1 builds the reader; it then returns `Err` naming the invariant
//! an input broke, and its seed corpus replays as a test.

/// One recording input line. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP1's reader, whose checks can fail"
)]
pub fn recording(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

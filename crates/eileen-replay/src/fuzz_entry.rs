//! The entry point of the `expected_trace` fuzz target (BUILD.md section 4.7): expected-trace
//! files into the matcher's parser. A stub until WP1 builds the parser; it then returns `Err`
//! naming the invariant an input broke, and its seed corpus replays as a test.

/// One expected-trace file. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP1's parser, whose checks can fail"
)]
pub fn expected_trace(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

//! The entry point of the `scenario_file` fuzz target (BUILD.md section 4.7): contributed
//! scenarios and their `inputs.jsonl` lines. A stub until WP2 builds the reader; it then returns
//! `Err` naming the invariant an input broke, and its seed corpus replays as a test.

/// One scenario file or input line. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's reader, whose checks can fail"
)]
pub fn scenario_file(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

//! The entry point of the `dictation_cleanup` fuzz target (BUILD.md section 4.7): transcripts
//! into the cleaner. A stub until WP4 builds the cleaner; it then returns `Err` naming the
//! invariant an input broke, and its seed corpus replays as a test.

/// One raw transcript. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP4's cleaner, whose checks can fail"
)]
pub fn dictation_cleanup(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

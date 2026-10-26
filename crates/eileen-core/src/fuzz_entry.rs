//! The entry point of the `shell_command` fuzz target (BUILD.md section 4.7): worker shell
//! commands into the permission gate's tokenizer and path extraction. A stub until WP1 builds the
//! tokenizer; it then returns `Err` naming the invariant an input broke, and its seed corpus
//! replays as a test.

/// A worker's shell command line. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP1's tokenizer, whose checks can fail"
)]
pub fn shell_command(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

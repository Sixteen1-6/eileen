//! The entry point of the `speech_frame` fuzz target (BUILD.md section 4.7): frames on the speech
//! pipe, whose length is checked against the kind's maximum before any allocation. A stub until
//! WP4 builds the pipe; it then returns `Err` naming the invariant an input broke, and its seed
//! corpus replays as a test.

/// One frame from the speech pipe. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP4's pipe parser, whose checks can fail"
)]
pub fn speech_frame(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

//! The entry point of the `skill_record` fuzz target (BUILD.md section 4.7): learned skill files,
//! their front matter and `recipe.json`. A stub until WP7 builds the reader and the linter; it
//! then returns `Err` naming the invariant an input broke, and its seed corpus replays as a test.

/// One learned skill file. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP7's reader, whose checks can fail"
)]
pub fn skill_record(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

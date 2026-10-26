//! The entry point of the `hud_line` fuzz target (BUILD.md section 4.7): HUD protocol lines. A
//! stub until WP8 builds the protocol; it then returns `Err` naming the invariant an input broke,
//! and its seed corpus replays as a test.

/// One HUD protocol line. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP8's protocol reader, whose checks can fail"
)]
pub fn hud_line(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

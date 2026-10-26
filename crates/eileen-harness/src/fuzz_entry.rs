//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `stream_json_line` and `control_message`, lines from `claude -p`. Stubs until WP3 builds the
//! parser; each then returns `Err` naming the invariant an input broke, and its seed corpus
//! replays as a test.

/// One stream-json line from `claude -p`. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP3's parser, whose checks can fail"
)]
pub fn stream_json_line(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

/// One control message from `claude -p`. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP3's parser, whose checks can fail"
)]
pub fn control_message(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

//! The entry point of the `config_toml` fuzz target (BUILD.md section 4.7): the user's
//! configuration files. A stub until WP7 builds the loader; it then returns `Err` naming the
//! invariant an input broke, and its seed corpus replays as a test.

/// One configuration file. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP7's loader, whose checks can fail"
)]
pub fn config_toml(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

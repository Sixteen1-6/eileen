//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `worker_hook_payload` (hook bodies) and `http_request` (the loopback HTTP parser, which fails
//! closed). Stubs until WP2 builds the parsers; each then returns `Err` naming the invariant an
//! input broke, and its seed corpus replays as a test.

/// One hook body from a worker. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's hook endpoint, whose checks can fail"
)]
pub fn worker_hook_payload(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

/// One request to the loopback endpoint. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until WP2's HTTP parser, whose checks can fail"
)]
pub fn http_request(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

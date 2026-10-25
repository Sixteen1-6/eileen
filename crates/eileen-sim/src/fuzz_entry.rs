//! The entry point of the `sim` fuzz target (BUILD.md section 4.7): simulator runs from bytes,
//! checked by every invariant. A stub until `wp0/xtask` builds the simulator's skeleton; it then
//! returns `Err` naming the invariant a run broke.

/// One simulator run from bytes. The stub accepts every input.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until the simulator's skeleton, whose invariants can fail"
)]
pub fn sim(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
}

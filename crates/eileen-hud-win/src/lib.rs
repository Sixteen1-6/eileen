//! Eileen's HUD renderer on Windows (BUILD.md sections 2.9 and 3.4): `eileen-platform`'s
//! `HudSurface` as the overlay and the strip, with Raw Input on the strip so only physical clicks
//! count, `WDA_EXCLUDEFROMCAPTURE`, per-monitor DPI, the DWM live thumbnail and `WM_DROPFILES`
//! for voice clips. A stub until WP8 builds it, after its renderer probe: every call answers
//! `PlatformError::Unavailable`.
//!
//! # Owns
//!
//! - [`WinHudSurface`], the Windows implementation of `HudSurface`.
//! - The renderer WP8's probe picks (`DirectWrite` into `Direct2D` on a `DirectComposition`
//!   surface, or the colour-keyed GDI fallback).
//!
//! # Does not own
//!
//! - The layout and the HUD protocol (`eileen-hud`), what the HUD shows (`eileen-core`), or the
//!   trait (`eileen-platform`).
//!
//! # Invariants
//!
//! - Windows only: the crate is `cfg(windows)` and compiles to an empty library elsewhere.
//! - Unsafe code only in a private `ffi` module, one operation per block (check 10).
//! - The Raw Input module forwards only a "came from a device" flag, never a key code or a
//!   pointer position (check 25).
//!
//! # Boundaries
//!
//! - Called by `eileen-hud` (under its Windows target table), on the HUD's own thread.
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-hud-win` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-hud-win`).
//! - Layers: desktop tests of the overlay in the `desktop` group, and idle cost through
//!   `cargo xtask measure-idle`.
//! - A bug here starts as the HUD showing in a screen share, or a click that was not physical
//!   being taken.
//!
//! # Traps
//!
//! - `HudSurface` is not `Send`: the surface lives on the HUD's thread.
//!
//! # Logs and tracing
//!
//! - None yet; WP8 names its events from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - Capture exclusion and the physical-click rule.

#![cfg(windows)]

use eileen_contracts::desktop::Rect;
use eileen_contracts::hud::HudInput;
use eileen_platform::{DrawList, HudSurface, PlatformError};

/// What every stub call answers.
const NOT_BUILT: PlatformError = PlatformError::Unavailable { what: "the HUD overlay" };

/// The overlay and the strip. A stub.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinHudSurface;

impl HudSurface for WinHudSurface {
    fn draw(&mut self, _list: &DrawList) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn set_watch(&mut self, _hwnd: Option<i64>, _rect: Rect) -> Result<(), PlatformError> {
        Err(NOT_BUILT)
    }

    fn next_input(&mut self) -> Option<(HudInput, bool)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use eileen_platform::{DrawList, HudSurface, PlatformError};

    use super::WinHudSurface;

    #[test]
    fn the_stub_surface_draws_nothing_and_has_no_input() {
        let mut surface = WinHudSurface;
        let unavailable = PlatformError::Unavailable { what: "the HUD overlay" };
        assert_eq!(surface.draw(&DrawList::default()), Err(unavailable), "nothing drawn");
        assert!(surface.next_input().is_none(), "no input");
    }
}

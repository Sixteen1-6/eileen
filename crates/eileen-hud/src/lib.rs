//! Eileen's on-screen layer (BUILD.md sections 2.9 and 3.3.6): the platform-neutral layout from
//! `HudState` to a draw list (the mic, the conversation ring, the heard line, jobs, approvals,
//! panels, suggestions, the first run, the machine panel), the HUD protocol, and the
//! `eileen-hud` process that drives `eileen-hud-win`. A lib and a binary. A stub until WP8 builds
//! it; the stub binary says it is not built yet.
//!
//! # Owns
//!
//! - The layout, with insta snapshots of the draw lists, corners, scale, opacity and high
//!   contrast.
//! - The HUD protocol with the `physical` flag on every input, and the process's command line
//!   ([`cli`]).
//! - The `hud_line` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The overlay window, Raw Input and DWM thumbnails (`eileen-hud-win`), what the HUD shows
//!   (`eileen-core` assembles `HudState`), or the settings flow its controls feed.
//!
//! # Invariants
//!
//! - Only physical clicks count: every `HudInput` carries the `physical` flag from Raw Input
//!   (WP8 acceptance 2: a synthetic click on Allow is ignored).
//! - Every clickable `HudInput` maps to a grammar entry or a hotkey (the accessibility test,
//!   WP8 acceptance 9).
//! - The window is excluded from capture (WP8 acceptance 2: a `desktop_screenshot` lacks it).
//!
//! # Boundaries
//!
//! - Started by `eileen-runtime` through the supervisor; fed over the HUD pipe.
//! - May depend on `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`;
//!   `eileen-hud-win` under the Windows target table.
//! - Must not depend on `eileen-core` or `eileen-runtime` (it is fed over the pipe) or rmcp, nor
//!   know vendor CLI flags (section 3.1); nor on any other `-win` or `-linux` crate, or on
//!   `eileen-hud-win` outside the Windows target table (check 15).
//! - Wire types: `HudStateLine` and `HudInputLine` (`eileen-contracts::hud`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-hud` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-hud`).
//! - Layers: draw-list snapshots on Linux, the accessibility test, the `hud_line` fuzz target,
//!   and desktop tests of the overlay on the owner's PC.
//! - A bug here starts as a draw-list snapshot diff nobody meant.
//!
//! # Traps
//!
//! - The HUD hides in presentation and full-screen states; a test of it must set that state.
//!
//! # Logs and tracing
//!
//! - None yet; WP8 declares its events in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The HUD protocol's lines and the `physical` flag.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

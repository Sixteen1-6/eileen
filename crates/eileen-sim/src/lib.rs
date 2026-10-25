//! The deterministic simulator (BUILD.md section 4.7): a `World` of simulated parts around the
//! real core, inputs generated from an `arbitrary::Unstructured` buffer, faults through
//! `buggify!`, and the sixteen invariants checked after every step. Development only. A stub
//! until WP0's `wp0/xtask` pull request builds its skeleton; WP1 owns it from then on.
//!
//! # Owns
//!
//! - The `World`, the TOML profiles, seeded runs, the fixed-column step log, `--doublecheck`,
//!   minimised `SimRegression` files in `crates/eileen-sim/regressions/`, and the invariants.
//! - The `sim` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The core's decisions (`eileen-core`), the recording formats (`eileen-record`), replay
//!   (`eileen-replay`), or the `cargo xtask sim` command line (`xtask`).
//!
//! # Invariants
//!
//! - One seed gives one trace on every OS: `--doublecheck` runs a seed twice and compares the
//!   trace hashes.
//! - The simulator thread runs on an explicit large stack.
//! - Never linked into a shipped binary (development only).
//!
//! # Boundaries
//!
//! - Called by `xtask` (`cargo xtask sim`) and the fuzz target `sim`.
//! - May depend on `eileen-core`, `eileen-record`, `eileen-replay`, `eileen-intent`,
//!   `eileen-dictation`, `eileen-skills`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: `SimRegression` (a contract type).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask sim` (the pull request seed set).
//! - Layers: one test per pull request seed, the regressions, exhaustive turn-taking.
//! - A bug here starts as a seed that fails only on one OS.
//!
//! # Traps
//!
//! - A fault site is named where it is written (`buggify!("store.write")`), never by file and
//!   line, so a `SimRegression` written on one OS replays on the other.
//!
//! # Logs and tracing
//!
//! - None yet: the step log is the simulator's own output, not tracing.
//!
//! # Do not change without a decision record
//!
//! - The invariants' numbering (section 4.7), which regressions and acceptance lines cite.

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;

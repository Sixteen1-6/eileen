//! Scenarios from real use (BUILD.md sections 3.3.14, 3.3.16 and 4.8): saving one from the
//! flight recorder, the automatic local scenarios, `check`, `bless`, `stale`, sharing with
//! generalisation and redaction, submitting through the contributor's own `gh`, and the
//! real-desktop runner. Shipped, so contributors can share from the first slice. A stub until WP2
//! builds it.
//!
//! # Owns
//!
//! - `eileen scenario` ([`cli`]), every form but `replay`, which it passes to `eileen-replay`.
//! - The real-desktop runner: fixtures, steps through the test endpoint, transcript injection
//!   or `WavSource`, expectations through the replay matcher, `ci_factor` timing.
//! - The `scenario_file` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The scenario format and the expiry rule (`eileen-contracts::scenario`), the privacy rules
//!   (`eileen_trace::privacy`), replay (`eileen-replay`), or the fleet (`eileen-fleet`).
//!
//! # Invariants
//!
//! - `check` applies exactly the rules CI's check 18 applies, through `eileen_trace::privacy`
//!   and `eileen_contracts::scenario::status_problems`.
//! - `bless` rewrites an expected trace only when every expectation still passes.
//! - A shared scenario holds only placeholders in its personal fields.
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`, `eileen-fleet` and `eileen-e2e`.
//! - May depend on `eileen-record`, `eileen-replay`, `eileen-supervisor`, `eileen-rt`,
//!   `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: scenario folders, in the contract format.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-scenarios` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-scenarios`).
//! - Layers: save, share and submit round trips on fixtures, the `scenario_file` fuzz target,
//!   and the end-to-end smoke on `windows-latest`.
//! - A bug here starts as a shared scenario carrying a personal path, or a save that does not
//!   replay.
//!
//! # Traps
//!
//! - The machine hash comes from `fleet\machine.toml` in the user's own data directory, never a
//!   throwaway one.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 declares its events in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - What `share` generalises and redacts, and the `stale` and quarantine caps.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

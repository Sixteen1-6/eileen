//! Eileen's test fleet runner (BUILD.md section 4.8): `eileen test --fleet` with one-time
//! consent, scenarios from the installed version's tag in recorded or live mode, attended and
//! armed runs, `FleetResult` lines with anonymous learning statistics, and submitting them to the
//! `devhub` branch. Shipped. A stub until WP9 builds it.
//!
//! # Owns
//!
//! - The fleet runner and its safety rules: a throwaway data directory, owned fixtures, call and
//!   full-screen refusal, abort on physical input, tier 2 auto-denied with voice approval off.
//! - `eileen test --fleet` ([`cli`]) and the `fleet_result` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Replaying a scenario (`eileen-replay`), the scenarios (`eileen-scenarios`), the result
//!   format (`eileen-contracts::fleet`), or result validation and ingest (`eileen-e2e`).
//!
//! # Invariants
//!
//! - The machine hash comes from `fleet\machine.toml` in the user's own data directory, never a
//!   run's throwaway one.
//! - An armed run starts only after 30 idle minutes, and the Task Scheduler entry only after an
//!   explicit yes.
//! - The stub runs nothing: `eileen test --fleet` says it is not built yet.
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` and `eileen-e2e`.
//! - May depend on `eileen-scenarios`, `eileen-replay`, `eileen-record`, `eileen-supervisor`,
//!   `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: `FleetResult` lines (`eileen-contracts::fleet`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-fleet` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-fleet`).
//! - Layers: unit tests for consent, scheduling and the result lines, and the `fleet_result`
//!   fuzz target.
//! - A bug here starts as a result line with a personal value, or a run that touched a window it
//!   did not create.
//!
//! # Traps
//!
//! - `--submit` opens the pull request from the contributor's own account through their `gh`;
//!   the runner holds no token.
//!
//! # Logs and tracing
//!
//! - None yet; WP9 declares its events in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - What a result line may hold, and the armed run's safety rules.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

//! The deterministic simulator (BUILD.md section 4.7): a `World` of simulated parts around the
//! real core, inputs drawn from an `arbitrary::Unstructured` buffer, and invariants checked
//! after every step. Development only. WP0's `wp0/xtask` built this skeleton; WP1 owns it from
//! phase A's merge and grows it to every part, every fault and all sixteen invariants.
//!
//! # Owns
//!
//! - [`World`], the TOML [`Profile`]s with their schema, seeded and byte-driven runs, the
//!   fixed-column step log, `--doublecheck`, `SimRegression` replay and the invariants.
//! - The `eileen-sim` binary ([`cli`]) and the `sim` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The core's decisions (`eileen-core`), the recording formats (`eileen-record`), replay
//!   (`eileen-replay`), or the `cargo xtask sim` command line (`xtask`).
//!
//! # Invariants
//!
//! - One seed gives one trace on every OS (`--doublecheck`; test
//!   `run::tests::one_seed_gives_one_hash_and_two_seeds_two`).
//! - Every run uses the large stack of [`run::SIM_STACK_BYTES`] ([`run::on_sim_thread`]).
//! - Each invariant can fail: a hand-built violating trace per rule (`invariants::tests`).
//! - Never linked into a shipped binary (development only).
//!
//! # Boundaries
//!
//! - Called by `xtask` (`cargo xtask sim` runs the binary) and the fuzz target `sim`.
//! - May depend on `eileen-core`, `eileen-record`, `eileen-replay`, `eileen-intent`,
//!   `eileen-dictation`, `eileen-skills`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`; not on `eileen-config`, so it carries its own settings validator.
//! - Wire types: `SimRegression` and `InputRecord` (contract types).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask sim` (the pull request seed set, [`run::PR_SEEDS`]).
//! - Layers: unit tests per module, one test per pull request seed
//!   (`it::pr_seeds`), every stored regression (`it::regressions`).
//! - A bug here starts as a seed that fails only on one OS.
//!
//! # Traps
//!
//! - The core and the generator take separate random streams from the seed, so a replay,
//!   which mints no IDs, hands the core the numbers the run did.
//! - A fault site is named where it is written (`buggify!("store.write")`), never by file and
//!   line, so a `SimRegression` written on one OS replays on the other.
//! - Hash with [`hash::TraceHash`], never std's `DefaultHasher`, whose algorithm may change.
//!
//! # Logs and tracing
//!
//! - The step log is the simulator's own output. The core's spans and log lines go to a
//!   flight-recorder ring on the simulator's thread, which invariant 13 checks; replay a
//!   failure with the `replay:` command it prints.
//!
//! # Do not change without a decision record
//!
//! - The invariants' numbering (section 4.7), which regressions and acceptance lines cite.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod hash;
pub mod inputs;
pub mod invariants;
pub mod profile;
pub mod run;
pub mod world;

pub use profile::Profile;
pub use run::{RunReport, run_seed};
pub use world::World;

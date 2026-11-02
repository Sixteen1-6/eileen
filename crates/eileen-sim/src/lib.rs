//! The deterministic simulator (BUILD.md section 4.7): a `World` of simulated parts around the
//! real core, inputs drawn from an `arbitrary::Unstructured` buffer, and invariants checked
//! after every step. Development only. WP0's `wp0/xtask` built this skeleton; WP1 owns it from
//! phase A's merge and grows it to every part, every fault and all sixteen invariants.
//!
//! # Owns
//!
//! - [`World`], the TOML [`Profile`]s with their schema, seeded and byte-driven runs, the
//!   fixed-column step log, `--doublecheck`, `SimRegression` replay, the invariants, and the
//!   exhaustive enumeration of turn-taking over `eileen_core::turn::Turns` ([`exhaustive`]).
//! - The parts that answer the core ([`parts`]): the speech process with its life (a load,
//!   `Ready`, a heartbeat a second, and the hangs, crashes and loads that never finish a test
//!   injects), the playback device in 10 ms callbacks, a scripted brain ([`BrainReply`]) and
//!   scripted worker sessions; and
//!   [`SimResolver`], the scripted intent resolver beside its own settings validator.
//! - The `eileen-sim` binary ([`cli`]) and the `sim` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The core's decisions (`eileen-core`), the recording formats (`eileen-record`), replay
//!   (`eileen-replay`), or the `cargo xtask sim` command line (`xtask`).
//!
//! # Invariants
//!
//! - One seed gives one trace, run after run (`--doublecheck` on every pull request seed, WP0
//!   acceptance 7; test `run::tests::one_seed_gives_one_hash_and_two_seeds_two`). Pinning one
//!   trace across OSes waits for WP1's core, whose changes would move any pinned hash.
//! - Every run uses the large stack of [`run::SIM_STACK_BYTES`] ([`run::on_sim_thread`]).
//! - Each invariant can fail: a hand-built violating trace per rule (`invariants::tests`);
//!   WP1 acceptance 1 extends this to every invariant of section 4.7.
//!
//! # Boundaries
//!
//! - Called by `xtask` (`cargo xtask sim` runs the binary) and the fuzz target `sim`; no shipped
//!   crate depends on it (development only: section 3.1's table and `process = "tests"` in its
//!   metadata; no repository check enforces it yet).
//! - May depend on `eileen-core`, `eileen-record`, `eileen-replay`, `eileen-intent`,
//!   `eileen-dictation`, `eileen-skills`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`; not on `eileen-config`, so it carries its own settings validator, and
//!   until WP7's resolver it scripts its own (`eileen-intent` is not a dependency today).
//! - Must not depend on `eileen-runtime`, `eileen-harness` or `eileen-supervisor`, which its
//!   `World` simulates, or on rmcp, nor know vendor CLI flags (section 3.1); nor on a `-win` or
//!   `-linux` crate (check 15).
//! - Wire types: `SimRegression` and `InputRecord` (contract types).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask sim` (the pull request seed set, [`run::PR_SEEDS`]).
//! - Layers: unit tests per module, one test per pull request seed
//!   (`it::pr_seeds`), every stored regression (`it::regressions`), slice S1's flows
//!   through the minimal core, the speech process's life among them (`it::slice`), barge-in's
//!   timing and heard prefix (`it::barge_in`), and `it::turn::exhaustive` to depth 6, or to
//!   the depth in `EILEEN_EXHAUSTIVE_DEPTH` (8 nightly).
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
//! - The step log is the simulator's own output. The core's spans and log lines go to the
//!   span collector on the simulator's thread ([`collect`]), which invariant 13 checks after
//!   every step; replay a failure with the `replay:` command it prints.
//!
//! # Do not change without a decision record
//!
//! - The invariants' numbering (section 4.7), which regressions and acceptance lines cite.

#![forbid(unsafe_code)]

pub mod cli;
pub mod collect;
pub mod exhaustive;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod hash;
pub mod inputs;
pub mod invariants;
pub mod parts;
pub mod profile;
pub mod resolver;
pub mod run;
pub mod world;

pub use parts::BrainReply;
pub use profile::Profile;
pub use resolver::SimResolver;
pub use run::{RunReport, run_seed};
pub use world::World;

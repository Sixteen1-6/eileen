//! Eileen's end-to-end tools (BUILD.md sections 4.7 and 4.8): benchmarks B1 to B10 and
//! B8-curve with A/B runs against the base binary, the soak runner, fleet ingest and clustering,
//! and the result validation behind `cargo xtask fleet-validate`. Development only. A lib and the
//! `eileen-e2e` binary that `cargo xtask` dispatches to. A stub until WP9 builds it; the stub
//! binary says each command is not built yet.
//!
//! # Owns
//!
//! - The benchmarks and their A/B judgement (alternating runs, Mann-Whitney, strata), and the
//!   results history on the `devhub` branch.
//! - The soak runner, fleet ingest and triage, and result validation.
//! - The commands `cargo xtask` dispatches here: `bench`, `triage` and `fleet-validate`
//!   ([`cli`]).
//!
//! # Does not own
//!
//! - The fleet runner on contributors' PCs (`eileen-fleet`), scenarios (`eileen-scenarios`), or
//!   replay (`eileen-replay`).
//!
//! # Invariants
//!
//! - A benchmark run is excluded only for "start it anyway" or thrashing, with the reason
//!   recorded (WP9 adds the test).
//! - Result files and comments are untrusted data, parsed by the fuzzed `fleet_result` parser
//!   (WP9 acceptance 4 and 7).
//!
//! # Boundaries
//!
//! - Called by `cargo xtask` and the `devhub.yml` workflow; no shipped crate depends on it
//!   (development only: section 3.1's table and `process = "tests"` in its metadata; no
//!   repository check enforces it yet).
//! - May depend on `eileen-scenarios`, `eileen-fleet`, `eileen-replay`, `eileen-record`,
//!   `eileen-supervisor`, `eileen-mcp`, `eileen-config`, `eileen-rt`, `eileen-contracts`,
//!   `eileen-stdx` and `eileen-trace`.
//! - Must not depend on `eileen-runtime`, `eileen-cli` or a sidecar's crate, whose binaries it
//!   runs, or on rmcp except through `eileen-mcp`, nor know vendor CLI flags (section 3.1);
//!   nor on a `-win` or `-linux` crate outside a matching target table (check 15).
//! - Wire types: benchmark result lines and fleet results, in contract formats.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-e2e` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-e2e`).
//! - Layers: unit tests for the statistics and the clustering; the benchmarks themselves run on
//!   the owner's PC.
//! - A bug here starts as a benchmark difference reported that was noise.
//!
//! # Traps
//!
//! - Benchmarks never run during a build (section 6.1 rule 11).
//!
//! # Logs and tracing
//!
//! - None yet; WP9 reads the event-log spans the other crates write.
//!
//! # Do not change without a decision record
//!
//! - The A/B test and its 10 percent threshold.

#![forbid(unsafe_code)]

pub mod cli;

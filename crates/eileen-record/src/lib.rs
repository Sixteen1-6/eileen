//! Eileen's recordings (BUILD.md section 3.3.16): the recording formats on disk, input records
//! and hook answers as `RecordLine`s, the content-addressed blob store, the reader, the writer and
//! `trim`. A stub until WP1 builds it.
//!
//! # Owns
//!
//! - Writing and reading recordings and checkpoints in the formats of
//!   `eileen-contracts::record`, with long strings moved into blobs.
//! - The BLAKE3 and zstd blob store with reference counts (`eileen-record::blob`, the only
//!   module here that may use vendored C, on the runtime's blocking pool).
//! - `eileen record` ([`cli`]) and the `recording` fuzz target's entry ([`fuzz_entry`]).
//!
//! # Does not own
//!
//! - The formats themselves (`eileen-contracts::record`, `scenario`, `fleet`), replaying a
//!   recording (`eileen-replay`), or deciding what to record (`eileen-core`, the flight recorder
//!   wiring in `eileen-runtime`).
//!
//! # Invariants
//!
//! - `record::stub_blobs` and `record::resolve_blobs` are exact inverses, so a written line
//!   reads back to the `RecordLine` that was written (WP1, a property test).
//! - `trim` keeps every setup input and each kept hook input's answer.
//! - The stub writes nothing: every `eileen record` form says it is not built yet.
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime`, `eileen-replay`, `eileen-scenarios`, `eileen-fleet`,
//!   `eileen-learn` and `eileen-cli`.
//! - May depend on `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: the recording files, all of them contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-record` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-record`).
//! - Layers: round-trip property tests, the `recording` fuzz target and its corpus replay.
//! - A bug here starts as a recording that does not replay, or a blob nothing refers to.
//!
//! # Traps
//!
//! - A recording holds the user's words: it is `Personal` wherever it is shown or shared, and
//!   scenarios are made from it only through `eileen-scenarios`' redaction.
//!
//! # Logs and tracing
//!
//! - None yet; WP1 names its spans in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The blob reference form (`{"$blob": "b3:..."}`) and the `{"$lit": ...}` escape.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

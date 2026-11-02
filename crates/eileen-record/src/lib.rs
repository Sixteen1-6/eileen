//! Eileen's recordings (BUILD.md section 3.3.16): the recording formats on disk, input records
//! and hook answers as `RecordLine`s ([`line`](mod@line)), the content-addressed blob store
//! ([`blob`]), the [`reader`], the [`writer`] and [`trim`]. `eileen record` itself is still a
//! stub.
//!
//! # Owns
//!
//! - Writing, reading and trimming recordings in the formats of `eileen-contracts::record`,
//!   one `RecordLine` a line with long strings in blobs, and which inputs count as setup.
//! - The BLAKE3 and zstd blob store with reference counts ([`blob`], the only module here that
//!   may use vendored C, on the runtime's blocking pool).
//! - `eileen record` ([`cli`]) and the `recording` fuzz target's entry (`fuzz_entry`).
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
//!   reads back to the `RecordLine` that was written, through the disk store as through the
//!   memory one (`it::round_trip::*`, property tests).
//! - `trim` keeps every input of the chosen turns and jobs, the latest setup input of each kind
//!   before the first of them, the setup inputs no other turn or job claims between the first
//!   and the last, and each kept hook input's answer, in their order, and nothing else
//!   (`it::trim::*`, and the `recording` fuzz target, which works the kept lines out on its own).
//! - A blob is named by the BLAKE3 hash of its bytes, counts each reference a writer added, is
//!   deleted with its last one, and reads as corrupt if it does not hash to its name
//!   (`blob::tests::*`, `it::blobs::*`).
//! - A count is never too low: a line that is not written keeps no reference, `release_blobs`
//!   releases each reference once or not at all, and a damaged count keeps its blob
//!   (`line::tests::*`, `it::blobs::*`, `blob::tests::*`). A count may be too high, which only
//!   keeps a blob until a sweep recounts it.
//! - Lines are bounded by `line::MAX_LINE_BYTES` and `line::MAX_DEPTH`, so every line a writer
//!   writes reads back, and blobs by `blob::MAX_BLOB_BYTES`.
//! - `eileen record` is still a stub that writes nothing: every form says it is not built yet
//!   (`eileen-cli`'s `it::commands::every_command_of_section_3_5_says_not_built_yet`).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime`, `eileen-replay`, `eileen-scenarios`, `eileen-fleet`,
//!   `eileen-learn` and `eileen-cli`.
//! - May depend on `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on any other Eileen crate, such as `eileen-core` or `eileen-replay`, or on
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on vendored C outside
//!   `eileen-record::blob` (check 24), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: the recording files, all of them contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-record`.
//! - Layers: unit tests beside the store and the codec; round-trip property tests, trim and
//!   blob tests through the public API in `tests/it`; the `recording` fuzz target and its corpus
//!   replay (`it::fuzz_corpus`).
//! - A bug here starts as a recording that does not replay, or a blob nothing refers to.
//!
//! # Traps
//!
//! - A recording holds the user's words: it is `Personal` wherever it is shown or shared, and
//!   scenarios are made from it only through `eileen-scenarios`' redaction. The writer keeps
//!   what it is given exactly: replacing unaddressed speech by its placeholder is the
//!   recorder's job in `eileen-runtime`, before the line reaches the writer.
//! - A blob is shared: never delete one by its file, only by releasing a reference, or a
//!   recording, incident or trajectory that still names it stops reading.
//! - A data directory's store is opened once and shared: `BlobStore::open` takes the exclusive
//!   lock on `blobs/.lock`, and a second open fails rather than lose a count update.
//! - `reader::release_blobs` is not idempotent. Rename a recording to a tombstone first, release
//!   it once, then delete the tombstone; a retry after `Ok` releases every reference again.
//! - `trim::setup_kind` names every `Input` variant and every variant of the enums it looks
//!   into, with no catch-all arm, so a new input in the contracts fails to compile there until
//!   someone decides whether it sets standing state a scenario must start from.
//!
//! # Logs and tracing
//!
//! - None yet: the crate returns typed errors and its callers log them. The recorder's spans are
//!   named in `eileen_trace::names` when `eileen-runtime` wires the flight recorder to it.
//!
//! # Do not change without a decision record
//!
//! - The blob reference form (`{"$blob": "b3:..."}`), the `{"$lit": ...}` escape and the
//!   store's layout (`<2 hex>/<hash>.zst`, its `<hash>.refs` count and the folder's `.lock`).

#![forbid(unsafe_code)]

pub mod blob;
pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod line;
pub mod reader;
#[cfg(test)]
mod test_support;
pub mod trim;
pub mod writer;

//! Tracing for every Eileen process (BUILD.md section 2.13, observability report sections 2
//! to 4 and 8): the subscriber stack, span and event names with their parent table, the
//! `pii.` policy, the Redactor, the flight recorder, Perfetto timelines, `traceparent`, and
//! the scenario privacy rules.
//!
//! # Owns
//!
//! - [`init()`] per [`ProcessKind`]: JSON files, each process pruning only its own
//!   ([`TraceConfig::max_log_files`]), the console, the [`ring`] and the [`slow`] layer.
//! - [`names`] and the span-parent check ([`schema`]); the `pii.` policy ([`pii`]), the
//!   [`redact::Redactor`] and the scenario privacy rules and PII scan ([`privacy`]).
//! - The [`chrome`] converter, [`traceparent`] parsing, and `test_support` (feature
//!   `testing`, which only dev-dependencies turn on).
//!
//! # Does not own
//!
//! - Spawning tasks (`eileen-rt`), the panic hook and minidumps (`eileen-crash`), assembling
//!   incidents and the log janitor over every process's files (`eileen-runtime`: 14 days and
//!   500 MB, BUILD.md section 2.15), OpenTelemetry export (v0.2).
//!
//! # Invariants
//!
//! - Every span and event name is a constant in [`names`] with a row in [`names::ALL`]
//!   (test `names::tests::every_named_parent_is_a_declared_span`; check 21 scans the code).
//! - No secret reaches a file, a ring dump or the console, raw or escaped, and every file and
//!   dump line stays valid JSON (tests `it::canary::fake_secrets_never_reach_disk_or_a_dump`
//!   and `it::canary::fake_secrets_never_reach_the_console`).
//! - A ring dump scans only the values under each record's `fields`, so it keeps every
//!   record's ids and times (test `ring::tests::span_ids_of_reused_slots_survive_the_dump`),
//!   and a ring's size bounds what its lines and their queue allocate
//!   (`ring::tests::the_bound_counts_what_the_lines_allocate`).
//! - `pii.` fields reach the files only as length and keyed hash unless the user opted in
//!   (test `it::stack::file_lines_carry_their_span_and_reduce_pii`), and no log line or ring
//!   record carries a build PC's home folder (`it::canary::no_home_folder_reaches_disk_or_a_dump`).
//! - A span trace, text that any sink can carry, shows `pii.` fields as their length alone,
//!   whatever `logging.include_personal` says (tests
//!   `it::stack::a_logged_span_trace_shows_pii_fields_as_their_length` and
//!   `it::stack::a_logged_span_trace_shows_pii_lengths_with_personal_logs_on`).
//! - A process deletes only its own daily log files, `<process>.<YYYY-MM-DD>.jsonl` matched
//!   exactly, and never today's (tests `it::stack::the_core_keeps_the_other_processes_files`
//!   and `it::stack::the_core_prunes_its_own_files_by_the_date_in_their_names`).
//!
//! # Boundaries
//!
//! - Called by every Eileen crate; `tools/repo-tools` uses [`redact`] and [`privacy`].
//! - May depend on `eileen-contracts`, `eileen-stdx`, `tracing` and its subscriber, appender
//!   and error crates, `serde_json`, `blake3`, `thiserror` and `toml` (for [`privacy`]); no
//!   async runtime, no OpenTelemetry.
//! - Wire types: the JSON-lines log and ring formats, documented in [`file`](mod@file) and
//!   [`ring`].
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-trace` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-trace`).
//! - Layers: unit tests per module; integration tests for the canary and the whole stack.
//! - A bug here starts as a log line without its turn id, a span under the wrong parent, or
//!   a secret on disk.
//!
//! # Traps
//!
//! - [`init()`] can run once per process; tests use [`init::build`] with `with_default`.
//! - Span ids are reused after a span closes, and every process hands out the same ones:
//!   records carry their own ids, and a `traceparent` resolves by trace and span id
//!   ([`schema`]).
//! - User content goes in `pii.` fields only, never in a span name or a message.
//! - [`TraceConfig::pii_key`] is the per-install key the runtime derives; a process started
//!   without it writes `pii.` fields as their length alone.
//! - The core's process name, `eileen`, begins every other process's file name: match log
//!   file names exactly, never by prefix (tracing-appender's `max_log_files` matches by
//!   prefix, so it stays off).
//!
//! # Logs and tracing
//!
//! - Emits `span.slow` (target `eileen_trace::slow`), and `soft_assert.failed` and
//!   `log_bridge.failed` (target `eileen_trace`); owns no spans.
//! - Pruning old log files writes nothing to the logs or to stderr.
//! - Read back with `eileen trace <id>`, or open an incident's `timeline.json` in Perfetto.
//!
//! # Do not change without a decision record
//!
//! - The line formats, the `pii.` prefix and the reduced form `{len, h}` (`{len}` when the
//!   process has no per-install key), the replacement text `[redacted:<kind>]`, and the
//!   span-parent rules of a span another package owns.

#![forbid(unsafe_code)]

pub mod chrome;
pub mod file;
pub mod init;
pub mod names;
pub mod pii;
pub mod privacy;
pub mod record;
pub mod redact;
mod retention;
pub mod ring;
pub mod schema;
pub mod slow;
#[cfg(any(test, feature = "testing"))]
pub mod test_support;
pub mod traceparent;

pub use init::{Console, ProcessKind, TraceConfig, TraceError, TraceGuard, build, init};
pub use record::{EventRecord, SpanRecord};
pub use redact::Redactor;

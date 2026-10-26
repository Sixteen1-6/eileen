//! The async plumbing every Eileen process with a runtime shares: the one way to start a task
//! ([`task::spawn`], [`task::spawn_blocking`]), named bounded channels ([`channel`]), and the
//! runtime builder ([`runtime::build`]).
//!
//! # Owns
//!
//! - [`task::spawn`] and [`task::spawn_blocking`]: named, in the caller's span, registered with
//!   `await-tree`, a panic logged as `task.panicked` before it fails the join handle.
//! - [`task::dump`]: every live task's await tree, for `eileen debug tasks` and incidents.
//! - Bounded channels that name themselves and log `channel.full`.
//!
//! # Does not own
//!
//! - The lag probe, watchdogs and incident assembly (`eileen-runtime`), child processes
//!   (`eileen-supervisor`), threads that are not tokio's (`eileen_stdx::thread`).
//!
//! # Invariants
//!
//! - The only `#[expect(clippy::disallowed_methods)]` here sit on `task::spawn_raw` and
//!   [`task::spawn_blocking`] (check 14).
//! - A spawned task's events carry the span it was spawned in (test
//!   `it::tasks::a_task_keeps_the_span_it_was_spawned_in`).
//! - Every channel has a maximum; a full one is counted and logged, never grown (test
//!   `channel::tests::a_full_channel_refuses_and_counts`, and clippy's ban on
//!   `unbounded_channel`).
//! - A panic message reaches the log files only as length and hash, in `pii.message` (test
//!   `it::tasks::a_panic_message_reaches_the_log_files_only_as_its_length`).
//!
//! # Boundaries
//!
//! - Called by every async crate (`eileen-runtime`, `eileen-mcp`, `eileen-supervisor`,
//!   the sidecars and the shim).
//! - May depend on `eileen-stdx`, `eileen-trace`, `tokio`, `tracing` and `await-tree`.
//! - Wire types: none.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-rt`.
//! - Layers: unit tests for channels and messages; integration tests run tasks on a
//!   current-thread runtime under `eileen_trace::test_support::capture`.
//! - A bug here starts as a log line with no turn id, or a hung job nobody can see.
//!
//! # Traps
//!
//! - Task names are `&'static str` constants: put IDs in span fields, not in the name.
//! - A panic still reaches the join handle as a `JoinError`; logging it does not swallow it.
//! - `await-tree` overhead is unmeasured; section 12 of the observability report asks for it.
//!
//! # Logs and tracing
//!
//! - Emits `task.panicked` (its message in `pii.message`) and `channel.full`; owns no spans.
//!   `eileen debug tasks` prints [`task::dump`].
//!
//! # Do not change without a decision record
//!
//! - The one `tokio::spawn` and the one `spawn_blocking`, and what they attach to a task.

#![forbid(unsafe_code)]

pub mod channel;
pub mod runtime;
pub mod task;

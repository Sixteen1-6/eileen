//! Eileen's runtime around the core (BUILD.md sections 2.1, 2.2, 2.5, 2.12, 2.13 and 6.4): the
//! driver loop that owns `Core`, the store, the bus, the memory sampler, the loopback endpoint,
//! path classification, the dispatcher, the sidecar clients and the flight recorder wiring.
//! [`driver`], [`bus`] and [`console`] are built; the rest is a stub until WP2 builds it.
//!
//! # Owns
//!
//! - The driver loop ([`driver`]), the store, the bus with explicit `Lagged` ([`bus`]), the
//!   view `eileen run --console` prints from it ([`console`]), and the sampler.
//! - The loopback endpoint and the hooks, `PathFact`, `GitFact` and `UrlFact` classification,
//!   the dispatcher and the `session` and `dev` toolsets.
//! - The commands of section 3.5 it implements ([`cli`]) and the `worker_hook_payload` and
//!   `http_request` fuzz targets' entries (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Decisions (`eileen-core`), the components it starts, the composition root (`eileen-cli`),
//!   MCP itself (`eileen-mcp`) or child processes (`eileen-supervisor`).
//!
//! # Invariants
//!
//! - Every queue is bounded and every task starts through `eileen_rt`; a full input queue
//!   answers `Backpressure` (`driver::tests::a_full_input_queue_answers_backpressure`).
//! - Each effect family has at most one component; a second claim, or a claim on `Timer`,
//!   refuses to start (`driver::tests::a_family_claimed_twice_refuses_to_start`).
//! - Inputs, a fired timer's included, reach the core in `seq` order with times that never run
//!   backwards and never past a pending timer's `at` (WP2 acceptance 16,
//!   `driver::tests::inputs_take_seq_and_time_in_the_order_the_core_sees`,
//!   `driver::tests::timers::a_timer_due_before_an_input_is_stepped_first`).
//! - `Hud` snapshots reach their component at most 10 a second, the newest at each turn
//!   (`driver::tests::hud_snapshots_go_at_most_ten_a_second_ending_with_the_last`).
//! - A bus subscriber that never reads holds neither the core nor the others
//!   (`driver::tests::bus::a_subscriber_that_never_reads_never_holds_the_core`), and what it
//!   missed is counted by a `Lagged` notice (`bus::tests::every_event_is_delivered_or_counted`).
//! - No control character from an event's text reaches the console
//!   (`console::tests::text_from_outside_cannot_add_a_line_or_drive_the_terminal`).
//! - When a run ends, however it ends, every component's task has ended or been aborted
//!   (`driver::tests::stopping::a_core_panic_still_stops_every_component`).
//! - The `PreToolUse` endpoint never answers `allow` (WP2 acceptance 3), and only a runtime
//!   reading real memory writes the machine-wide memory files (WP2 acceptance 12).
//! - The stub runs nothing: every command here says it is not built yet (`eileen-cli`'s
//!   `it::commands::every_command_of_section_3_5_says_not_built_yet`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`. May depend on `eileen-core`, `eileen-record`, `eileen-replay`,
//!   `eileen-config`, `eileen-mcp`, `eileen-supervisor`, `eileen-buildguard` (`samples` only),
//!   `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on a component's crate (which `eileen-cli` wires), on rmcp except through
//!   `eileen-mcp`, on vendored C outside `eileen-runtime::store` (check 24) or on a `-win` or
//!   `-linux` crate (check 15), nor know vendor CLI flags (section 3.1).
//! - Wire types: the loopback endpoint's JSON and the store's rows, built from contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-runtime`.
//! - Layers: unit and table tests (the driver and the bus on paused tokio time, path classes,
//!   the HTTP parser), an insta snapshot of the console's lines, shuttle tests of the bus and
//!   the supervisor glue (not built yet), the redaction canary, the smoke with stubs.
//! - A bug here starts as an event out of order, a lagging subscriber that lost events silently,
//!   or a path in the wrong class.
//!
//! # Traps
//!
//! - The store's SQLite runs on the blocking pool, never on a runtime worker.
//! - A component sends an input's `seq` and times as zero: the driver stamps them ([`driver`]).
//! - A full component queue holds the driver, and the core with it, until there is room, so a
//!   component keeps taking effects while it waits on anything else. The bus never waits; the
//!   console writes to a terminal that can stall, so it runs on its own thread ([`console`]).
//! - `RunError::Component` keeps the component's words as `Personal`, out of its message, since
//!   they can quote a path: log `message.get()` only under `pii.message`.
//!
//! # Logs and tracing
//!
//! - Span `core.step` (DEBUG, one per input); events `effect.unserved` (once per family per
//!   run) and `component.ended`. The bus and the console have none of their own.
//!
//! # Do not change without a decision record
//!
//! - The loopback endpoint's routes and token rules, and the dispatcher's caller verification.

#![forbid(unsafe_code)]

/// The trait of every component the runtime starts (section 3.3.15), named here so the
/// composition root reaches it through the crate that runs the components.
pub use eileen_contracts::ports::Component;

pub mod bus;
pub mod cli;
pub mod console;
pub mod driver;
#[doc(hidden)]
pub mod fuzz_entry;

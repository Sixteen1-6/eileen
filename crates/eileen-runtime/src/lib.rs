//! Eileen's runtime around the core (BUILD.md sections 2.1, 2.2, 2.5, 2.12, 2.13 and 6.4): the
//! driver loop that owns `Core`, the SQLite store, the bus, the memory sampler, the loopback
//! HTTP endpoint, path classification, the dispatcher pipe server, the sidecar clients and the
//! flight recorder wiring. The driver loop ([`driver`]), the bus ([`bus`]) and the console view
//! ([`console`]) are built; the rest is a stub until WP2 builds it.
//!
//! # Owns
//!
//! - The driver loop ([`driver`]: one task owns the core, inputs through a bounded channel,
//!   effects routed to components by family, the core's timers), the store, the bus with
//!   explicit `Lagged` ([`bus`]: the component that claims `Emit` and fans each event out to
//!   its subscribers), and the sampler.
//! - The view `eileen run --console` prints from the bus ([`console`]), which `eileen-cli`
//!   gives a subscriber and the terminal.
//! - The loopback endpoint and the hooks, `PathFact`, `GitFact` and `UrlFact` classification,
//!   the dispatcher and the `session` and `dev` toolsets.
//! - The commands of section 3.5 it implements ([`cli`]) and the `worker_hook_payload` and
//!   `http_request` fuzz targets' entries (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Decisions (`eileen-core`), the components it starts (each crate's `component`), the
//!   composition root (`eileen-cli`), MCP itself (`eileen-mcp`) or child processes
//!   (`eileen-supervisor`).
//!
//! # Invariants
//!
//! - Every queue is bounded and every task starts through `eileen_rt` (`disallowed-methods`
//!   bans `unbounded_channel` and `tokio::spawn`); a full input queue answers `Backpressure`
//!   (`driver::tests::a_full_input_queue_answers_backpressure`).
//! - Each effect family has at most one component; a second claim, or a claim on `Timer`,
//!   refuses to start (`driver::tests::a_family_claimed_twice_refuses_to_start`).
//! - Inputs, a fired timer's included, reach the core in `seq` order with times that never run
//!   backwards, and a timer comes back at its `at` (WP2 acceptance 16,
//!   `driver::tests::inputs_take_seq_and_time_in_the_order_the_core_sees`).
//! - No input is stepped at a time past a pending timer's `at`: a timer due strictly earlier
//!   goes first, as in the simulator, even when tokio has not woken it yet
//!   (`driver::tests::timers::a_timer_due_before_an_input_is_stepped_first`).
//! - `Hud` snapshots reach their component at most 10 a second, the newest at each turn
//!   (`driver::tests::hud_snapshots_go_at_most_ten_a_second_ending_with_the_last`).
//! - Publishing a bus event never waits for a subscriber, so one that never reads holds
//!   neither the core nor the others
//!   (`driver::tests::bus::a_subscriber_that_never_reads_never_holds_the_core`).
//! - Every event a subscriber did not get is counted by a `Lagged` notice before the next
//!   event it gets, never lost silently (simulator invariant 6,
//!   `bus::tests::every_event_is_delivered_or_counted`).
//! - The console writes one line per event it shows, and no control character from an event's
//!   text reaches the terminal
//!   (`console::tests::text_from_outside_cannot_add_a_line_or_drive_the_terminal`).
//! - When a run ends, however it ends, every component's task has ended or been aborted
//!   (`driver::tests::stopping::a_core_panic_still_stops_every_component`).
//! - The `PreToolUse` endpoint never answers `allow`; `allow` comes only from a held `approve`
//!   call after a `UserInput` approval (WP2 acceptance 3).
//! - Only a runtime reading real memory writes the machine-wide memory files (section 6.1; WP2
//!   acceptance 12).
//! - The stub runs nothing: every command here says it is not built yet (`eileen-cli`'s
//!   `it::commands::every_command_of_section_3_5_says_not_built_yet`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`.
//! - May depend on `eileen-core`, `eileen-record`, `eileen-replay`, `eileen-config`,
//!   `eileen-mcp`, `eileen-supervisor`, `eileen-buildguard` (its `samples` module only),
//!   `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on a component's crate (`eileen-harness`, `eileen-audio`, `eileen-learn`
//!   and the like, which `eileen-cli` wires) or on rmcp except through `eileen-mcp`, nor know
//!   vendor CLI flags (section 3.1); nor on vendored C outside `eileen-runtime::store`
//!   (check 24), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: the loopback endpoint's JSON and the store's rows, built from contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-runtime`.
//! - Layers: unit and table tests (the driver and the bus on paused tokio time, path classes,
//!   the HTTP parser), an insta snapshot of the console's lines (`cargo xtask insta`), shuttle
//!   tests of the bus and the supervisor glue (not built yet), the redaction canary, and the
//!   end-to-end smoke with stubs.
//! - A bug here starts as an event out of order, a lagging subscriber that lost events silently,
//!   or a path in the wrong class.
//!
//! # Traps
//!
//! - The store's SQLite runs on the blocking pool, never on a runtime worker
//!   (`eileen-runtime::store` is the only module here that may use vendored C).
//! - A component sends an input's `seq` and times as zero (a non-zero `seq` fails a soft
//!   assertion): the queue's receiving end in the driver stamps them as it hands the input to the
//!   core, and stamps a fired timer the same way, never through its own queue.
//! - A full component queue holds the driver, and the core with it, until there is room: no
//!   input is stepped and no timer fires meanwhile, the core's own deadlines included, though
//!   shutdown and any component's end still end the run. So a component keeps taking effects
//!   while it waits on anything else.
//! - A bus subscriber that stops reading is not told until it reads again, and then gets one
//!   `Lagged` with the count: a reader that must not miss an event (the store's `events` table)
//!   reads without waiting on anything slower than the bus.
//! - Everything the console prints goes through `console`'s escaping. A new line that writes
//!   an event's text directly lets a job's output drive the user's terminal.
//! - `RunError::Component` keeps the component's words as `Personal`, out of its message, since
//!   they can quote a path: log `message.get()` only under `pii.message`.
//!
//! # Logs and tracing
//!
//! - Span `core.step` (DEBUG, one per input: `seq`, `input`, `effects`); events
//!   `effect.unserved` (once per family per run) and `component.ended` (its error in
//!   `pii.message`). The bus and the console log nothing: a subscriber learns of its lag
//!   from the `Lagged` event. The flight recorder's input tap and the lag probe come with WP2's W2 work.
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

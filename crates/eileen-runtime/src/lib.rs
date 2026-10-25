//! Eileen's runtime around the core (BUILD.md sections 2.1, 2.2, 2.5, 2.12, 2.13 and 6.4): the
//! driver loop that owns `Core`, the SQLite store, the bus, the memory sampler, the loopback
//! HTTP endpoint, path classification, the dispatcher pipe server, the sidecar clients and the
//! flight recorder wiring. A stub until WP2 builds it.
//!
//! # Owns
//!
//! - The driver loop (one task owns the core, inputs through a bounded channel, effects routed
//!   to components by family), the store, the bus with explicit `Lagged`, and the sampler.
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
//! - Every queue is bounded and every task starts through `eileen_rt`.
//! - The `PreToolUse` endpoint never answers `allow`; `allow` comes only from a held `approve`
//!   call after a `UserInput` approval (WP2 acceptance 3).
//! - Only a runtime reading real memory writes the machine-wide memory files (section 6.1).
//! - The stub runs nothing: every command here says it is not built yet.
//!
//! # Boundaries
//!
//! - Called by `eileen-cli`.
//! - May depend on `eileen-core`, `eileen-record`, `eileen-replay`, `eileen-config`,
//!   `eileen-mcp`, `eileen-supervisor`, `eileen-buildguard` (its `samples` module only),
//!   `eileen-platform`, `eileen-rt`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: the loopback endpoint's JSON and the store's rows, built from contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-runtime` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-runtime`).
//! - Layers: unit and table tests (path classes, the HTTP parser), shuttle tests of the bus and
//!   the supervisor glue, the redaction canary, and the end-to-end smoke with stubs.
//! - A bug here starts as an event out of order, a lagging subscriber that lost events silently,
//!   or a path in the wrong class.
//!
//! # Traps
//!
//! - The store's SQLite runs on the blocking pool, never on a runtime worker
//!   (`eileen-runtime::store` is the only module here that may use vendored C).
//!
//! # Logs and tracing
//!
//! - None yet; WP2 owns the driver's spans, the flight recorder's input tap and the lag probe.
//!
//! # Do not change without a decision record
//!
//! - The loopback endpoint's routes and token rules, and the dispatcher's caller verification.

#![forbid(unsafe_code)]

/// The trait of every component the runtime starts (section 3.3.15), named here so the
/// composition root reaches it through the crate that runs the components.
pub use eileen_contracts::ports::Component;

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

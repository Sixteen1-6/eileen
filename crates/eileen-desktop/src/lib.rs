//! Eileen's desktop control (BUILD.md section 2.6): the neutral MCP server behind the 18
//! `desktop_*` tools, with their per-op tiers, safety rules, grounding, element indexing,
//! self-healing selectors, DPI mapping and the insertion ladder, run as the `eileen-desktop`
//! sidecar. A lib and a binary. A stub until WP5 builds it; the stub binary says it is not built
//! yet.
//!
//! # Owns
//!
//! - The tool handlers and their `ToolSpec`s, the target-category tiers from
//!   `app-categories.toml`, typed-text redaction, the send-like word list, per-app grants.
//! - Element indexing and selector identity, evidence-based re-finding, set-of-mark drawing,
//!   the app-name scorer, the insertion ladder's decisions.
//! - The sidecar's command line, `eileen-desktop doctor` included ([`cli`]).
//!
//! # Does not own
//!
//! - The Windows backend: UI Automation, input, capture and windows (`eileen-desktop-win`), the
//!   permission gate (`eileen-core`), or MCP itself (`eileen-mcp`).
//!
//! # Invariants
//!
//! - Physical input during a lease stops synthetic input within one event (invariant 9).
//! - Eileen's own hotkeys are blocked combos; typed text is redacted before any log.
//! - Every UI Automation string enters through `eileen_contracts::desktop::sanitize`.
//!
//! # Boundaries
//!
//! - Started by `eileen-runtime` through the supervisor; called over MCP.
//! - May depend on `eileen-mcp`, `eileen-platform`, `eileen-supervisor`, `eileen-rt`,
//!   `eileen-contracts`, `eileen-stdx` and `eileen-trace`; `eileen-desktop-win` under the
//!   Windows target table.
//! - Wire types: the `desktop` toolset's arguments and results (`eileen-contracts::tools`).
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-desktop` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-desktop`).
//! - Layers: neutral unit tests on Linux against the platform fakes, and desktop tests in the
//!   `desktop` group under `desktop.lock`, on windows the test created.
//! - A bug here starts as a click that landed somewhere it should not have.
//!
//! # Traps
//!
//! - Desktop tests touch only windows the test created (owned fixtures such as `testpad`).
//!
//! # Logs and tracing
//!
//! - None yet. WP5's spans are `desktop.capture` and `desktop.action` from
//!   `eileen_trace::names`, each taking its remote parent from the call.
//!
//! # Do not change without a decision record
//!
//! - The per-op tiers and the safety rules of section 2.6.

#![forbid(unsafe_code)]

pub mod cli;

//! Eileen's drivers for vendor CLIs (BUILD.md sections 2.3, 2.5 and 3.3.8): Claude workers and
//! the Claude brain, the warm pool's session lifecycle, the brain benchmark, and the Codex, ACP,
//! `DeepSeek` and local routes as designed slots. The stream-json parser is built; the drivers
//! are stubs until WP3 builds them.
//!
//! # Owns
//!
//! - [`WorkerDriver`] and [`BrainBackend`] with their enum dispatch ([`AnyWorker`], [`AnyBrain`]),
//!   [`worker_for`], [`brain_for`] and [`brain_benchmark`].
//! - The harness [`component`] (the brain's Primary, Spare and Fallback slots, the workers) and
//!   `eileen bench brain`, `eileen claude` and `eileen doctor claude` ([`cli`]).
//! - The typed stream-json parser and its mappings to `WorkerEvent` and `BrainEvent`
//!   ([`stream`]).
//! - [`SUBAGENT_TOOLS`], the subagent tools every session disallows in v0.1.
//! - The `stream_json_line` and `control_message` fuzz targets' entries (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Decisions about jobs, sessions and approvals (`eileen-core`), starting processes
//!   (`eileen-supervisor`), build slots (`eileen-buildguard`), or the hook endpoint
//!   (`eileen-runtime`). `SessionSpec`, `JobBinding` and `BrainSlot` are contract types.
//!
//! # Invariants
//!
//! - Every vendor process starts through `eileen_supervisor`, in a Job Object
//!   (`disallowed-methods` bans `Command::new`; check 14).
//! - The stub builds only the Claude route, whose drivers answer `NotBuilt` until WP3 builds
//!   them, and every other route is `RouteUnavailable` (unit tests
//!   `tests::only_the_claude_route_has_a_worker_driver` and
//!   `tests::the_stub_claude_brain_answers_not_built_and_measures_nothing`).
//! - A stream-json line longer than `stream::MAX_LINE_BYTES` is malformed before it is parsed,
//!   an unknown `type` is `Unknown`, and a tool's input is redacted by the `Redactor` string by
//!   string before it is serialized and leaves the parser, at most 2 KB (`stream::tests`, and the
//!   `stream_json_line` fuzz target).
//! - Denials are read from the stream, and an `approve` call that errored, timed out or lost its
//!   server counts as one when the guard asked about the call (section 2.5;
//!   `stream::tests::failed_approve_calls_count_as_denials` and
//!   `stream::tests::a_failed_call_the_guard_did_not_ask_about_is_no_denial`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (the component) and `eileen-stub-harness`.
//! - May depend on `eileen-supervisor`, `eileen-buildguard`, `eileen-rt`, `eileen-contracts`,
//!   `eileen-stdx` and `eileen-trace`.
//! - Must not depend on `eileen-core` or `eileen-runtime` (the core decides), `eileen-mcp` or
//!   rmcp (section 3.1), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: the vendors' CLI flags and protocols (stream-json for Claude), which only this
//!   crate knows (section 3.1); Eileen's side of them is contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-harness`.
//! - Layers: unit tests for the parser and the flags (the parser's against the lines CLI 2.1.304
//!   wrote in WP3's approval contract test, copied under `tests/fixtures/wp3-approval/`), runs
//!   against `eileen-stub-harness`, the two fuzz targets with their corpora replayed in
//!   `it::fuzz_corpus` (`control_message` once its parser lands), and `real_claude::` tests on
//!   the owner's PC.
//! - A bug here starts as a stream-json line that became `Unknown`, or a session whose init
//!   lists a server it should not.
//!
//! # Traps
//!
//! - The real executable is resolved through the npm shim or the native layout, never the
//!   desktop app's copy.
//! - CI never holds vendor credentials, and no code path opens a vendor credential file.
//! - The CLI writes `system/init` again at the start of every user turn, and `system/init`
//!   lists the subagent tool as `Task` while the model calls it `Agent`.
//! - Not every denial looks like one: an `approve` call that fails reaches the model as a tool
//!   error and is missing from `result.permission_denials`, so the parser reads it from the
//!   tool result's text (`stream::DenialKind::ApproveFailed`). The CLI wraps any tool that
//!   failed in the same text, so only a call the guard asked about counts.
//! - Redact a JSON value's strings before serializing it, never its text: serialized, a newline
//!   before a token is the `n` of `\n`, after which the `Redactor` sees no word start.
//! - A `rate_limit_event` reports one window, while `PlanUsage` holds both with the nearer
//!   reset, so the events pass through `stream::PlanWindows` before they become `RateLimit`.
//!
//! # Logs and tracing
//!
//! - None yet; WP3 declares its spans in `eileen_trace::names`. Tool inputs are redacted in the
//!   parser, so no later log sees them whole.
//!
//! # Do not change without a decision record
//!
//! - The flags every Claude session gets (section 2.5), the approval route, and the subagent
//!   tools staying disallowed in v0.1.

#![forbid(unsafe_code)]

mod brain;
pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod stream;
mod worker;

pub use brain::{
    AnyBrain, BrainBackend, BrainBenchResult, BrainSuite, ClaudeBrain, CodexBrain, DeepSeekBrain,
    LocalBrain, brain_benchmark, brain_for,
};
use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::jobs::Route;
use eileen_contracts::ports::{Component, IdleComponent};
pub use worker::{
    AcpWorker, AnyWorker, ClaudeWorker, CodexWorker, WorkerDriver, WorkerSession, worker_for,
};

/// The subagent tools, which no session gets in v0.1 (section 2.5): `--disallowedTools` names
/// them and `--tools` never lists them. Both names, because `system/init` lists the tool as
/// `Task` and the model calls it `Agent` (WP3's approval contract test, section 2.5).
pub const SUBAGENT_TOOLS: [&str; 2] = ["Task", "Agent"];

/// What the harness reads at start. WP3 adds its fields (the CLI's location, the model aliases,
/// the pool sizes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessConfig;

/// What went wrong in a driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    /// The route is not built in this version.
    #[error("the {} route is not available", .route.display_name())]
    RouteUnavailable {
        /// The route asked for.
        route: Route,
    },
    /// The route is built in this version, and WP3 has not built its driver yet.
    #[error("the {} route is not built yet (WP3 builds it)", .route.display_name())]
    NotBuilt {
        /// The route asked for.
        route: Route,
    },
}

/// The harness component: the brain's slots and the workers. The stub accepts the brain and
/// worker families and does nothing.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "section 3.3.15's signature; HarnessConfig has no fields until WP3"
)]
pub fn component(_cfg: &HarnessConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("harness", &[EffectFamily::Brain, EffectFamily::Worker]))
}

#[cfg(test)]
mod tests;

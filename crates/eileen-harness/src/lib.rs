//! Eileen's drivers for vendor CLIs (BUILD.md sections 2.3, 2.5 and 3.3.8): Claude workers and
//! the Claude brain, the warm pool's session lifecycle, the brain benchmark, and the Codex, ACP,
//! `DeepSeek` and local routes as designed slots. A stub until WP3 builds it.
//!
//! # Owns
//!
//! - [`WorkerDriver`] and [`BrainBackend`] with their enum dispatch ([`AnyWorker`], [`AnyBrain`]),
//!   [`worker_for`], [`brain_for`] and [`brain_benchmark`].
//! - The harness [`component`] (the brain's Primary, Spare and Fallback slots, the workers) and
//!   `eileen bench brain`, `eileen claude` and `eileen doctor claude` ([`cli`]).
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
//! - Only this crate knows vendor CLI flags and protocols (section 3.1).
//! - Every vendor process starts through `eileen_supervisor`, in a Job Object.
//! - The stub builds only the Claude route, whose drivers answer `NotBuilt` until WP3 builds
//!   them, and every other route is `RouteUnavailable` (unit tests in `tests`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (the component) and `eileen-stub-harness`.
//! - May depend on `eileen-supervisor`, `eileen-buildguard`, `eileen-rt`, `eileen-contracts`,
//!   `eileen-stdx` and `eileen-trace`.
//! - Wire types: the vendors' protocols (stream-json for Claude); Eileen's side of them is
//!   contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-harness` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-harness`).
//! - Layers: unit tests for the parser and the flags, runs against `eileen-stub-harness`, the
//!   two fuzz targets, and `real_claude::` tests on the owner's PC.
//! - A bug here starts as a stream-json line that became `Unknown`, or a session whose init
//!   lists a server it should not.
//!
//! # Traps
//!
//! - The real executable is resolved through the npm shim or the native layout, never the
//!   desktop app's copy.
//! - CI never holds vendor credentials, and no code path opens a vendor credential file.
//!
//! # Logs and tracing
//!
//! - None yet; WP3 declares its spans in `eileen_trace::names` and redacts tool inputs.
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

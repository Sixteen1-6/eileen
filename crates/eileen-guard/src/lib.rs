//! Eileen's guard (BUILD.md sections 2.1 and 2.13): a small process started detached from the
//! core's Job Object, which watches the core's heartbeat, writes crash and hang dumps through
//! its `minidumper` server, restarts a hung core with a rate limit, and reports when it fails
//! fast. A lib and the `eileen-guard` binary. A stub until WP2 builds it; the stub binary says it
//! is not built yet.
//!
//! # Owns
//!
//! - The `minidumper` server every process's crash handler (`eileen-crash`) talks to, and the
//!   hang dump before a kill.
//! - The core heartbeat watch and `restart_core`, the guard's one sanctioned `Command::new`
//!   (section 4b.2, check 14).
//! - The guard's entry point ([`run`]).
//!
//! # Does not own
//!
//! - Its own spawn (`eileen_supervisor::spawn_detached`), the in-process crash handler
//!   (`eileen-crash`), or incidents (`eileen-runtime`).
//!
//! # Invariants
//!
//! - It runs outside the core's Job Object, so it outlives a core it must restart
//!   (`eileen-supervisor`'s `tests::the_guard_spawn_outlives_its_parent`, run on Windows).
//! - Restarts are rate-limited; a core that keeps failing is reported, not looped (WP2 adds the
//!   test).
//! - `minidumper` appears only under its Windows target table (section 4.1, check 15).
//!
//! # Boundaries
//!
//! - Started by `eileen-runtime` through `eileen_supervisor::spawn_detached`.
//! - May depend on `eileen-crash`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`, and
//!   on `minidumper` under the Windows target table; it is one of the three crates outside the
//!   spawn rule (section 3.1).
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-supervisor`, `eileen-rt` or
//!   rmcp, nor know vendor CLI flags (section 3.1); nor on a `-win` or `-linux` crate, or on
//!   `minidumper` outside the Windows target table (check 15).
//! - Wire types: `minidumper`'s protocol and the core's heartbeat.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-guard`.
//! - Layers: a Windows test that kills a stand-in sidecar mid-call and finds the guard's hang
//!   dump in the incident and a restart within the backoff bound (WP2 acceptance 2).
//! - A bug here starts as a hung core nobody restarted, or a restart storm.
//!
//! # Traps
//!
//! - Its handles are null (it is spawned detached), so it cannot log to stderr; it logs to its
//!   own file.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 names its events from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The one `#[expect]`ed `Command::new` in `restart_core`, and the restart rate limit.

#![forbid(unsafe_code)]

use std::io::{self, Write as _};
use std::process::ExitCode;

/// Runs the guard.
pub const fn run() -> Result<(), RunError> {
    Err(RunError::NotBuilt)
}

/// Why the guard stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// WP2 has not built it yet.
    #[error("`eileen-guard` is not built yet (WP2 builds it)")]
    NotBuilt,
}

/// The `eileen-guard` binary: runs it and reports a failure on standard error. Returns the
/// process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => match writeln!(io::stderr().lock(), "Error: {error}") {
            Ok(()) | Err(_) => ExitCode::FAILURE,
        },
    }
}

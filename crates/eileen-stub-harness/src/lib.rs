//! A stand-in for `claude -p` in tests (BUILD.md sections 3.3.16 and 4.7): it speaks the same
//! stream-json and plays `StubScriptLine` scripts from `tests/harness-scripts/*.ndjson`, so CI
//! runs workers and the brain with no vendor CLI and no credentials. Development only. A lib and
//! the `eileen-stub-harness` binary. A stub until WP3 builds it; the stub binary says it is not
//! built yet.
//!
//! # Owns
//!
//! - Playing a script: its lines in order, held where the script waits for a tool result or an
//!   approval, with its faults (crash, hang, malformed line).
//! - The binary `eileen run --stub-harness` starts in place of the claude CLI ([`run`]).
//!
//! # Does not own
//!
//! - The scripts (`tests/harness-scripts/`, recorded and redacted from real sessions), the
//!   driver it stands in for (`eileen-harness`), or the script format (`eileen-contracts`).
//!
//! # Invariants
//!
//! - Event shapes match the real CLI's: the nightly contract test compares them with the
//!   scripts (`cargo xtask contract-claude` on the owner's PC, which WP3 builds).
//!
//! # Boundaries
//!
//! - Started by `eileen-harness` through the supervisor in tests; no shipped crate depends on it
//!   (development only: section 3.1's table, and `process = "tests"` in its metadata, check 22).
//! - May depend on `eileen-harness`, `eileen-contracts` and `eileen-stdx`.
//! - Must not depend on `eileen-trace` (it writes only the protocol on stdout), `eileen-rt`,
//!   `eileen-runtime` or rmcp (section 3.1), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: stream-json, as the claude CLI writes it.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-stub-harness` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-stub-harness`).
//! - Layers: the harness's tests against it (WP3 acceptance 2).
//! - A bug here starts as a harness test that passes against the stub and fails against the
//!   real CLI.
//!
//! # Traps
//!
//! - A script holds redacted text only; the `Redactor` runs before a recording becomes one.
//!
//! # Logs and tracing
//!
//! - None: it writes only the protocol on stdout.
//!
//! # Do not change without a decision record
//!
//! - The script format (`StubScriptLine`, a contract type).

#![forbid(unsafe_code)]

use std::io::{self, Write as _};
use std::process::ExitCode;

/// Runs the stub harness.
pub const fn run() -> Result<(), RunError> {
    Err(RunError::NotBuilt)
}

/// Why the stub harness stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// WP3 has not built it yet.
    #[error("`eileen-stub-harness` is not built yet (WP3 builds it)")]
    NotBuilt,
}

/// The `eileen-stub-harness` binary: runs it and reports a failure on standard error. Returns the
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

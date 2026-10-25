//! The `eileen-e2e` command line, which `cargo xtask bench`, `triage` and `fleet-validate`
//! dispatch to (BUILD.md section 4b.5; `contract-claude` goes to WP3's harness binary). A stub
//! until WP9 builds it: every command says it is not built yet.

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP9 has not built it yet.
    #[error("`{command}` is not built yet (WP9 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// `eileen-e2e <command>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "eileen-e2e", version, about = "Eileen's end-to-end tools")]
pub struct E2eCli {
    /// The command.
    #[command(subcommand)]
    pub command: E2eCommand,
}

/// The commands of `eileen-e2e`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum E2eCommand {
    /// Run the benchmarks.
    Bench,
    /// Cluster fleet failures into issues.
    Triage,
    /// Validate fleet result files.
    FleetValidate,
}

/// Runs `eileen-e2e`.
pub const fn run(_cli: E2eCli) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen-e2e" })
}

/// The `eileen-e2e` binary: parses its command line, runs it, and reports a failure on
/// standard error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match E2eCli::try_parse() {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(&error),
        },
        Err(error) => usage_exit(&error),
    }
}

/// Prints clap's help, version or usage error and returns its exit code: 0 for help and
/// version, 2 for a usage error.
fn usage_exit(error: &clap::Error) -> ExitCode {
    let printed = error.print();
    if error.use_stderr() || printed.is_err() { ExitCode::from(2) } else { ExitCode::SUCCESS }
}

/// Reports a failed command on standard error and returns exit code 1.
fn failure(error: &impl std::fmt::Display) -> ExitCode {
    match writeln!(io::stderr().lock(), "Error: {error}") {
        Ok(()) | Err(_) => ExitCode::FAILURE,
    }
}

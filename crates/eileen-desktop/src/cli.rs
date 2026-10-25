//! The `eileen-desktop` command line: with no subcommand it serves the `desktop` toolset over
//! MCP on stdio; `doctor` is what `eileen doctor desktop` runs. A stub until WP5 builds it: every
//! form says it is not built yet.

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP5 has not built it yet.
    #[error("`{command}` is not built yet (WP5 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// `eileen-desktop [doctor]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "eileen-desktop", version, about = "Eileen's desktop control sidecar")]
pub struct DesktopCli {
    /// What to do instead of serving the desktop tools.
    #[command(subcommand)]
    pub command: Option<DesktopCommand>,
}

/// The subcommands of `eileen-desktop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum DesktopCommand {
    /// Check desktop control on this PC.
    Doctor,
}

/// Runs `eileen-desktop`.
pub const fn run(_cli: DesktopCli) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen-desktop" })
}

/// The `eileen-desktop` binary: parses its command line, runs it, and reports a failure on
/// standard error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match DesktopCli::try_parse() {
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

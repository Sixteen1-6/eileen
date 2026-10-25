//! The `eileen-apps` command line: with no subcommand it serves the `apps` toolset over MCP on
//! stdio; `setup` (the face model) and `doctor resolve` are what `eileen setup` and `eileen doctor
//! resolve` run. A stub until WP6 builds it: every form says it is not built yet.

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP6 has not built it yet.
    #[error("`{command}` is not built yet (WP6 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// `eileen-apps [setup | doctor ...]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "eileen-apps", version, about = "Eileen's app adapters sidecar")]
pub struct AppsCli {
    /// What to do instead of serving the app tools.
    #[command(subcommand)]
    pub command: Option<AppsCommand>,
}

/// The subcommands of `eileen-apps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AppsCommand {
    /// Download and verify the face model.
    Setup,
    /// Check an app's adapter on this PC.
    #[command(subcommand)]
    Doctor(DoctorTarget),
}

/// The apps `eileen-apps doctor` checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum DoctorTarget {
    /// Resolve, through every route or the one named.
    Resolve {
        /// Check through this route only.
        #[arg(long, value_enum)]
        route: Option<ResolveRoute>,
    },
}

/// The routes to Resolve (section 2.7). `eileen-runtime` has its own copy for `eileen doctor
/// resolve --route`, which it passes to this binary: the core binary may not depend on this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResolveRoute {
    /// Resolve's own MCP server.
    Mcp,
    /// The Lua bridge.
    Bridge,
}

/// Runs `eileen-apps`.
pub const fn run(_cli: AppsCli) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen-apps" })
}

/// The `eileen-apps` binary: parses its command line, runs it, and reports a failure on
/// standard error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match AppsCli::try_parse() {
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

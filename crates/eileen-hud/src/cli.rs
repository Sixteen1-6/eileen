//! The `eileen-hud` command line: it runs the HUD process on the pipe the core names. A stub
//! until WP8 builds it: it says it is not built yet.

use std::io::{self, Write as _};
use std::process::ExitCode;

use clap::Parser;

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP8 has not built it yet.
    #[error("`{command}` is not built yet (WP8 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// `eileen-hud`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "eileen-hud", version, about = "Eileen's on-screen layer")]
pub struct HudCli;

/// Runs `eileen-hud`.
pub const fn run(_cli: HudCli) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen-hud" })
}

/// The `eileen-hud` binary: parses its command line, runs it, and reports a failure on
/// standard error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match HudCli::try_parse() {
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

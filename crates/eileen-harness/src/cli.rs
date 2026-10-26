//! The commands of BUILD.md section 3.5 that the harness implements, which `eileen-cli`
//! dispatches here: `bench brain`, `claude` and `doctor claude`. A stub until WP3 builds them:
//! each says it is not built yet.

use clap::{Args, Subcommand};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP3 has not built it yet.
    #[error("`{command}` is not built yet (WP3 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// `eileen bench brain [--backend B]`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct BenchBrainArgs {
    /// Benchmark this backend instead of the current one.
    #[arg(long, value_name = "B")]
    pub backend: Option<String>,
}

/// The forms of `eileen claude`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum ClaudeCommand {
    /// Print the official plugin install commands and run them after a yes.
    Install,
    /// Print the uninstall commands and run them after a yes.
    Uninstall,
    /// Drain and restart Claude sessions around a CLI update.
    UpdateWindow,
}

const fn not_built(command: &'static str) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command })
}

/// Runs `eileen bench brain`.
pub fn bench_brain(_args: BenchBrainArgs) -> Result<(), CliError> {
    not_built("eileen bench brain")
}

/// Runs `eileen claude`.
pub const fn claude(_command: Option<ClaudeCommand>) -> Result<(), CliError> {
    not_built("eileen claude")
}

/// Runs `eileen doctor claude`.
pub const fn doctor_claude() -> Result<(), CliError> {
    not_built("eileen doctor claude")
}

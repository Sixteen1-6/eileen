//! The commands of BUILD.md section 3.5 that the harness implements, which `eileen-cli`
//! dispatches here: `bench brain`, `claude` and `doctor claude`. `doctor claude` is built
//! ([`doctor`]); the other two say they are not built yet.

use std::io::{self, Write};

use clap::{Args, Subcommand};

use crate::locate::LocateError;

pub mod doctor;

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP3 has not built it yet.
    #[error("`{command}` is not built yet (WP3 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
    /// No claude CLI could be used; the doctor printed what it searched and passed over.
    #[error(transparent)]
    Locate(#[from] LocateError),
    /// `eileen doctor claude` found the CLI unusable (signed out, too old, or no answer); the
    /// report says which.
    #[error("the claude CLI is not ready for Eileen (see the report above)")]
    ClaudeNotReady,
    /// The doctor's report could not be printed, or its runtime could not start.
    #[error("`eileen doctor claude` could not run: {reason}")]
    DoctorFailed {
        /// The I/O error, as text.
        reason: String,
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

/// Runs `eileen doctor claude` and prints its report on standard output: the CLI Eileen
/// would run, its version, whether it is signed in, and the toolchains a worker sees. Fails
/// when no CLI can be used or the one found is not ready.
pub fn doctor_claude() -> Result<(), CliError> {
    let failed = |error: io::Error| CliError::DoctorFailed { reason: error.to_string() };
    let inputs = doctor::DoctorInputs::from_process();
    let runtime = eileen_rt::runtime::build("doctor", Some(1), 1).map_err(failed)?;
    let found = doctor::doctor(&inputs, &mut doctor::SupervisedRunner::new(&runtime));
    let mut out = io::stdout().lock();
    match found {
        Ok(report) => {
            write!(out, "{report}").map_err(failed)?;
            if report.healthy() { Ok(()) } else { Err(CliError::ClaudeNotReady) }
        }
        Err(error) => {
            write!(out, "{}", doctor::LocateReport(&error)).map_err(failed)?;
            Err(CliError::Locate(error))
        }
    }
}

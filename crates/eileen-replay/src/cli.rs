//! `eileen replay` and `eileen scenario replay` (BUILD.md section 3.5), which `eileen-cli` and
//! `eileen-scenarios` dispatch here. A stub until WP1 builds replay: both say they are not built
//! yet.

use clap::{Args, ValueEnum};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP1 has not built it yet.
    #[error("`{command}` is not built yet (WP1 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// Which replay mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReplayMode {
    /// Every recorded input in order, stopping at the first difference.
    Strict,
    /// A fresh core fed by the scenario rules.
    Scenario,
}

/// Which level to replay at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReplayLevel {
    /// The core alone.
    Core,
}

/// `eileen replay ID [--level core] [--mode strict|scenario]`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ReplayArgs {
    /// The recording or incident to replay.
    pub id: String,
    /// The level to replay at.
    #[arg(long, value_enum)]
    pub level: Option<ReplayLevel>,
    /// The replay mode.
    #[arg(long, value_enum)]
    pub mode: Option<ReplayMode>,
}

/// `eileen scenario replay ID`, which `cargo xtask scenarios replay` dispatches to.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ScenarioReplayArgs {
    /// The scenario, as `<area>/<id>`.
    pub id: String,
}

/// Runs `eileen replay`.
pub fn replay(_args: ReplayArgs) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen replay" })
}

/// Runs `eileen scenario replay`.
pub fn scenario_replay(_args: ScenarioReplayArgs) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen scenario replay" })
}

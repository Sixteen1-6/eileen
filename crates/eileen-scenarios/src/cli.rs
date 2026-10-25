//! `eileen scenario` (BUILD.md section 3.5), which `eileen-cli` dispatches here. A stub until WP2
//! builds it: every form says it is not built yet, and `replay` goes to `eileen-replay`, whose
//! stub says the same.

use clap::{Args, Subcommand};
use eileen_replay::cli::ScenarioReplayArgs;

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP2 has not built it yet.
    #[error("`{command}` is not built yet (WP2 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
    /// `eileen scenario replay`, which `eileen-replay` implements, failed.
    #[error(transparent)]
    Replay(#[from] eileen_replay::cli::CliError),
}

/// One scenario, as `<area>/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct ScenarioRef {
    /// The scenario, as `<area>/<id>`.
    pub id: String,
}

/// `eileen scenario stale <id> --issue <url> [--days N]`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct StaleArgs {
    /// The scenario, as `<area>/<id>`.
    pub id: String,
    /// The issue that tracks the breakage.
    #[arg(long)]
    pub issue: String,
    /// Days until the mark expires.
    #[arg(long)]
    pub days: Option<u16>,
}

/// The forms of `eileen scenario`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ScenarioCommand {
    /// Save the last turn or job as a local scenario.
    Save,
    /// List the scenarios.
    List,
    /// Show one scenario.
    Show(ScenarioRef),
    /// Generalise and redact a scenario so it can be shared.
    Share(ScenarioRef),
    /// Submit a shared scenario as a pull request.
    Submit(ScenarioRef),
    /// Run a scenario on the real desktop.
    Run(ScenarioRef),
    /// Replay a scenario in scenario mode.
    Replay(ScenarioReplayArgs),
    /// Re-run a scenario and rewrite its expected trace if every expectation still passes.
    Bless(ScenarioRef),
    /// Mark a scenario stale, with today's date as `since`.
    Stale(StaleArgs),
    /// Check the scenarios against the privacy, schema and expiry rules CI applies.
    Check,
}

/// Runs `eileen scenario`.
pub fn scenario(command: Option<ScenarioCommand>) -> Result<(), CliError> {
    let Some(ScenarioCommand::Replay(args)) = command else {
        return Err(CliError::NotBuilt { command: "eileen scenario" });
    };
    eileen_replay::cli::scenario_replay(args).map_err(CliError::from)
}

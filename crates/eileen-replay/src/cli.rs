//! `eileen replay` and `eileen scenario replay` (BUILD.md section 3.5), which `eileen-cli` and
//! `eileen-scenarios` dispatch here. A stub until WP1 builds replay: both say they are not built
//! yet.

use clap::{Args, ValueEnum};

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
pub fn replay(_args: ReplayArgs) -> anyhow::Result<()> {
    anyhow::bail!("`eileen replay` is not built yet (WP1 builds it)")
}

/// Runs `eileen scenario replay`.
pub fn scenario_replay(_args: ScenarioReplayArgs) -> anyhow::Result<()> {
    anyhow::bail!("`eileen scenario replay` is not built yet (WP1 builds it)")
}

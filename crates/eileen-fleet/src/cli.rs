//! `eileen test --fleet` (BUILD.md section 3.5), which `eileen-cli` dispatches here. A stub
//! until WP9 builds the runner: it says it is not built yet.

use std::path::PathBuf;

use clap::{Args, ValueEnum};

/// How the fleet runs a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunMode {
    /// Brain and worker answers from the recording, no model.
    Recorded,
    /// Real Claude, within a turn budget.
    Live,
}

/// The arguments of `eileen test`: `--fleet`; one of `--once`, `--nightly` and `--arm-tonight`;
/// `--filter`; `--mode recorded|live`; and `--submit`, with `--push` or `--file F`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[expect(clippy::struct_excessive_bools, reason = "one field per flag of `eileen test`")]
pub struct TestArgs {
    /// Run the fleet's scenarios.
    #[arg(long)]
    pub fleet: bool,
    /// Run once now, attended.
    #[arg(long)]
    pub once: bool,
    /// Run every night.
    #[arg(long)]
    pub nightly: bool,
    /// Run tonight after 30 idle minutes.
    #[arg(long)]
    pub arm_tonight: bool,
    /// Run only the scenarios that match.
    #[arg(long)]
    pub filter: Option<String>,
    /// Recorded or live.
    #[arg(long, value_enum)]
    pub mode: Option<RunMode>,
    /// Open the results pull request from your own account.
    #[arg(long)]
    pub submit: bool,
    /// Commit the results straight to the `devhub` branch (contributors with write access).
    #[arg(long, requires = "submit")]
    pub push: bool,
    /// Submit a results file someone else sent.
    #[arg(long, value_name = "F", requires = "submit")]
    pub file: Option<PathBuf>,
}

/// Runs `eileen test`.
pub fn test(_args: TestArgs) -> anyhow::Result<()> {
    anyhow::bail!("`eileen test --fleet` is not built yet (WP9 builds it)")
}

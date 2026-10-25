//! The `eileen-e2e` command line, which `cargo xtask contract-claude`, `bench`, `triage` and
//! `fleet-validate` dispatch to (BUILD.md section 4b.5). A stub until WP9 builds it: every
//! command says it is not built yet.

use clap::{Parser, Subcommand};

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
    /// Replay the harness scripts' prompts through the real claude CLI and compare event shapes.
    ContractClaude,
    /// Run the benchmarks.
    Bench,
    /// Cluster fleet failures into issues.
    Triage,
    /// Validate fleet result files.
    FleetValidate,
}

/// Runs `eileen-e2e`.
pub fn run(_cli: E2eCli) -> anyhow::Result<()> {
    anyhow::bail!("`eileen-e2e` is not built yet (WP9 builds it)")
}

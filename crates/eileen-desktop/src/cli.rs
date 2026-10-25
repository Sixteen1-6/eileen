//! The `eileen-desktop` command line: with no subcommand it serves the `desktop` toolset over
//! MCP on stdio; `doctor` is what `eileen doctor desktop` runs. A stub until WP5 builds it: every
//! form says it is not built yet.

use clap::{Parser, Subcommand};

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
pub fn run(_cli: DesktopCli) -> anyhow::Result<()> {
    anyhow::bail!("`eileen-desktop` is not built yet (WP5 builds it)")
}

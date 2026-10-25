//! The `eileen-apps` command line: with no subcommand it serves the `apps` toolset over MCP on
//! stdio; `setup` (the face model) and `doctor resolve` are what `eileen setup` and `eileen doctor
//! resolve` run. A stub until WP6 builds it: every form says it is not built yet.

use clap::{Parser, Subcommand, ValueEnum};

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

/// The routes to Resolve (section 2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResolveRoute {
    /// Resolve's own MCP server.
    Mcp,
    /// The Lua bridge.
    Bridge,
}

/// Runs `eileen-apps`.
pub fn run(_cli: AppsCli) -> anyhow::Result<()> {
    anyhow::bail!("`eileen-apps` is not built yet (WP6 builds it)")
}

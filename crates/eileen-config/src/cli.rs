//! The commands of BUILD.md section 3.5 that `eileen-config` implements, which `eileen-cli`
//! dispatches here: `config` and `apps add-mcp`. A stub until WP7 builds them: each says it is
//! not built yet.

use clap::{Args, Subcommand};

/// The forms of `eileen config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum ConfigCommand {
    /// Show every setting's value and where it came from.
    Explain,
    /// Open the configuration file in an editor.
    Edit,
    /// Undo the last settings change.
    Undo,
    /// List the profiles.
    Profiles,
    /// Export the configuration.
    Export,
    /// Import a configuration.
    Import,
}

/// `eileen apps add-mcp <app> <server>`: approve a third-party MCP server for an app.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct AddMcpArgs {
    /// The app.
    pub app: String,
    /// The server's registry id.
    pub server: String,
}

/// Runs `eileen config`.
pub fn config(_command: Option<ConfigCommand>) -> anyhow::Result<()> {
    anyhow::bail!("`eileen config` is not built yet (WP7 builds it)")
}

/// Runs `eileen apps add-mcp`.
pub fn apps_add_mcp(_args: AddMcpArgs) -> anyhow::Result<()> {
    anyhow::bail!("`eileen apps add-mcp` is not built yet (WP7 builds it)")
}

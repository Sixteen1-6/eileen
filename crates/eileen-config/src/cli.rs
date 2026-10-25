//! The commands of BUILD.md section 3.5 that `eileen-config` implements, which `eileen-cli`
//! dispatches here: `config` and `apps add-mcp`. A stub until WP7 builds them: each says it is
//! not built yet.

use clap::{Args, Subcommand};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP7 has not built it yet.
    #[error("`{command}` is not built yet (WP7 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

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
pub const fn config(_command: Option<ConfigCommand>) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen config" })
}

/// Runs `eileen apps add-mcp`.
pub fn apps_add_mcp(_args: AddMcpArgs) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen apps add-mcp" })
}

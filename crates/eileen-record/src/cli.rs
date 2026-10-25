//! `eileen record` (BUILD.md section 3.5), which `eileen-cli` dispatches here. A stub until WP1
//! builds the recorder: every form says it is not built yet.

use clap::Subcommand;

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

/// The forms of `eileen record`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum RecordCommand {
    /// Start recording this session.
    Start,
    /// Stop recording.
    Stop,
    /// List the recordings.
    List,
    /// Trim a recording to the inputs a scenario needs.
    Trim,
}

/// Runs `eileen record`.
pub const fn record(_command: Option<RecordCommand>) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen record" })
}

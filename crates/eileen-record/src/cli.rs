//! `eileen record` (BUILD.md section 3.5), which `eileen-cli` dispatches here. A stub until WP1
//! builds the recorder: every form says it is not built yet.

use clap::Subcommand;

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
pub fn record(_command: Option<RecordCommand>) -> anyhow::Result<()> {
    anyhow::bail!("`eileen record` is not built yet (WP1 builds it)")
}

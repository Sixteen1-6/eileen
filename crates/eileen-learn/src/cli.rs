//! The commands of BUILD.md section 3.5 that `eileen-learn` implements, which `eileen-cli`
//! dispatches here: `skills`, `facts`, `apps proposals` and `notes`. A stub until WP7 builds
//! them: each says it is not built yet.

use clap::{Args, Subcommand};

/// `eileen skills distill [--job ID] [--wait]`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct DistillArgs {
    /// Distill this job instead of the next in the queue.
    #[arg(long, value_name = "ID")]
    pub job: Option<String>,
    /// Wait for the distillation's outcome.
    #[arg(long)]
    pub wait: bool,
}

/// The forms of `eileen skills`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum SkillsCommand {
    /// List the learned skills.
    List,
    /// Show one skill.
    Show,
    /// Show a skill's versions.
    History,
    /// Go back to an earlier version.
    Rollback,
    /// Forget a skill.
    Forget,
    /// Distill a finished job into a skill draft.
    Distill(DistillArgs),
}

/// The forms of `eileen facts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum FactsCommand {
    /// List the facts.
    List,
    /// Forget a fact.
    Forget,
}

/// The forms of `eileen notes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum NotesCommand {
    /// List Claude's notes on this PC and the held notes.
    List,
    /// Show one note, with who wrote it and when.
    Show,
    /// Forget a note.
    Forget,
    /// Keep a held note.
    Keep,
    /// Drop a held note.
    Drop,
}

fn not_built(command: &str) -> anyhow::Result<()> {
    anyhow::bail!("`{command}` is not built yet (WP7 builds it)")
}

/// Runs `eileen skills`.
pub fn skills(_command: Option<SkillsCommand>) -> anyhow::Result<()> {
    not_built("eileen skills")
}

/// Runs `eileen facts`.
pub fn facts(_command: Option<FactsCommand>) -> anyhow::Result<()> {
    not_built("eileen facts")
}

/// Runs `eileen apps proposals`.
pub fn apps_proposals() -> anyhow::Result<()> {
    not_built("eileen apps proposals")
}

/// Runs `eileen notes`.
pub fn notes(_command: Option<NotesCommand>) -> anyhow::Result<()> {
    not_built("eileen notes")
}

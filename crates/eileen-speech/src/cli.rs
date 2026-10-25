//! The `eileen-speech` command line (BUILD.md sections 3.5 and 6.6): with no subcommand it runs
//! as the speech process; `setup`, `bench` and `voice ...` are what `eileen setup`, `eileen
//! doctor machine --bench` and `eileen voice` run. A stub until WP4 builds it: every form says it
//! is not built yet.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// `eileen-speech [--stub FILE] [setup | bench | voice ...]`.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "eileen-speech", version, about = "Eileen's speech process")]
pub struct SpeechCli {
    /// Inject the transcripts in this file instead of running the models.
    #[arg(long, value_name = "FILE")]
    pub stub: Option<PathBuf>,
    /// What to do instead of running as the speech process.
    #[command(subcommand)]
    pub command: Option<SpeechCommand>,
}

/// The subcommands of `eileen-speech`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum SpeechCommand {
    /// Download and verify the speech models, run the self-test and time voice cloning.
    Setup,
    /// Run the machine profile's short speech benchmarks.
    Bench,
    /// Manage voices.
    #[command(subcommand)]
    Voice(VoiceCommand),
}

/// The forms of `eileen-speech voice`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum VoiceCommand {
    /// List the voices.
    List,
    /// Learn a voice from a consented recording.
    Learn,
    /// Use a voice, optionally with another engine.
    Use {
        /// The voice.
        name: String,
        /// The engine to speak it with.
        #[arg(long, value_name = "E")]
        engine: Option<String>,
    },
    /// Delete a voice and every file it left.
    Delete,
    /// Render text in a voice to a file.
    Render {
        /// The text.
        text: String,
        /// The voice.
        #[arg(long, value_name = "NAME")]
        voice: String,
        /// The file to write.
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
    },
    /// Check a file for Eileen's watermark.
    CheckMark {
        /// The file.
        file: PathBuf,
    },
}

/// Runs `eileen-speech`.
pub fn run(_cli: SpeechCli) -> anyhow::Result<()> {
    anyhow::bail!("`eileen-speech` is not built yet (WP4 builds it)")
}

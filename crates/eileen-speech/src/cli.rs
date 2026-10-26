//! The `eileen-speech` command line (BUILD.md sections 3.5 and 6.6): with no subcommand it runs
//! as the speech process; `setup`, `bench` and `voice ...` are what `eileen setup`, `eileen
//! doctor machine --bench` and `eileen voice` run. A stub until WP4 builds it: every form says it
//! is not built yet.

use std::convert::Infallible;
use std::fmt;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, Subcommand};

/// Why a command of this crate failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// The command exists, and WP4 has not built it yet.
    #[error("`{command}` is not built yet (WP4 builds it)")]
    NotBuilt {
        /// The command as the user typed it, without its arguments.
        command: &'static str,
    },
}

/// The text `voice render` speaks. Its `Debug` gives the length only, because the text is the
/// user's own words.
#[derive(Clone, PartialEq, Eq)]
pub struct RenderText(pub String);

impl fmt::Debug for RenderText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderText").field("len", &self.0.len()).finish()
    }
}

impl FromStr for RenderText {
    type Err = Infallible;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.to_owned()))
    }
}

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
        text: RenderText,
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
pub fn run(_cli: SpeechCli) -> Result<(), CliError> {
    Err(CliError::NotBuilt { command: "eileen-speech" })
}

/// The `eileen-speech` binary: parses its command line, runs it, and reports a failure on
/// standard error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match SpeechCli::try_parse() {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(&error),
        },
        Err(error) => usage_exit(&error),
    }
}

/// Prints clap's help, version or usage error and returns its exit code: 0 for help and
/// version, 2 for a usage error.
fn usage_exit(error: &clap::Error) -> ExitCode {
    let printed = error.print();
    if error.use_stderr() || printed.is_err() { ExitCode::from(2) } else { ExitCode::SUCCESS }
}

/// Reports a failed command on standard error and returns exit code 1.
fn failure(error: &impl fmt::Display) -> ExitCode {
    match writeln!(io::stderr().lock(), "Error: {error}") {
        Ok(()) | Err(_) => ExitCode::FAILURE,
    }
}

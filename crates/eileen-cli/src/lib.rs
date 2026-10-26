//! The `eileen` binary (BUILD.md sections 3.1, 3.3.15 and 3.5): the composition root that wires
//! the components through each crate's `component(cfg)`, and the command line that passes each
//! subcommand to the crate that implements it. WP0's stub wires the components whose crates
//! exist (harness or responder, audio, learning, shell) and every subcommand, each saying it is
//! not built yet; WP2 adds the speech, sidecar and HUD clients and owns it from then on.
//!
//! # Owns
//!
//! - [`Cli`] and [`Command`]: every subcommand of section 3.5, passed to the owning crate's
//!   `cli` module by [`run`].
//! - [`wiring`]: the components `eileen run` starts, with `--replay-models` swapping the
//!   harness for `eileen_replay::responder`.
//! - `--console`, `eileen about`'s credits and the framework of `setup` and `doctor` (WP2).
//!
//! # Does not own
//!
//! - What any subcommand does: each is implemented in its crate's `cli` module. The runtime
//!   (`eileen-runtime`), speech models (`eileen-speech.exe`, run through the supervisor) and app
//!   automation (`eileen-apps.exe`).
//!
//! # Invariants
//!
//! - Every subcommand of section 3.5 exists, and while its package has not landed it says it is
//!   not built yet (integration test
//!   `it::commands::every_command_of_section_3_5_says_not_built_yet`); `eileen --version` runs
//!   (`it::process::eileen_version_runs`).
//! - The shipped `eileen` binary links no ONNX Runtime and no app automation (section 3.1's
//!   table keeps `eileen-speech` and `eileen-apps` out of this crate's row; check 15 keeps `ort`
//!   in their Windows target tables).
//!
//! # Boundaries
//!
//! - Nothing calls it: it is the binary.
//! - May depend on every crate that makes up the core process (section 3.1's table), with
//!   `eileen-shell-win` under the Windows target table and `eileen-shell-linux` under the Linux
//!   one.
//! - Must not depend on `eileen-speech`, `eileen-desktop`, `eileen-apps` or `eileen-hud`, which
//!   run as their own processes, on the development-only `eileen-sim`, `eileen-e2e` and
//!   `eileen-stub-harness`, or on rmcp except through `eileen-mcp`, nor know vendor CLI flags
//!   (section 3.1); nor on any other `-win` or `-linux` crate (check 15).
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-cli`.
//! - Layers: every command form parsed and dispatched in-process, and the binary run through
//!   the supervisor.
//! - A bug here starts as a subcommand that reaches the wrong crate, or a component not wired.
//!
//! # Traps
//!
//! - `eileen voice ...` passes its arguments through to `eileen-speech.exe voice` unparsed,
//!   which parses them itself.
//!
//! # Logs and tracing
//!
//! - None yet; WP2 initialises tracing for the core process here (`eileen_trace::init`).
//!
//! # Do not change without a decision record
//!
//! - The subcommands and their flags (section 3.5).

#![forbid(unsafe_code)]

pub mod wiring;

use std::convert::Infallible;
use std::fmt;
use std::io::{self, Write as _};
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use eileen_config::cli::{AddMcpArgs, ConfigCommand};
use eileen_fleet::cli::TestArgs;
use eileen_harness::cli::{BenchBrainArgs, ClaudeCommand};
use eileen_learn::cli::{FactsCommand, NotesCommand, SkillsCommand};
use eileen_record::cli::RecordCommand;
use eileen_replay::cli::ReplayArgs;
use eileen_runtime::cli::{
    DebugCommand, DoctorCheck, IncidentCommand, JobsCommand, RunArgs, SayArgs, TraceArgs,
};
use eileen_scenarios::cli::ScenarioCommand;

/// `eileen <command>`.
#[derive(Debug, Parser)]
#[command(name = "eileen", version, about = "Eileen, a voice-first layer for Windows")]
pub struct Cli {
    /// The command.
    #[command(subcommand)]
    pub command: Command,
}

/// Every subcommand of section 3.5.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start everything.
    Run(RunArgs),
    /// Send a typed request to the running Eileen.
    Say(SayArgs),
    /// The brain benchmark, or the developer probe.
    Bench {
        /// `brain`, or nothing for the developer probe.
        #[command(subcommand)]
        what: Option<BenchCommand>,
    },
    /// Status from the running Eileen.
    Status,
    /// List, follow or cancel jobs.
    Jobs {
        /// The form.
        #[command(subcommand)]
        command: Option<JobsCommand>,
    },
    /// Version, channel and the credits.
    About,
    /// Models, checks, Start menu and sign-in entries, the install id and the machine profile.
    Setup,
    /// Check the claude CLI, audio, Resolve, desktop control or the machine profile.
    Doctor {
        /// The check, or nothing for all of them.
        #[command(subcommand)]
        check: Option<DoctorCommand>,
    },
    /// Settings.
    Config {
        /// The form.
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// Learned skills.
    Skills {
        /// The form.
        #[command(subcommand)]
        command: Option<SkillsCommand>,
    },
    /// Learned facts.
    Facts {
        /// The form.
        #[command(subcommand)]
        command: Option<FactsCommand>,
    },
    /// Route proposals and approved MCP servers.
    Apps {
        /// The form.
        #[command(subcommand)]
        command: AppsCommand,
    },
    /// Claude's notes on this PC and the held notes.
    Notes {
        /// The form.
        #[command(subcommand)]
        command: Option<NotesCommand>,
    },
    /// Voices, through `eileen-speech voice`.
    Voice {
        /// The arguments `eileen-speech voice` takes.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<VoiceArg>,
    },
    /// The scenario corpus.
    Scenario {
        /// The form.
        #[command(subcommand)]
        command: Option<ScenarioCommand>,
    },
    /// The test fleet.
    Test(TestArgs),
    /// One turn's or job's trace across processes.
    Trace(TraceArgs),
    /// The logs.
    Logs,
    /// Incidents.
    Incident {
        /// The form.
        #[command(subcommand)]
        command: Option<IncidentCommand>,
    },
    /// Recording.
    Record {
        /// The form.
        #[command(subcommand)]
        command: Option<RecordCommand>,
    },
    /// Replay a recording or an incident.
    Replay(ReplayArgs),
    /// Live internals.
    Debug {
        /// The form.
        #[command(subcommand)]
        command: Option<DebugCommand>,
    },
    /// The official Claude plugin, and draining sessions around a CLI update.
    Claude {
        /// The form.
        #[command(subcommand)]
        command: Option<ClaudeCommand>,
    },
    /// Probe the MCP tools (a developer command).
    McpProbe,
}

/// One argument `eileen voice` passes to `eileen-speech voice` unparsed. Its `Debug` gives the
/// length only, because `voice render` carries the user's own words.
#[derive(Clone, PartialEq, Eq)]
pub struct VoiceArg(pub String);

impl fmt::Debug for VoiceArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoiceArg").field("len", &self.0.len()).finish()
    }
}

impl FromStr for VoiceArg {
    type Err = Infallible;

    fn from_str(arg: &str) -> Result<Self, Self::Err> {
        Ok(Self(arg.to_owned()))
    }
}

/// The forms of `eileen bench` that name a benchmark.
#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    /// The brain benchmark (B10).
    Brain(BenchBrainArgs),
}

/// The checks of `eileen doctor`.
#[derive(Debug, Subcommand)]
pub enum DoctorCommand {
    /// The claude CLI: sign-in, the version range and the toolchains a worker sees.
    Claude,
    /// The checks the runtime runs.
    #[command(flatten)]
    Runtime(DoctorCheck),
}

/// The forms of `eileen apps`.
#[derive(Debug, Subcommand)]
pub enum AppsCommand {
    /// Route proposals waiting for a decision.
    Proposals,
    /// Approve a third-party MCP server for an app.
    AddMcp(AddMcpArgs),
}

/// Why a command failed: the error of the crate that implements it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// `eileen config` and `eileen apps add-mcp`.
    #[error(transparent)]
    Config(#[from] eileen_config::cli::CliError),
    /// `eileen test --fleet`.
    #[error(transparent)]
    Fleet(#[from] eileen_fleet::cli::CliError),
    /// `eileen bench brain`, `eileen claude` and `eileen doctor claude`.
    #[error(transparent)]
    Harness(#[from] eileen_harness::cli::CliError),
    /// `eileen skills`, `facts`, `notes` and `apps proposals`.
    #[error(transparent)]
    Learn(#[from] eileen_learn::cli::CliError),
    /// `eileen record`.
    #[error(transparent)]
    Record(#[from] eileen_record::cli::CliError),
    /// `eileen replay`.
    #[error(transparent)]
    Replay(#[from] eileen_replay::cli::CliError),
    /// The runtime's commands.
    #[error(transparent)]
    Runtime(#[from] eileen_runtime::cli::CliError),
    /// `eileen scenario`.
    #[error(transparent)]
    Scenarios(#[from] eileen_scenarios::cli::CliError),
}

/// The `eileen` binary: parses its command line, runs it, and reports a failure on standard
/// error. Returns the process's exit code, so no destructor or log flush is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => match run(cli) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => match writeln!(io::stderr().lock(), "Error: {error}") {
                Ok(()) | Err(_) => ExitCode::FAILURE,
            },
        },
        Err(error) => {
            let printed = error.print();
            if error.use_stderr() || printed.is_err() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}

/// Passes the command to the crate that implements it.
pub fn run(cli: Cli) -> Result<(), CommandError> {
    match cli.command {
        Command::Run(args) => {
            let components = wiring::components(&args);
            eileen_runtime::cli::run(args, components).map_err(CommandError::from)
        }
        Command::Say(args) => eileen_runtime::cli::say(args).map_err(CommandError::from),
        Command::Bench { what: Some(BenchCommand::Brain(args)) } => {
            eileen_harness::cli::bench_brain(args).map_err(CommandError::from)
        }
        Command::Bench { what: None } => eileen_runtime::cli::bench().map_err(CommandError::from),
        Command::Status => eileen_runtime::cli::status().map_err(CommandError::from),
        Command::Jobs { command } => eileen_runtime::cli::jobs(command).map_err(CommandError::from),
        Command::About => eileen_runtime::cli::about().map_err(CommandError::from),
        Command::Setup => eileen_runtime::cli::setup().map_err(CommandError::from),
        Command::Doctor { check: Some(DoctorCommand::Claude) } => {
            eileen_harness::cli::doctor_claude().map_err(CommandError::from)
        }
        Command::Doctor { check: Some(DoctorCommand::Runtime(check)) } => {
            eileen_runtime::cli::doctor(Some(check)).map_err(CommandError::from)
        }
        Command::Doctor { check: None } => {
            eileen_runtime::cli::doctor(None).map_err(CommandError::from)
        }
        Command::Config { command } => {
            eileen_config::cli::config(command).map_err(CommandError::from)
        }
        Command::Skills { command } => {
            eileen_learn::cli::skills(command).map_err(CommandError::from)
        }
        Command::Facts { command } => eileen_learn::cli::facts(command).map_err(CommandError::from),
        Command::Apps { command: AppsCommand::Proposals } => {
            eileen_learn::cli::apps_proposals().map_err(CommandError::from)
        }
        Command::Apps { command: AppsCommand::AddMcp(args) } => {
            eileen_config::cli::apps_add_mcp(args).map_err(CommandError::from)
        }
        Command::Notes { command } => eileen_learn::cli::notes(command).map_err(CommandError::from),
        Command::Voice { args } => {
            let args = args.into_iter().map(|arg| arg.0).collect();
            eileen_runtime::cli::voice(args).map_err(CommandError::from)
        }
        Command::Scenario { command } => {
            eileen_scenarios::cli::scenario(command).map_err(CommandError::from)
        }
        Command::Test(args) => eileen_fleet::cli::test(args).map_err(CommandError::from),
        Command::Trace(args) => eileen_runtime::cli::trace(args).map_err(CommandError::from),
        Command::Logs => eileen_runtime::cli::logs().map_err(CommandError::from),
        Command::Incident { command } => {
            eileen_runtime::cli::incident(command).map_err(CommandError::from)
        }
        Command::Record { command } => {
            eileen_record::cli::record(command).map_err(CommandError::from)
        }
        Command::Replay(args) => eileen_replay::cli::replay(args).map_err(CommandError::from),
        Command::Debug { command } => {
            eileen_runtime::cli::debug(command).map_err(CommandError::from)
        }
        Command::Claude { command } => {
            eileen_harness::cli::claude(command).map_err(CommandError::from)
        }
        Command::McpProbe => eileen_runtime::cli::mcp_probe().map_err(CommandError::from),
    }
}

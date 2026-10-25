//! The `eileen` binary (BUILD.md sections 3.1, 3.3.15 and 3.5): the composition root that wires
//! every component through its crate's `component(cfg)` and the real ports, and the command line
//! that passes each subcommand to the crate that implements it. WP0's stub wires every component
//! and every subcommand; until their packages land, each subcommand says it is not built yet.
//! WP2 owns it from then on.
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
//! - The shipped `eileen` binary links no ONNX Runtime and no app automation (section 3.1).
//!
//! # Boundaries
//!
//! - Nothing calls it: it is the binary.
//! - May depend on every crate that makes up the core process (section 3.1's table), and on
//!   `eileen-shell-win` under the Windows target table; never on `eileen-speech`,
//!   `eileen-desktop`, `eileen-apps` or `eileen-hud`, which run as their own processes.
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-cli` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-cli`).
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
        args: Vec<String>,
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

/// Passes the command to the crate that implements it.
pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Run(args) => {
            let components = wiring::components(&args);
            eileen_runtime::cli::run(args, components)
        }
        Command::Say(args) => eileen_runtime::cli::say(args),
        Command::Bench { what: Some(BenchCommand::Brain(args)) } => {
            eileen_harness::cli::bench_brain(args)
        }
        Command::Bench { what: None } => eileen_runtime::cli::bench(),
        Command::Status => eileen_runtime::cli::status(),
        Command::Jobs { command } => eileen_runtime::cli::jobs(command),
        Command::About => eileen_runtime::cli::about(),
        Command::Setup => eileen_runtime::cli::setup(),
        Command::Doctor { check: Some(DoctorCommand::Claude) } => {
            eileen_harness::cli::doctor_claude()
        }
        Command::Doctor { check: Some(DoctorCommand::Runtime(check)) } => {
            eileen_runtime::cli::doctor(Some(check))
        }
        Command::Doctor { check: None } => eileen_runtime::cli::doctor(None),
        Command::Config { command } => eileen_config::cli::config(command),
        Command::Skills { command } => eileen_learn::cli::skills(command),
        Command::Facts { command } => eileen_learn::cli::facts(command),
        Command::Apps { command: AppsCommand::Proposals } => eileen_learn::cli::apps_proposals(),
        Command::Apps { command: AppsCommand::AddMcp(args) } => {
            eileen_config::cli::apps_add_mcp(args)
        }
        Command::Notes { command } => eileen_learn::cli::notes(command),
        Command::Voice { args } => eileen_runtime::cli::voice(args),
        Command::Scenario { command } => eileen_scenarios::cli::scenario(command),
        Command::Test(args) => eileen_fleet::cli::test(args),
        Command::Trace(args) => eileen_runtime::cli::trace(args),
        Command::Logs => eileen_runtime::cli::logs(),
        Command::Incident { command } => eileen_runtime::cli::incident(command),
        Command::Record { command } => eileen_record::cli::record(command),
        Command::Replay(args) => eileen_replay::cli::replay(args),
        Command::Debug { command } => eileen_runtime::cli::debug(command),
        Command::Claude { command } => eileen_harness::cli::claude(command),
        Command::McpProbe => eileen_runtime::cli::mcp_probe(),
    }
}

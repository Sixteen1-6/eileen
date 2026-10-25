//! The commands of BUILD.md section 3.5 that `eileen-runtime` implements, which `eileen-cli`
//! dispatches here: `run`, `say`, `status`, `jobs`, `about`, `setup`, `doctor` (every check but
//! `claude`), `voice` (forwarded to `eileen-speech.exe`), `trace`, `logs`, `incident`, `debug`,
//! `mcp-probe` and `bench`. A stub until WP2 builds them: each says it is not built yet.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use eileen_contracts::ports::Component;

/// `eileen run`: start everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
#[expect(clippy::struct_excessive_bools, reason = "one field per flag of `eileen run`")]
pub struct RunArgs {
    /// Use this data directory instead of the default.
    #[arg(long, value_name = "D")]
    pub data_dir: Option<PathBuf>,
    /// Read the microphone from a WAV file instead, as `wav:<file>`.
    #[arg(long, value_name = "wav:F")]
    pub audio_source: Option<String>,
    /// Write what Eileen says to a WAV file instead of the speakers, as `capture:<file>`.
    #[arg(long, value_name = "capture:F")]
    pub audio_sink: Option<String>,
    /// Use the stub harness playing this script instead of the claude CLI.
    #[arg(long, value_name = "SCRIPT")]
    pub stub_harness: Option<PathBuf>,
    /// Answer brain and worker commands from the recordings in this folder.
    #[arg(long, value_name = "DIR")]
    pub replay_models: Option<PathBuf>,
    /// Start muted.
    #[arg(long)]
    pub muted: bool,
    /// Run without the HUD.
    #[arg(long)]
    pub no_hud: bool,
    /// Print what is heard, what Eileen says and the jobs, one line each.
    #[arg(long)]
    pub console: bool,
    /// Enable the scenario runner's tool endpoint (never in normal runs).
    #[arg(long)]
    pub test_harness: bool,
}

/// `eileen say "<text>"`: a typed request into the running instance.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct SayArgs {
    /// What to say to Eileen.
    pub text: String,
}

/// The forms of `eileen jobs`.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum JobsCommand {
    /// List the jobs.
    List,
    /// Follow one job's events.
    Tail {
        /// The job.
        id: String,
    },
    /// Cancel one job.
    Cancel {
        /// The job.
        id: String,
    },
}

/// The checks of `eileen doctor` the runtime runs; `eileen doctor claude` is the harness's.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum DoctorCheck {
    /// The microphone, the speakers and echo cancellation.
    Audio,
    /// Resolve, through `eileen-apps.exe doctor resolve`.
    Resolve {
        /// Check through this route only.
        #[arg(long, value_enum)]
        route: Option<ResolveRoute>,
    },
    /// Desktop control, through `eileen-desktop.exe`.
    Desktop,
    /// The machine profile: the facts, the plan with its reasons and the last benchmarks.
    Machine {
        /// Run the short benchmarks again.
        #[arg(long)]
        bench: bool,
    },
}

/// The routes `eileen doctor resolve --route` can name (section 2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResolveRoute {
    /// Resolve's own MCP server.
    Mcp,
    /// The Lua bridge.
    Bridge,
}

/// `eileen trace ID | last`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct TraceArgs {
    /// A turn or job id, or `last`.
    pub target: String,
}

/// The forms of `eileen incident`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum IncidentCommand {
    /// List the incidents.
    List,
    /// Summarise one incident.
    Show,
    /// Capture an incident now.
    Capture,
    /// Pack an incident to share.
    Pack,
}

/// The forms of `eileen debug`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum DebugCommand {
    /// Live counters.
    Stats,
    /// The running tasks.
    Tasks,
    /// A dump of the flight recorder.
    Dump,
}

fn not_built(command: &str) -> anyhow::Result<()> {
    anyhow::bail!("`{command}` is not built yet (WP2 builds it)")
}

/// Runs `eileen run` with the components the composition root wired.
pub fn run(_args: RunArgs, _components: Vec<Box<dyn Component>>) -> anyhow::Result<()> {
    not_built("eileen run")
}

/// Runs `eileen say`.
pub fn say(_args: SayArgs) -> anyhow::Result<()> {
    not_built("eileen say")
}

/// Runs `eileen status`.
pub fn status() -> anyhow::Result<()> {
    not_built("eileen status")
}

/// Runs `eileen jobs`.
pub fn jobs(_command: Option<JobsCommand>) -> anyhow::Result<()> {
    not_built("eileen jobs")
}

/// Runs `eileen about`.
pub fn about() -> anyhow::Result<()> {
    not_built("eileen about")
}

/// Runs `eileen setup`.
pub fn setup() -> anyhow::Result<()> {
    not_built("eileen setup")
}

/// Runs `eileen doctor`, every check or one.
pub fn doctor(_check: Option<DoctorCheck>) -> anyhow::Result<()> {
    not_built("eileen doctor")
}

/// Runs `eileen voice ...`, which passes its arguments to `eileen-speech.exe voice`.
pub fn voice(_args: Vec<String>) -> anyhow::Result<()> {
    anyhow::bail!("`eileen voice` is not built yet (WP4 builds it in eileen-speech)")
}

/// Runs `eileen trace`.
pub fn trace(_args: TraceArgs) -> anyhow::Result<()> {
    not_built("eileen trace")
}

/// Runs `eileen logs`.
pub fn logs() -> anyhow::Result<()> {
    not_built("eileen logs")
}

/// Runs `eileen incident`.
pub fn incident(_command: Option<IncidentCommand>) -> anyhow::Result<()> {
    not_built("eileen incident")
}

/// Runs `eileen debug`.
pub fn debug(_command: Option<DebugCommand>) -> anyhow::Result<()> {
    not_built("eileen debug")
}

/// Runs `eileen mcp-probe`.
pub fn mcp_probe() -> anyhow::Result<()> {
    not_built("eileen mcp-probe")
}

/// Runs `eileen bench`, the developer probe (`eileen bench brain` is the harness's).
pub fn bench() -> anyhow::Result<()> {
    not_built("eileen bench")
}

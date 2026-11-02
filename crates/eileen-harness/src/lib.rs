//! Eileen's drivers for vendor CLIs (BUILD.md sections 2.3, 2.5 and 3.3.8): Claude workers and
//! the Claude brain, the warm pool's session lifecycle, the brain benchmark, and the Codex, ACP,
//! `DeepSeek` and local routes as designed slots. The stream-json parser, each session's spawn
//! files, the CLI's location, `eileen doctor claude` and the Claude worker without the pool
//! ([`ClaudeWorker`]) are built; the brain driver, the pool and the component are stubs until
//! WP3 builds them.
//!
//! # Owns
//!
//! - [`WorkerDriver`] and [`BrainBackend`] with their enum dispatch ([`AnyWorker`], [`AnyBrain`]),
//!   [`worker_for`], [`brain_for`] and [`brain_benchmark`].
//! - The harness [`component`] (the brain's Primary, Spare and Fallback slots, the workers) and
//!   `eileen bench brain`, `eileen claude` and `eileen doctor claude` ([`cli`]).
//! - The typed stream-json parser and its mappings to `WorkerEvent` and `BrainEvent`
//!   ([`stream`]), and the stdio control protocol's lines with the interrupt Eileen writes
//!   ([`stream::control`]).
//! - [`SUBAGENT_TOOLS`], the subagent tools every session disallows in v0.1.
//! - What each Claude session spawns with ([`session`]): its command line, settings file, MCP
//!   config and environment, and the flag constants the nightly CLI drift job reads
//!   ([`session::CLAUDE_FLAGS`], [`session::CLAUDE_UNLISTED_FLAGS`]).
//! - Finding the real `claude` executable ([`locate`]): through npm's shim or the native
//!   installer's layout, with its version and the D77 floor ([`locate::MIN_CLI`]).
//! - `eileen doctor claude` ([`cli::doctor`]): the CLI found, its version, `claude auth status`
//!   by exit code, the running sessions' versions and the toolchains a worker sees.
//! - The `stream_json_line` and `control_message` fuzz targets' entries (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Decisions about jobs, sessions and approvals (`eileen-core`), starting processes
//!   (`eileen-supervisor`), build slots (`eileen-buildguard`), or the hook endpoint
//!   (`eileen-runtime`). `SessionSpec`, `JobBinding` and `BrainSlot` are contract types.
//!
//! # Invariants
//!
//! - Every vendor process starts through `eileen_supervisor`, in a Job Object
//!   (`disallowed-methods` bans `Command::new`; check 14).
//! - Only the Claude route has drivers, and every other route is `RouteUnavailable`; the Claude
//!   brain answers `NotBuilt` until WP3 builds it (unit tests
//!   `tests::only_the_claude_route_has_a_worker_driver` and
//!   `tests::the_stub_claude_brain_answers_not_built_and_measures_nothing`).
//! - A Claude worker session is bound to a job only after its `system/init` lists `eileen` as
//!   connected and reports no auto-memory folder; one that reports a folder is started once
//!   more and then refused (D77). It spawns through `eileen_supervisor` in the Claude class
//!   with exactly the session's environment; every write to it and every wait for its init or
//!   its exit has a deadline; cancel kills its Job Object and waits for the job to empty
//!   (`eileen-stub-harness::it::worker`).
//! - A stream-json line longer than `stream::MAX_LINE_BYTES` is malformed before it is parsed,
//!   an unknown `type` is `Unknown`, and a tool's input is redacted by the `Redactor` string by
//!   string before it is serialized and leaves the parser, at most 2 KB (`stream::tests`, and the
//!   `stream_json_line` fuzz target).
//! - An interrupt is written only when `system/init` offers it, at most
//!   `stream::control::MAX_OPEN_REQUESTS` are open at once, and a `control_response` settles
//!   only a request Eileen sent and has not seen answered; one with no `request_id` is
//!   malformed, and one to a request that is not open settles nothing
//!   (`stream::control::SettleError::NotOpen`; `stream::control::tests`, and the
//!   `control_message` fuzz target).
//! - Every session spawns with `--setting-sources ""`, `--strict-mcp-config`, `--permission-mode
//!   dontAsk`, the `approve` permission-prompt tool, the subagent tools disallowed, vendor memory
//!   and file checkpointing off, and its tokens only in its environment, never in a file or on
//!   the command line; the brain gets no built-in tool and no third-party MCP server, the brain
//!   and the librarian only the constructed allowlist, a worker the user's environment minus the
//!   denylist (sections 2.3, 2.5 and 2.12; `session::tests`). Every path a session is given is
//!   absolute and none lies under a `.claude` folder. A third-party MCP server reaches only the
//!   job kinds its config entry names.
//! - The CLI found is npm's package binary or the native installer's, never the Claude desktop
//!   app's bundled copy or another `claude` on `PATH`, and never lies under a `.claude` folder;
//!   a `PATH` folder under one is refused before anything in it is looked at
//!   (`locate::tests::the_desktop_app_copy_is_refused`,
//!   `locate::tests::a_bare_claude_exe_in_another_folder_is_passed_over`,
//!   `locate::tests::a_folder_under_the_users_claude_folder_is_never_searched`).
//! - `eileen doctor claude` reads `claude auth status` by its exit code only, never its output,
//!   and any exit code but 0 gets the `claude auth login` suggestion with nothing else started
//!   (`cli::doctor::tests::a_signed_out_cli_gets_the_login_suggestion_and_nothing_else_runs`,
//!   `cli::doctor::tests::any_non_zero_exit_of_auth_status_gets_the_login_suggestion`); every
//!   probe runs as a tool in a coding worker's environment, built by the same code as a
//!   worker's, with a deadline, and the CLI and the toolchains are looked for on that
//!   environment's `PATH`
//!   (`cli::doctor::tests::auth_status_runs_in_the_worker_environment_with_a_deadline`,
//!   `cli::doctor::tests::the_toolchains_are_looked_for_on_the_path_a_worker_gets`, and the
//!   version and vswhere probes in their own tests).
//! - Denials are read from the stream, and an `approve` call that errored, timed out or lost its
//!   server counts as one when the guard asked about the call (section 2.5;
//!   `stream::tests::failed_approve_calls_count_as_denials` and
//!   `stream::tests::a_failed_call_the_guard_did_not_ask_about_is_no_denial`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (the component) and `eileen-stub-harness`.
//! - May depend on `eileen-supervisor`, `eileen-buildguard`, `eileen-rt`, `eileen-contracts`,
//!   `eileen-stdx` and `eileen-trace`, and on `tokio` for the doctor's deadlines.
//! - Must not depend on `eileen-core` or `eileen-runtime` (the core decides), `eileen-mcp` or
//!   rmcp (section 3.1), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: the vendors' CLI flags and protocols (stream-json for Claude), which only this
//!   crate knows (section 3.1); Eileen's side of them is contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-harness`; the worker's runs against the stub:
//!   `cargo xtask test -p eileen-stub-harness worker`.
//! - Layers: unit tests for the parser and the flags, with insta snapshots of each session kind's
//!   command line, settings file and MCP config (the parser's against the lines CLI 2.1.304
//!   wrote in WP3's approval contract test, copied under `tests/fixtures/wp3-approval/`), runs
//!   against `eileen-stub-harness`, the CLI's location against fake npm and native layouts in a
//!   temporary folder, `ClaudeWorker` against the stub in `eileen-stub-harness::it::worker`
//!   (there because Cargo names the stub's binary only to its own package's tests), the doctor
//!   against a scripted probe runner, its supervised runner against
//!   this test binary run again as a chatty child
//!   (`cli::doctor::tests::the_supervised_runner_reads_a_real_child_to_its_exit`: exit code,
//!   the 4 KB cap on standard output, standard error drained), the two fuzz targets with their
//!   corpora replayed in `it::fuzz_corpus`, and `real_claude::` tests on the owner's PC. The
//!   kill when a probe's deadline passes is dropping the supervised child, which
//!   `eileen-supervisor`'s own tests cover (dropping a child kills its job), since testing it
//!   here would need a real-time wait.
//! - A bug here starts as a stream-json line that became `Unknown`, or a session whose init
//!   lists a server it should not.
//!
//! # Traps
//!
//! - The real executable is resolved through the npm shim or the native layout, never the
//!   desktop app's copy. npm writes `claude.cmd`, `claude.ps1` and a shell script side by side;
//!   the `.cmd` and `.ps1` name the package's `bin\claude.exe` relative to their own folder
//!   (`%dp0%`, `$basedir`), and an older package's shim runs `cli.js` through node instead,
//!   which is no executable Eileen can supervise.
//! - `claude auth status` prints the account's email and organisation, so the doctor reads its
//!   exit code only: standard output is null, and standard error is drained and dropped
//!   (`ChildSpec::raw_stderr`), since the supervisor would otherwise forward each line into
//!   tracing. Only exit code 0 is recorded as signed in, so every other code is read as signed
//!   out.
//! - On Windows tokio reads each child pipe on a blocking-pool thread, and a probe reads its
//!   standard output while draining its standard error, so the doctor's runtime needs two
//!   blocking threads (`cli::doctor::BLOCKING_THREADS`); with one, a child that fills its
//!   standard error stalls until the deadline.
//! - Windows links count too: a `claude.exe` in a `PATH` folder can be a symbolic link or sit
//!   behind a junction, so `locate` follows each link by reading it, checking every target by
//!   name before anything there is looked at, rather than canonicalizing, which would open
//!   whatever the link leads to.
//! - CI never holds vendor credentials, and no code path opens a vendor credential file.
//! - The CLI writes `system/init` again at the start of every user turn, and `system/init`
//!   lists the subagent tool as `Task` while the model calls it `Agent`.
//! - CLI 2.1.316 writes nothing on stdout, `system/init` included, until the first user line
//!   arrives, while `ClaudeWorker::start` waits for init before a job is bound, as section
//!   3.3.8 has it. Against the real CLI a start therefore runs into its init deadline, until
//!   the amendment that settles how a session shows it is ready before its job changes the
//!   start (the CLI does answer the `initialize` and `mcp_status` control requests before any
//!   user line; `docs/wp/WP3-harness.md`, "Surprises").
//! - Not every denial looks like one: an `approve` call that fails reaches the model as a tool
//!   error and is missing from `result.permission_denials`, so the parser reads it from the
//!   tool result's text (`stream::DenialKind::ApproveFailed`). The CLI wraps any tool that
//!   failed in the same text, so only a call the guard asked about counts.
//! - Windows variable names ignore case and Linux ones do not: `HostEnv::names_ignore_case`
//!   carries the host's rule, so on Windows `Path` is `PATH` and a session variable replaces
//!   every spelling of its name, while on Linux `http_proxy` and `HTTP_PROXY` both survive.
//! - The settings file and the MCP config name the tokens (`$EILEEN_HOOK_TOKEN` through
//!   `allowedEnvVars`, `${EILEEN_SHIM_TOKEN}`), and the CLI fills them from the session's
//!   environment; writing a token into either file puts it on disk.
//! - The CLI expands `${VAR}` in every MCP server's command, arguments and `env` from its own
//!   environment, which holds both tokens, so a third-party server whose command or arguments
//!   hold a `$` is refused (`session::SessionError::ExpandableMcpArg`); blanking the tokens in
//!   its `env` does not stop an expansion in its arguments.
//! - Redact a JSON value's strings before serializing it, never its text: serialized, a newline
//!   before a token is the `n` of `\n`, after which the `Redactor` sees no word start.
//! - One pass of the `Redactor` can leave a token: right after another match it has no word
//!   start until that match is replaced. The parser redacts each string until it settles.
//! - A `rate_limit_event` reports one window, while `PlanUsage` holds both with the nearer
//!   reset, so the events pass through `stream::PlanWindows` before they become `RateLimit`.
//!
//! # Logs and tracing
//!
//! - None yet; WP3 declares its spans in `eileen_trace::names`. Tool inputs are redacted in the
//!   parser, so no later log sees them whole.
//!
//! # Do not change without a decision record
//!
//! - The flags every Claude session gets (section 2.5), the approval route, and the subagent
//!   tools staying disallowed in v0.1.

#![forbid(unsafe_code)]

mod brain;
pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;
pub mod locate;
pub mod session;
pub mod stream;
mod worker;

use std::path::PathBuf;
use std::time::Duration;

pub use brain::{
    AnyBrain, BrainBackend, BrainBenchResult, BrainSuite, ClaudeBrain, CodexBrain, DeepSeekBrain,
    LocalBrain, brain_benchmark, brain_for,
};
use eileen_contracts::config::{ApprovalsConfig, McpConfig};
use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::jobs::Route;
use eileen_contracts::ports::{Component, IdleComponent};
pub use worker::{
    AcpWorker, AnyWorker, ClaudeWorker, CodexWorker, WorkerDriver, WorkerSession, worker_for,
};

/// The subagent tools, which no session gets in v0.1 (section 2.5): `--disallowedTools` names
/// them and `--tools` never lists them. Both names, because `system/init` lists the tool as
/// `Task` and the model calls it `Agent` (WP3's approval contract test, section 2.5).
pub const SUBAGENT_TOOLS: [&str; 2] = ["Task", "Agent"];

/// How long a Claude session may take from its spawn to `system/init` by default. A worker's
/// cold start under load took 30 to 111 s (section 0).
pub const INIT_DEADLINE: Duration = Duration::from_secs(120);

/// What the harness reads at start. WP3 adds the rest of its fields (the model aliases, the
/// pool sizes) as it builds the drivers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessConfig {
    /// Eileen's data folder; each session's files go under `run/<session>/` in it.
    pub data_dir: PathBuf,
    /// `approvals.hook_hold_s`: how long an `approve` call is held while the user decides,
    /// seconds. `MCP_TOOL_TIMEOUT` is this plus 30 s.
    pub hook_hold_s: u16,
    /// The absolute path of `eileen-shim` in Eileen's install folder, which each session's MCP
    /// config starts. Never a bare name: the CLI would search for it, from the worker's folder
    /// too.
    pub shim_exe: PathBuf,
    /// The approved third-party MCP servers: which job kinds get each, and its rung.
    pub mcp: McpConfig,
    /// The absolute path of the real `claude` executable, which the runtime fills from
    /// [`locate`]. Empty until then, and a worker refuses to start without it.
    pub claude_exe: PathBuf,
    /// What the runtime read from the user's machine, from which each session's environment is
    /// built (section 2.12).
    pub host: session::HostEnv,
    /// How long a session may take from its spawn to `system/init` ([`INIT_DEADLINE`]).
    pub init_deadline: Duration,
}

/// A config with no data folder, no shim and no CLI, for the stub drivers. `session::session_files`
/// refuses it, so a session never starts from it by accident: the runtime fills both paths
/// before the first spawn.
impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::new(),
            hook_hold_s: ApprovalsConfig::default().hook_hold_s,
            shim_exe: PathBuf::new(),
            mcp: McpConfig::default(),
            claude_exe: PathBuf::new(),
            host: session::HostEnv::default(),
            init_deadline: INIT_DEADLINE,
        }
    }
}

/// What went wrong in a driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    /// The route is not built in this version.
    #[error("the {} route is not available", .route.display_name())]
    RouteUnavailable {
        /// The route asked for.
        route: Route,
    },
    /// The route is built in this version, and WP3 has not built its driver yet.
    #[error("the {} route is not built yet (WP3 builds it)", .route.display_name())]
    NotBuilt {
        /// The route asked for.
        route: Route,
    },
    /// The driver does not carry out this command yet: the pool, worktrees and snapshots come
    /// later in WP3.
    #[error("the driver does not carry out {command} yet")]
    Unsupported {
        /// The command.
        command: &'static str,
    },
    /// `HarnessConfig::claude_exe` is not an absolute path: the CLI was not located.
    #[error("the claude executable has not been located")]
    NoExecutable,
    /// The session's files could not be built.
    #[error(transparent)]
    Session(#[from] session::SessionError),
    /// The session's run folder could not be written.
    #[error("the session's {what} could not be written: {kind}")]
    Files {
        /// What was being written.
        what: &'static str,
        /// The OS's error.
        kind: std::io::ErrorKind,
    },
    /// The CLI could not be started.
    #[error("the claude CLI could not be started: {kind}")]
    Spawn {
        /// The OS's error.
        kind: std::io::ErrorKind,
    },
    /// The session wrote no `system/init` within the deadline.
    #[error("the session wrote no system/init within {secs} s")]
    InitDeadline {
        /// The deadline, seconds.
        secs: u64,
    },
    /// The session's process ended before its `system/init`.
    #[error("the session ended before its system/init (exit code {code:?})")]
    EndedBeforeInit {
        /// Its exit code, when it had one.
        code: Option<i32>,
    },
    /// The session's `system/init` does not list `eileen` as connected, so it is never bound
    /// (section 2.5).
    #[error("the session's system/init does not list eileen as connected")]
    EileenNotConnected,
    /// The session's `system/init` reported an auto-memory folder on its first start and on its
    /// restart, though every session starts with vendor memory off (D77), so it is never bound.
    #[error("the session reported an auto-memory folder with vendor memory off")]
    MemoryPathReported,
    /// A session is already started on this driver.
    #[error("a session is already started")]
    AlreadyStarted,
    /// There is no session: none was started, or it ended or was cancelled.
    #[error("there is no session")]
    NoSession,
    /// The session already has its job.
    #[error("the session already has its job")]
    AlreadyBound,
    /// A user turn came before the job's first line.
    #[error("the session has no job yet")]
    NotBound,
    /// The interrupt was not written.
    #[error(transparent)]
    Interrupt(#[from] stream::control::SendError),
    /// A write to the session's stdin failed.
    #[error("the session's stdin could not be written: {kind}")]
    Write {
        /// The OS's error.
        kind: std::io::ErrorKind,
    },
    /// A write to the session's stdin did not finish within its deadline.
    #[error("a write to the session's stdin passed its deadline")]
    WriteDeadline,
}

/// The harness component: the brain's slots and the workers. The stub accepts the brain and
/// worker families and does nothing.
#[must_use]
pub fn component(_cfg: &HarnessConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("harness", &[EffectFamily::Brain, EffectFamily::Worker]))
}

#[cfg(test)]
mod tests;

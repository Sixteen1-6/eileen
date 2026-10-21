//! Shared enums and job types (BUILD.md section 3.3.2).

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{
    ApprovalId, ConnectorId, JobId, Personal, TurnId, Untrusted, WallTime, WorkerSessionId,
};
use crate::intent::SlotValues;
use crate::learn::SkillRef;
use crate::schema::Registry;

/// A route: which vendor harness or model family does a piece of work. Workers are named only
/// by route.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    /// Claude, through the user's own signed-in `claude` CLI. The only route built in v0.1.
    Claude,
    /// The Codex app server (a designed slot).
    Codex,
    /// Grok over ACP (a designed slot).
    Grok,
    /// `DeepSeek` (a designed slot; its API key is the one key Eileen would store).
    DeepSeek,
    /// A local model (a designed slot).
    Local,
}

impl Route {
    /// Every route, in declaration order.
    pub const ALL: [Self; 5] = [Self::Claude, Self::Codex, Self::Grok, Self::DeepSeek, Self::Local];

    /// The only place a worker's user-facing name is defined.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Grok => "Grok",
            Self::DeepSeek => "DeepSeek",
            Self::Local => "Local model",
        }
    }

    /// Whether v0.1 builds this route: true only for Claude.
    #[must_use]
    pub const fn built_in_v0_1(self) -> bool {
        matches!(self, Self::Claude)
    }
}

/// Permission tier of a call (section 2.12), ordered from harmless to refused.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Tier 0: reads that change nothing.
    Observe = 0,
    /// Tier 1: changes that are easy to undo.
    Reversible = 1,
    /// Tier 2: changes that need the user's own approval.
    Consequential = 2,
    /// Tier 3: never allowed.
    Blocked = 3,
}

impl Tier {
    /// The tier's number, 0 to 3.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Observe => 0,
            Self::Reversible => 1,
            Self::Consequential => 2,
            Self::Blocked => 3,
        }
    }
}

/// How much thought a request gets.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    /// Answered at once, by the fast path or the brain's first words.
    Instant,
    /// A short job.
    Quick,
    /// A job given room to think.
    Considered,
    /// The deepest model and the most time.
    Deep,
}

/// What kind of work a job is.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Code changes in a repository worktree.
    Coding,
    /// Driving the desktop through Eileen's desktop and apps tools.
    ComputerUse,
    /// A general task with the desktop and apps toolsets.
    General,
    /// Research on the web.
    Research,
    /// A deep answer to a question, in the background of a fast one.
    DeepAnswer,
    /// The learning loop's librarian.
    Librarian,
    /// A learned recipe replayed without a model.
    Replay,
    /// Continuing an earlier Claude session.
    ContinueSession,
    /// A task inside one app through its adapter.
    AppTask,
    /// A fast-path command or a brain turn that ran `act` tools: no session, silent, ends with
    /// its turn.
    Act,
    /// Reserved for the v0.2 Colab connector; refused in v0.1.
    MlProject,
}

impl JobKind {
    /// Every job kind, in declaration order.
    pub const ALL: [Self; 11] = [
        Self::Coding,
        Self::ComputerUse,
        Self::General,
        Self::Research,
        Self::DeepAnswer,
        Self::Librarian,
        Self::Replay,
        Self::ContinueSession,
        Self::AppTask,
        Self::Act,
        Self::MlProject,
    ];

    /// Whether v0.1 runs this kind: false only for `MlProject`, which the dispatcher refuses
    /// with the user error `JobKindUnavailable` (section 2.5).
    #[must_use]
    pub const fn built_in_v0_1(self) -> bool {
        !matches!(self, Self::MlProject)
    }
}

/// Where a job is in its life.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Accepted, waiting for memory, a build slot or the plan.
    Queued,
    /// A worker or replay is on it.
    Running,
    /// Waiting on something outside the worker.
    Blocked,
    /// Waiting for the user to approve a call.
    AwaitingApproval,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Stopped by the user.
    Cancelled,
    /// Stopped by a core restart while it ran.
    Interrupted,
}

impl JobStatus {
    /// True once the job can no longer change state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted)
    }
}

/// How a fan-out's workers share a task.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FanoutMode {
    /// Every worker tries the whole task; the best result wins.
    Compete,
    /// Each worker takes one subtask.
    Split,
}

/// A set of Eileen tools given to one role (section 3.3.9).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Toolset {
    /// The brain's tools.
    Brain,
    /// Deterministic actions for the brain's addressed turns.
    Act,
    /// Tools every worker gets.
    Worker,
    /// The desktop tools for computer-use and general workers.
    Desktop,
    /// The app tools for computer-use and general workers.
    Apps,
    /// The librarian's tools.
    Learn,
    /// The owner's own Claude sessions, through the plugin.
    Session,
    /// Read-only developer tools for coding workers in an Eileen worktree.
    Dev,
}

/// Where a turn came from.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TurnSource {
    /// The microphone.
    Voice,
    /// Typing in Eileen's own UI.
    Typed,
    /// A HUD click.
    Hud,
    /// A hotkey.
    Hotkey,
    /// The tray menu.
    Tray,
    /// `eileen say` and other command-line input.
    Cli,
    /// The owner's own Claude session, through the plugin.
    ClaudeSession,
    /// A job event the core announces.
    JobEvent,
    /// A scenario runner.
    Scenario,
    /// The first run's fixed demo requests.
    FirstRun,
    /// A connector other than the PC (later: a phone, the home).
    Connector(ConnectorId),
}

/// Whether a turn may use the brain's full tools.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TurnAuthority {
    /// Began from addressed user input (voice after the gate, typing in Eileen's UI, the HUD)
    /// or from the first-run demo's fixed requests: full brain tools.
    Addressed,
    /// Everything else (job events, announcements, `Cli`, `ClaudeSession`): read-only brain
    /// tools.
    Ambient,
}

/// A class of pooled worker session.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PoolClass {
    /// A warm computer-use session.
    ComputerUse,
    /// A warm deep-answer session.
    Deep,
}

/// Who made a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallOrigin {
    /// The brain, in a turn.
    Brain {
        /// The turn.
        turn: TurnId,
        /// Whether the turn was addressed.
        authority: TurnAuthority,
    },
    /// A worker session on a job.
    Worker {
        /// The job.
        job: JobId,
        /// The session.
        session: WorkerSessionId,
        /// The session's route.
        route: Route,
    },
    /// A replayed recipe.
    Replay {
        /// The replay job.
        job: JobId,
        /// The skill being replayed.
        skill: SkillRef,
    },
    /// The deterministic fast path.
    FastPath {
        /// The turn it answered.
        turn: TurnId,
    },
    /// The owner's own Claude session through the plugin.
    Session,
    /// Eileen's own bookkeeping calls (checks, the intent highlight, media pause during
    /// approvals).
    Core,
}

/// What the brain or a skill asks for when it starts a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobSpec {
    /// The kind of job.
    pub kind: JobKind,
    /// The task, up to 8,000 characters.
    pub task: String,
    /// A hint for the job's spoken name.
    pub title_hint: Option<String>,
    /// How much thought it gets.
    pub depth: Depth,
    /// The route; `None` lets policy decide.
    pub route: Option<Route>,
    /// A name from `routing.models` or a full id; `None` lets policy decide. The harness
    /// resolves it to the full id it passes and records.
    pub model: Option<String>,
    /// How many workers, 1 to 8.
    pub fanout: u8,
    /// How the workers share the task.
    pub mode: FanoutMode,
    /// `Split` mode: one subtask per worker.
    pub subtasks: Vec<String>,
    /// `DeepAnswer`: at most 4 subquestions.
    pub subquestions: Vec<String>,
    /// A repository name or absolute path.
    pub repo: Option<Personal<String>>,
    /// `ContinueSession` only: the session to resume.
    pub resume_session: Option<String>,
    /// A learned skill to follow (`Replay`: the recipe to run).
    pub skill: Option<SkillRef>,
    /// `Replay`: the slots of the `[match]` block; with any other skill job, the slots the use
    /// is attributed with (validated against the skill's slot specs).
    pub slots: SlotValues,
    /// `DeepAnswer`: a short topic key ("food near me") that scopes feedback facts and the
    /// answers cache.
    pub topic: Option<String>,
    /// A `DeepAnswer` started by anticipation; results go to the cache and the HUD only.
    pub anticipated: bool,
    /// The addressed turn that caused it; checked by the dispatcher.
    pub authorized_by: TurnId,
    /// Fan-out children point at their parent.
    pub parent: Option<JobId>,
    /// The user opted this job in to Claude in Chrome.
    pub chrome: bool,
}

/// A job as the registry, the HUD and the store see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    /// Its id.
    pub id: JobId,
    /// Its spoken name.
    pub name: String,
    /// Its name as a slug, for folders and branches.
    pub slug: String,
    /// "the second one": its ordinal among live jobs, when it has one.
    pub ordinal: Option<u8>,
    /// What was asked for.
    pub spec: JobSpec,
    /// Where it is.
    pub status: JobStatus,
    /// The route that runs it.
    pub route: Route,
    /// The resolved model id.
    pub model: String,
    /// The worker session bound to it.
    pub session: Option<WorkerSessionId>,
    /// When it was created.
    pub created: WallTime,
    /// When it last changed.
    pub updated: WallTime,
    /// The last progress line.
    pub last_progress: Option<Untrusted<String>>,
    /// Progress in percent, when known.
    pub progress_pct: Option<u8>,
    /// Drives announcement order.
    pub salience_pct: u8,
    /// Its worktree folder.
    pub worktree: Option<Personal<String>>,
    /// Its branch.
    pub branch: Option<String>,
    /// The snapshot it can be restored to.
    pub snapshot_ref: Option<String>,
    /// What it has cost so far.
    pub cost: Cost,
    /// The grants given to it.
    pub grants: Vec<Grant>,
    /// What it has read from outside.
    pub taint: Taint,
    /// Whether the user is focused on it.
    pub focus: bool,
    /// The spoken override that routed it, verbatim.
    pub override_words: Option<Personal<String>>,
    /// When an open approval question expires.
    pub approval_deadline: Option<WallTime>,
}

/// What a turn or job cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cost {
    /// Wall time, ms.
    pub duration_ms: u64,
    /// Time in the vendor's API, ms.
    pub api_ms: u64,
    /// Model turns.
    pub model_turns: u32,
    /// Input tokens.
    pub tokens_in: u64,
    /// Output tokens.
    pub tokens_out: u64,
    /// Input tokens read from the cache.
    pub tokens_cache_read: u64,
}

/// A permission the user granted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Grant {
    /// What it matches.
    pub matcher: GrantMatcher,
    /// The highest tier it allows.
    pub tier: Tier,
    /// How long it lasts.
    pub scope: GrantScope,
    /// The approval that granted it.
    pub granted_by: ApprovalId,
    /// What the user was shown.
    pub summary: String,
}

/// What a grant matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantMatcher {
    /// The normalised call that was shown to the user.
    ExactCall {
        /// The tool.
        tool: String,
        /// The digest of the normalised input.
        input_digest: String,
    },
    /// A tool pattern; tier 1 only.
    ToolPattern {
        /// The pattern.
        pattern: String,
    },
    /// "May Claude control Resolve?"
    DesktopApp {
        /// The process name.
        process: String,
    },
}

/// How long a grant lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    /// Until the job ends.
    ThisJob,
    /// Always: tier 1, or tier 2 with a logged decision id.
    Always,
}

/// The one set of observed sources (section 2.12): a job's taint and the learning loop's tainted
/// steps both use it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ObservedSource {
    /// A UI Automation tree.
    Tree,
    /// A screenshot.
    Screen,
    /// The user's selection.
    Selection,
    /// A web page or a web service.
    Web,
    /// A file.
    File,
    /// A notification.
    Notification,
}

/// What a job has read from outside Eileen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Taint {
    /// The observed sources the job read.
    pub sources: BTreeSet<ObservedSource>,
    /// Read outside the job's folder and its skill snapshot, or started so because the
    /// dispatching turn held ledger lines or the task repeats an out-of-scope fact.
    pub private_read: bool,
}

impl Taint {
    /// True once the job has read anything from outside.
    #[must_use]
    pub fn untrusted_seen(&self) -> bool {
        !self.sources.is_empty()
    }
}

/// The user's plan usage, from the vendor's rate-limit events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanUsage {
    /// The five-hour window, in percent.
    pub five_hour_pct: Option<u8>,
    /// The seven-day window, in percent.
    pub seven_day_pct: Option<u8>,
    /// When the nearer window resets.
    pub resets_at: Option<WallTime>,
}

/// The machine's memory, sampled every 5 s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryPressure {
    /// `GlobalMemoryStatusEx` available physical memory (free plus standby), MB.
    pub available_mb: u32,
    /// The commit limit minus the commit charge, MB.
    pub commit_headroom_mb: u32,
    /// The hard-fault paging rate over the last 30 s: the PDH counter `\Memory\Pages Input/sec`
    /// (pages, mapped-file reads included) times the page size, KB/s; judged against a baseline.
    pub paging_kb_s: u32,
    /// The effective `memory.max_paging_kb_s`: the setting, or for `auto` the value the runtime
    /// computes once an hour from the stored 7-day averages.
    pub paging_limit_kb_s: u32,
    /// The biggest process that is not Eileen's, and its working set in MB.
    pub largest_process: Option<(Personal<String>, u32)>,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Route>();
    r.add::<Tier>();
    r.add::<Depth>();
    r.add::<JobKind>();
    r.add::<JobStatus>();
    r.add::<FanoutMode>();
    r.add::<Toolset>();
    r.add::<TurnSource>();
    r.add::<TurnAuthority>();
    r.add::<PoolClass>();
    r.add::<CallOrigin>();
    r.add::<JobSpec>();
    r.add::<Job>();
    r.add::<Cost>();
    r.add::<Grant>();
    r.add::<GrantMatcher>();
    r.add::<GrantScope>();
    r.add::<ObservedSource>();
    r.add::<Taint>();
    r.add::<PlanUsage>();
    r.add::<MemoryPressure>();
}

#[cfg(test)]
mod tests {
    use super::{JobKind, JobStatus, Route, Tier};

    #[test]
    fn route_names_are_defined_once_and_only_claude_is_built() {
        let names: Vec<_> = Route::ALL.iter().map(|r| r.display_name()).collect();
        assert_eq!(names, ["Claude", "Codex", "Grok", "DeepSeek", "Local model"], "names");
        let built: Vec<_> = Route::ALL.into_iter().filter(|r| r.built_in_v0_1()).collect();
        assert_eq!(built, [Route::Claude], "built in v0.1");
        assert_eq!(serde_json::to_string(&Route::DeepSeek).unwrap(), "\"deepseek\"", "wire name");
    }

    #[test]
    fn only_ml_project_is_reserved() {
        let reserved: Vec<_> = JobKind::ALL.into_iter().filter(|k| !k.built_in_v0_1()).collect();
        assert_eq!(reserved, [JobKind::MlProject], "reserved kinds");
        assert_eq!(serde_json::to_string(&JobKind::MlProject).unwrap(), "\"ml_project\"", "wire");
    }

    #[test]
    fn tiers_order_by_their_level() {
        assert!(Tier::Observe < Tier::Reversible && Tier::Consequential < Tier::Blocked, "order");
        assert_eq!(Tier::Consequential.level(), 2, "level");
        assert!(JobStatus::Interrupted.is_terminal() && !JobStatus::Blocked.is_terminal(), "end");
    }
}

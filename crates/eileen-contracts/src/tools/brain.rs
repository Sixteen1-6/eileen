//! The brain toolset's arguments and results.
//!
//! Provisional: WP2 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ToolInput, input};
use crate::config::SettingChange;
use crate::ids::{JobId, ProposalId, SkillId, TurnId};
use crate::jobs::{Depth, FanoutMode, JobKind, Route};
use crate::learn::ForgetTarget;
use crate::record::BlobRef;
use crate::schema::Registry;

const MODULE: &str = "tools/brain";

/// `jobs_dispatch`: start one job, or a fan-out. Tier 1 (asks first when plan usage is high and
/// `fanout` is above 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobsDispatch {
    /// The kind of job.
    pub kind: JobKind,
    /// The task, up to 8,000 characters.
    pub task: String,
    /// A hint for the job's spoken name.
    pub title_hint: Option<String>,
    /// How much thought it gets.
    pub depth: Option<Depth>,
    /// How many workers, 1 to 8.
    pub fanout: Option<u8>,
    /// How the workers share the task.
    pub mode: Option<FanoutMode>,
    /// `Split` mode: one subtask per worker.
    #[serde(default)]
    pub subtasks: Vec<String>,
    /// `DeepAnswer`: at most 4 subquestions.
    #[serde(default)]
    pub subquestions: Vec<String>,
    /// A repository name or absolute path.
    #[schemars(extend("x-eileen-path" = true))]
    pub repo: Option<String>,
    /// The route; leave it out to let policy decide.
    pub route: Option<Route>,
    /// A model name from `routing.models`, or a full id.
    pub model: Option<String>,
    /// A learned skill to follow.
    pub skill: Option<SkillId>,
    /// Kind `Replay`: must equal the `[match]` block's skill and slots; any other kind with a
    /// skill: the slots the use is attributed with, validated against the skill's slot specs.
    pub slots: Option<Value>,
    /// `DeepAnswer` only: a short topic key.
    pub topic: Option<String>,
    /// `DeepAnswer` only: started by anticipation.
    #[serde(default)]
    pub anticipated: bool,
}

/// What `jobs_dispatch` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobsDispatchOutput {
    /// The jobs it started.
    pub jobs: Vec<DispatchedJob>,
    /// What Eileen says to acknowledge them.
    pub ack: String,
}

/// One job `jobs_dispatch` started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DispatchedJob {
    /// The job.
    pub job: JobId,
    /// Its spoken name.
    pub name: String,
    /// Its ordinal.
    pub ordinal: Option<u8>,
    /// Whether it waits for admission.
    pub queued: bool,
}

/// `jobs_status`. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobsStatus {
    /// One job; leave it out for every live job.
    pub job: Option<JobRef>,
}

/// How a tool names a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobRef {
    /// By id.
    Id(JobId),
    /// By slug.
    Slug(String),
    /// By spoken name.
    Name(String),
    /// "the second one".
    Ordinal(u8),
}

/// `jobs_control`. Tier 1; `Keep` with cleanup tier 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobsControl {
    /// The job.
    pub job: JobRef,
    /// What to do.
    pub action: JobAction,
}

/// What `jobs_control` does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobAction {
    /// Stop it.
    Cancel,
    /// Interrupt its current turn.
    Interrupt,
    /// Interrupt it and give it new words.
    Steer {
        /// The words.
        text: String,
    },
    /// Move it to another route, model or depth.
    Reroute {
        /// The route.
        route: Option<Route>,
        /// The model.
        model: Option<String>,
        /// The depth.
        depth: Option<Depth>,
    },
    /// Focus on it.
    Focus,
    /// Show its changes.
    ShowChanges,
    /// Keep its changes.
    Keep,
    /// Undo its changes.
    Undo,
}

/// `desktop_state`. Tier 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopStateArgs {
    /// How much to return.
    pub detail: Detail,
    /// Only changes after this sequence number.
    pub since_seq: Option<u64>,
}

/// How much `desktop_state` returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// A short summary.
    #[default]
    Summary,
    /// Everything.
    Full,
}

/// `desktop_look`: a screenshot for the brain. Tier 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopLook {
    /// What to look at.
    pub target: LookTarget,
    /// The longest edge, pixels; 1280 by default.
    pub max_edge_px: Option<u32>,
}

/// What `desktop_look` looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LookTarget {
    /// Every monitor.
    Screen,
    /// The foreground window.
    Foreground,
    /// One window.
    Window {
        /// The window.
        hwnd: i64,
    },
}

/// `capabilities`: what Eileen can do here. Tier 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "CapabilitiesArgs")]
pub struct Capabilities {
    /// How wide to look.
    pub scope: CapabilityScope,
}

/// How wide `capabilities` looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    /// The focused app.
    #[default]
    FocusedApp,
    /// Everything.
    Everything,
}

/// `settings_get`. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsGet {
    /// The keys; empty for a short summary.
    #[serde(default)]
    pub keys: Vec<String>,
}

/// `settings_propose`: returns a `Proposal`. Tier 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsPropose {
    /// The changes.
    pub changes: Vec<SettingChange>,
    /// What the user said.
    pub utterance: String,
}

/// `settings_apply`. The tier follows the proposal's risk; `Safety` keys need a `UserInput`
/// approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsApply {
    /// The proposal.
    pub proposal: ProposalId,
}

/// `settings_undo`. Tier 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsUndo {
    /// How many versions back.
    pub steps: Option<u8>,
    /// Or straight to this version.
    pub to_version: Option<u64>,
}

/// `learned_query`. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnedQuery {
    /// A topic.
    pub about: Option<String>,
    /// Skills, facts or both.
    pub kind: Option<LearnedKind>,
    /// Only since this time, as the user said it.
    pub since: Option<String>,
    /// At most this many.
    pub limit: Option<u8>,
}

/// What `learned_query` looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnedKind {
    /// Learned skills.
    Skills,
    /// Facts about the user.
    Facts,
    /// Both.
    Both,
}

/// `learned_forget`. Tier 2. "forget that I `<text>`" maps to `About { text }` through the
/// grammar; the brain uses it for other phrasings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnedForget {
    /// What to forget.
    pub target: ForgetTarget,
}

/// `learned_correct`. Tier 1, addressed turns only. The instruction is stored with provenance
/// `User` only if it is a contiguous span of the turn's normalised transcript, else after a
/// `UserInput` yes as `Approved`; a skill target queues `LearnCmd::Repair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnedCorrect {
    /// What to correct.
    pub target: CorrectTarget,
    /// How.
    pub instruction: String,
}

/// What `learned_correct` corrects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorrectTarget {
    /// A fact.
    Fact {
        /// The fact.
        id: String,
    },
    /// A skill.
    Skill {
        /// The skill.
        id: SkillId,
    },
}

/// `ledger_search`. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LedgerSearch {
    /// What to look for.
    pub query: String,
    /// How far back, hours.
    pub since_hours: Option<u32>,
    /// At most this many.
    pub limit: Option<u8>,
}

/// `scenario_save`. Tier 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioSave {
    /// What to save.
    pub which: SaveTarget,
    /// Save it as a failure.
    #[serde(default)]
    pub failed: bool,
    /// What should have happened.
    pub expected: Option<String>,
}

/// What `scenario_save` saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveTarget {
    /// The last request.
    Last,
    /// One turn.
    Turn {
        /// The turn.
        turn: TurnId,
    },
    /// One job.
    Job {
        /// The job.
        job: JobId,
    },
}

/// `ask_user`: at most 140 characters and 4 options. Tier 0. Refused when it asks for a
/// password, PIN, one-time code or card number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AskUser {
    /// The question.
    pub question: String,
    /// The options.
    #[serde(default)]
    pub options: Vec<String>,
    /// How long to wait, seconds.
    pub timeout_s: Option<u16>,
    /// An image to show with it.
    pub image: Option<BlobRef>,
}

/// `hud_show`: at most 8 KiB of Markdown. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudShow {
    /// The Markdown.
    pub markdown: String,
    /// A title.
    pub title: Option<String>,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<JobsDispatch>(out, MODULE, "jobs_dispatch");
    input::<JobsStatus>(out, MODULE, "jobs_status");
    input::<JobsControl>(out, MODULE, "jobs_control");
    input::<DesktopStateArgs>(out, MODULE, "desktop_state");
    input::<DesktopLook>(out, MODULE, "desktop_look");
    input::<Capabilities>(out, MODULE, "capabilities");
    input::<SettingsGet>(out, MODULE, "settings_get");
    input::<SettingsPropose>(out, MODULE, "settings_propose");
    input::<SettingsApply>(out, MODULE, "settings_apply");
    input::<SettingsUndo>(out, MODULE, "settings_undo");
    input::<LearnedQuery>(out, MODULE, "learned_query");
    input::<LearnedForget>(out, MODULE, "learned_forget");
    input::<LearnedCorrect>(out, MODULE, "learned_correct");
    input::<LedgerSearch>(out, MODULE, "ledger_search");
    input::<ScenarioSave>(out, MODULE, "scenario_save");
    input::<AskUser>(out, MODULE, "ask_user");
    input::<HudShow>(out, MODULE, "hud_show");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<JobsDispatch>();
    r.add::<JobsDispatchOutput>();
    r.add::<DispatchedJob>();
    r.add::<JobsStatus>();
    r.add::<JobRef>();
    r.add::<JobsControl>();
    r.add::<JobAction>();
    r.add::<DesktopStateArgs>();
    r.add::<Detail>();
    r.add::<DesktopLook>();
    r.add::<LookTarget>();
    r.add::<Capabilities>();
    r.add::<CapabilityScope>();
    r.add::<SettingsGet>();
    r.add::<SettingsPropose>();
    r.add::<SettingsApply>();
    r.add::<SettingsUndo>();
    r.add::<LearnedQuery>();
    r.add::<LearnedKind>();
    r.add::<LearnedForget>();
    r.add::<LearnedCorrect>();
    r.add::<CorrectTarget>();
    r.add::<LedgerSearch>();
    r.add::<ScenarioSave>();
    r.add::<SaveTarget>();
    r.add::<AskUser>();
    r.add::<HudShow>();
}

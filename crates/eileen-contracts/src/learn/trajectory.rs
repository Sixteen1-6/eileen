//! What a job did, step by step, and how it ended.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::skills::{SkillRef, SnapshotManifest};
use crate::core_io::Rung;
use crate::ids::{ApprovalId, CallId, ConnectorId, JobId, Personal, ToolName, Untrusted};
use crate::intent::SlotValues;
use crate::jobs::{Cost, JobStatus, ObservedSource, Route, Taint, Tier};
use crate::schema::Registry;

/// Where a line of a skill traces to. `User`: the user's own transcript. `Worker`: an untainted
/// step's arguments. `Tool`: Eileen's schemas and descriptions. `Observed`: overlaps observed
/// text or traces only to a tainted step. Computed by `eileen-skills`, never taken from the
/// librarian.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// The user's own transcript.
    User,
    /// An untainted step's arguments.
    Worker {
        /// The worker's route.
        route: Route,
    },
    /// Eileen's schemas and descriptions.
    Tool {
        /// The tool.
        tool: ToolName,
    },
    /// Observed text, or a tainted step.
    Observed {
        /// The source.
        source: ObservedSource,
    },
    /// The librarian wrote it.
    Librarian,
    /// The user approved it.
    Approved {
        /// The approval.
        approval: ApprovalId,
    },
}

/// What kind of job a trajectory comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobOrigin {
    /// A worker session.
    Worker,
    /// A replayed recipe.
    Replay,
    /// The fast path.
    FastPath,
    /// A brain turn that ran `act` tools.
    Act,
}

/// One job's record for the learning loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Trajectory {
    /// The format's version.
    pub schema: u16,
    /// The job.
    pub job_id: JobId,
    /// What the user said.
    pub request: Personal<String>,
    /// The dispatched task.
    pub task: String,
    /// The focused app, app versions touched, document identity (redacted), connectors.
    pub context: DesktopContext,
    /// The route that ran it.
    pub route: Route,
    /// What kind of job it was.
    pub origin: JobOrigin,
    /// Which skill versions the session had.
    pub snapshot: Option<SnapshotManifest>,
    /// A hash or manifest mismatch: excluded from learning and statistics.
    pub snapshot_tampered: bool,
    /// The skills it used.
    pub skills_used: Vec<SkillUse>,
    /// The ids of the facts bound into the job (its facts block).
    pub facts: Vec<String>,
    /// The observed sources the job read (section 2.12).
    pub taint: Taint,
    /// Its steps.
    pub steps: Vec<TrajectoryStep>,
    /// What it cost.
    pub cost: Cost,
    /// How it ended.
    pub outcome: Option<Outcome>,
    /// How many values were redacted.
    pub redactions: u32,
    /// The user asked not to learn from it.
    pub no_learn: bool,
}

/// The context a job ran in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopContext {
    /// The focused app.
    pub focused_app: Option<String>,
    /// The apps touched and their versions.
    pub app_versions: Vec<(String, String)>,
    /// The document's identity, redacted.
    pub document: Option<Personal<String>>,
    /// The connectors involved.
    pub connectors: Vec<ConnectorId>,
}

/// One use of a skill. `slot_values`, from the first source that has them: the job's dispatched
/// slots; the `Skill` call's arguments parsed with the skill's slot resolvers
/// (`eileen_skills::slots::from_args`); the skill's template over the request. Else
/// `SlotsUnknown`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillUse {
    /// The skill.
    pub skill: SkillRef,
    /// The `Skill` call's arguments.
    pub args: Untrusted<String>,
    /// The slot values the use is attributed with.
    pub slot_values: SlotValuesOrUnknown,
}

/// A use's slot values, or that they are unknown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlotValuesOrUnknown {
    /// Known.
    Known(SlotValues),
    /// No source had them.
    SlotsUnknown,
}

/// One step of a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrajectoryStep {
    /// The call.
    pub call: CallId,
    /// When, ms since the job started.
    pub at_ms: u64,
    /// The tool.
    pub tool: ToolName,
    /// The op's tag, for tools with ops.
    pub op: Option<String>,
    /// The tool's schema hash.
    pub schema_hash: String,
    /// Its arguments, redacted.
    pub args: Untrusted<Value>,
    /// Whether it succeeded.
    pub ok: bool,
    /// Its result, redacted; long string leaves cut, every key and the structure kept.
    pub result: Untrusted<Value>,
    /// Its tier.
    pub tier: Tier,
    /// The user's answer, when it needed approval.
    pub approval: Option<bool>,
    /// Computed by `eileen-skills`, never taken from the librarian.
    pub provenance: Provenance,
    /// Whether the job was tainted when it ran.
    pub tainted: bool,
    /// The route it took.
    pub rung: Option<Rung>,
    /// The app and version touched.
    pub app: Option<(String, String)>,
    /// Set only on the core's shadow checks (section 2.10 item 7): the recipe step whose
    /// postcondition this read evaluated; never compiled.
    pub shadow_of: Option<String>,
}

/// `shadow::agree`'s result (section 2.10 item 7). Only `Agree` counts toward level 3; neither
/// of the others is a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShadowVerdict {
    /// The job did what the recipe would have.
    Agree,
    /// It did something else.
    Disagree {
        /// The recipe step.
        step: String,
        /// How.
        reason: ShadowMiss,
    },
    /// It could not be told.
    Inconclusive {
        /// The recipe step.
        step: String,
        /// Why.
        reason: String,
    },
}

/// How a shadowed job differed from the recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShadowMiss {
    /// A call the recipe makes is missing.
    MissingCall,
    /// A call has different arguments.
    DifferentArgs {
        /// The argument.
        arg: String,
    },
    /// A call the recipe does not make.
    ExtraCall {
        /// The call.
        call: CallId,
    },
    /// A postcondition failed.
    PostconditionFailed,
    /// A delegate step's span was empty.
    EmptyDelegateSpan,
    /// An `ask` step was not asked.
    AskNotAsked,
}

/// What the user's reaction to a job says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// Accepted.
    Accepted,
    /// Praised.
    Praised,
    /// Corrected.
    Corrected {
        /// What the user said.
        text: Personal<String>,
    },
    /// Undone.
    Undone {
        /// How long after the job ended, ms.
        after_ms: u64,
    },
    /// Retried.
    Retried,
    /// Cancelled.
    Cancelled,
    /// A post-check passed.
    PostcheckPassed,
    /// A post-check failed.
    PostcheckFailed {
        /// The step.
        step: String,
    },
}

/// A job's verdict once the regret window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// It worked.
    Success,
    /// It did not.
    Failure,
    /// It is not clear.
    Unclear,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Outcome {
    /// Its terminal status.
    pub status: JobStatus,
    /// The signals in the regret window.
    pub signals: Vec<Signal>,
    /// The verdict.
    pub verdict: Verdict,
    /// Whether an objective check passed (a verification hint or a skill postcondition).
    pub objective_check: bool,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<Provenance>();
    r.add::<JobOrigin>();
    r.add::<Trajectory>();
    r.add::<DesktopContext>();
    r.add::<SkillUse>();
    r.add::<SlotValuesOrUnknown>();
    r.add::<TrajectoryStep>();
    r.add::<ShadowVerdict>();
    r.add::<ShadowMiss>();
    r.add::<Signal>();
    r.add::<Verdict>();
    r.add::<Outcome>();
}

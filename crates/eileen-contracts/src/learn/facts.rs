//! Facts about the user, phrasings, and forgetting.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::skills::{SkillRef, SlotKind};
use crate::ids::{
    IncidentId, JobId, Personal, RecordingId, ScenarioId, SkillId, ToolName, TurnId, WallTime,
    WorkerSessionId,
};
use crate::record::BlobRef;
use crate::schema::Registry;

use super::trajectory::Provenance;

/// A fact about the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserFact {
    /// Its id.
    pub id: String,
    /// Its kind.
    pub kind: FactKind,
    /// The fact.
    pub text: Personal<String>,
    /// Where it traces to.
    pub provenance: Provenance,
    /// The subject key a correction matches (section 2.10 item 11).
    pub subject: Option<String>,
    /// A topic, repository or app the fact applies to.
    pub scope: Option<String>,
    /// The turn it came from.
    pub source_turn: Option<TurnId>,
    /// The job it came from.
    pub source_job: Option<JobId>,
    /// How sure Eileen is, percent.
    pub confidence_pct: u8,
    /// When it was made.
    pub created: WallTime,
    /// When it was last confirmed.
    pub confirmed: WallTime,
}

/// The kinds of fact. Sources are `SourceStat`s, not facts; `Phrasing` names the kind in
/// commands and queries and is stored as a `PhrasingFact`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    /// About the user.
    User,
    /// The user's feedback on answers.
    Feedback,
    /// About a project.
    Project,
    /// A reference the user keeps.
    Reference,
    /// The user's vocabulary.
    Vocabulary,
    /// How the user phrases things.
    Phrasing,
}

/// A change to the facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactDelta {
    /// A new fact.
    Add {
        /// The fact.
        fact: UserFact,
    },
    /// The user confirmed a fact.
    Confirm {
        /// The fact.
        id: String,
        /// When.
        at: WallTime,
    },
    /// A fact retired.
    Retire {
        /// The fact.
        id: String,
        /// Why.
        reason: RetireReason,
        /// The fact that replaces it.
        replaced_by: Option<String>,
    },
}

/// Why a fact retired. `Corrected` and `Forgotten` queue repairs of skills listing the fact;
/// `Stale` and `Undone` do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetireReason {
    /// The user corrected it.
    Corrected,
    /// The user asked Eileen to forget it.
    Forgotten,
    /// Unconfirmed for too long.
    Stale,
    /// Undone.
    Undone,
}

/// A phrasing: how the user says something Eileen can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhrasingFact {
    /// Its id.
    pub id: String,
    /// What the user said.
    pub utterance: Personal<String>,
    /// What it means.
    pub target: PhrasingTarget,
    /// The utterance's embedding.
    pub embedding: Vec<f32>,
    /// Times the user confirmed it.
    pub confirmed: u16,
    /// Exact repeats only, until the first clean regret window.
    pub provisional: bool,
    /// The turn it came from.
    pub source_turn: TurnId,
    /// The `Act` job it came from; forget plans use both.
    pub source_job: Option<JobId>,
    /// `SlotAlias` targets: the subject key of the phrase ("video editor"), from a clarifying
    /// answer or "X means Y"; a `SameSubject` correction deletes the others.
    pub subject: Option<String>,
    /// Made from a first-run demo turn: deleted when the first run ends.
    pub ephemeral: bool,
}

/// What a phrasing means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhrasingTarget {
    /// A tool call with no slots.
    SlotFreeCall {
        /// The tool.
        tool: ToolName,
        /// Its arguments.
        args: Value,
    },
    /// A grammar entry.
    GrammarEntry {
        /// The entry's id.
        id: String,
    },
    /// A learned skill's template.
    SkillTemplate {
        /// The skill.
        skill: SkillId,
    },
    /// Another name for a slot value ("the video editor" means Resolve).
    SlotAlias {
        /// The slot kind.
        kind: SlotKind,
        /// The value.
        value: Value,
    },
}

/// One entry of the user's vocabulary, for the recognizer and the dictation cleaner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VocabularyEntry {
    /// How it is written.
    pub written: Personal<String>,
    /// How the recognizer tends to hear it.
    pub heard_as: Vec<Personal<String>>,
}

/// What to forget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForgetTarget {
    /// A skill.
    Skill {
        /// The skill.
        id: SkillId,
    },
    /// A fact.
    Fact {
        /// The fact.
        id: String,
    },
    /// Everything about something ("forget that I ...").
    About {
        /// What.
        text: String,
    },
    /// The last thing used.
    LastUsed,
    /// A job and everything it left.
    Job {
        /// The job.
        job: JobId,
    },
}

/// Everything one forget removes. `phrasings`: every phrasing whose source turn or source job
/// is in the plan; `answers`: the answers-cache rows those jobs wrote; `source_topics`: source
/// statistics topics no other job contributed to. The jobs and turns also go to
/// `Persist(PurgeJobs)` for `eileen.db`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ForgetPlan {
    /// Skills.
    pub skills: Vec<SkillRef>,
    /// Facts.
    pub facts: Vec<String>,
    /// Phrasings.
    pub phrasings: Vec<String>,
    /// Answers-cache rows.
    pub answers: Vec<String>,
    /// Source statistics topics.
    pub source_topics: Vec<String>,
    /// Jobs.
    pub jobs: Vec<JobId>,
    /// Turns.
    pub turns: Vec<TurnId>,
    /// Blobs.
    pub blobs: Vec<BlobRef>,
    /// Incidents.
    pub incidents: Vec<IncidentId>,
    /// Recordings.
    pub recordings: Vec<RecordingId>,
    /// Saved scenarios.
    pub scenarios: Vec<ScenarioId>,
    /// Live sessions to recycle.
    pub live_sessions: Vec<WorkerSessionId>,
}

/// What a forget removed, by count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ForgetReport {
    /// Skills.
    pub skills: u32,
    /// Facts.
    pub facts: u32,
    /// Phrasings.
    pub phrasings: u32,
    /// Answers-cache rows.
    pub answers: u32,
    /// Jobs.
    pub jobs: u32,
    /// Blobs.
    pub blobs: u32,
    /// Incidents.
    pub incidents: u32,
    /// Recordings.
    pub recordings: u32,
    /// Saved scenarios.
    pub scenarios: u32,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<UserFact>();
    r.add::<FactKind>();
    r.add::<FactDelta>();
    r.add::<RetireReason>();
    r.add::<PhrasingFact>();
    r.add::<PhrasingTarget>();
    r.add::<VocabularyEntry>();
    r.add::<ForgetTarget>();
    r.add::<ForgetPlan>();
    r.add::<ForgetReport>();
}

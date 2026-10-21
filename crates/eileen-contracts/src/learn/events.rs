//! What passes between the core and the learning store, and the learning loop's bus events.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::facts::{FactDelta, ForgetPlan, ForgetReport, PhrasingFact, PhrasingTarget, UserFact};
use super::routes::{RouteProposal, RouteStat, SourceStat};
use super::skills::{
    CommitReason, Level, LintError, Recipe, SkillDoc, SkillMeta, SkillRef, SnapshotManifest,
};
use super::trajectory::{Outcome, Signal, TrajectoryStep, Verdict};
use crate::ids::{JobId, Personal, RouteProposalId, SkillId, TurnId, Untrusted, WorkerSessionId};
use crate::jobs::{Cost, Taint};
use crate::schema::Registry;

/// From the learning store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnEvent {
    /// The skills, recipes, phrasings or facts changed; the core rebuilds the resolver and the
    /// skill engine.
    SkillsChanged {
        /// Every skill.
        skills: Vec<SkillMeta>,
        /// Every recipe.
        recipes: Vec<Recipe>,
        /// Every phrasing.
        phrasings: Vec<PhrasingFact>,
        /// Every active fact.
        facts: Vec<UserFact>,
    },
    /// A session's snapshot was written.
    SnapshotWritten {
        /// The session.
        session: WorkerSessionId,
        /// Its manifest.
        manifest: SnapshotManifest,
    },
    /// A session's snapshot no longer matches its manifest.
    SnapshotTampered {
        /// The session.
        session: WorkerSessionId,
        /// The file.
        file: String,
    },
    /// A proposed skill version was committed, held or rejected.
    Proposed {
        /// The skill.
        skill: SkillId,
        /// What happened.
        outcome: ProposeOutcome,
    },
    /// A librarian batch finished.
    LibrarianFinished {
        /// What it cost.
        cost: Cost,
        /// Jobs distilled.
        distilled: u16,
        /// Drafts rejected.
        rejected: u16,
    },
    /// A forget finished.
    Forgot {
        /// What it removed.
        report: ForgetReport,
    },
    /// A worker proposed a route.
    RouteProposed {
        /// The proposal.
        proposal: Box<RouteProposal>,
    },
    /// After each update (a job's verdict was final); the core rebuilds the skill engine and the
    /// resolver's introspection with them.
    RouteStats {
        /// Every route statistic.
        stats: Vec<RouteStat>,
    },
    /// After each update; deep-answer jobs' facts blocks read them.
    SourceStats {
        /// Every source statistic.
        stats: Vec<SourceStat>,
    },
}

/// What `learn_skill_propose` and a commit come to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposeOutcome {
    /// Committed.
    Committed {
        /// The new version.
        version: u32,
    },
    /// Some lines wait for the user's approval.
    PendingApproval {
        /// The held lines.
        lines: Vec<u32>,
    },
    /// The whole draft waits for review (free-text taint).
    PendingReview,
    /// Rejected by the linter.
    Rejected {
        /// What is wrong.
        errors: Vec<LintError>,
        /// Tries left before the draft is dropped.
        attempts_left: u8,
    },
}

/// To the learning store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnCmd {
    /// Start a job's trajectory; an `Act` job carries its turn context (section 2.12).
    Begin {
        /// The job.
        job: JobId,
        /// What it has read already.
        taint: Taint,
        /// An `Act` job's observed turn context.
        observed: Option<Untrusted<String>>,
    },
    /// Record a step.
    Step {
        /// The job.
        job: JobId,
        /// The step.
        step: Box<TrajectoryStep>,
    },
    /// Record a signal.
    Signal {
        /// The job.
        job: JobId,
        /// The signal.
        signal: Signal,
    },
    /// Close a job's trajectory.
    Finish {
        /// The job.
        job: JobId,
        /// How it ended.
        outcome: Outcome,
    },
    /// Write a session's learned-skill snapshot.
    WriteSnapshot {
        /// The session.
        session: WorkerSessionId,
        /// The skills to put in it.
        select: Vec<SkillRef>,
    },
    /// Run the librarian over a batch of jobs.
    RunLibrarian {
        /// The jobs.
        batch: Vec<JobId>,
    },
    /// Commit a skill version.
    Commit {
        /// Its files.
        doc: Box<SkillDoc>,
        /// Why.
        reason: CommitReason,
    },
    /// Carry out a forget.
    Forget(ForgetPlan),
    /// Roll a skill back.
    Rollback {
        /// The skill.
        skill: SkillId,
        /// The version to return to.
        to_version: u32,
    },
    /// Change the facts.
    FactDelta(FactDelta),
    /// Record a phrasing.
    Phrasing {
        /// What the user said.
        utterance: Personal<String>,
        /// What it means.
        target: PhrasingTarget,
        /// Whether the user confirmed it.
        confirm: bool,
        /// The turn.
        turn: TurnId,
        /// The `Act` job.
        job: Option<JobId>,
        /// The subject key.
        subject: Option<String>,
        /// Made in the first run.
        ephemeral: bool,
    },
    /// Delete phrasings (journaled with the rows, so "undo that" restores them).
    DeletePhrasings {
        /// The phrasings.
        ids: Vec<String>,
    },
    /// Queue a repair of a skill from the user's words.
    Repair {
        /// The skill.
        skill: SkillId,
        /// What the user said.
        words: Personal<String>,
        /// The turn.
        turn: TurnId,
    },
    /// Update the source statistics of a deep answer.
    SourceStat {
        /// The topic.
        topic: Personal<String>,
        /// Hosts it cited.
        cited: Vec<String>,
        /// Hosts of an accepted answer.
        accepted: Vec<String>,
        /// Hosts the user opened.
        opened: Vec<String>,
    },
    /// Store a route proposal.
    RouteProposal(RouteProposal),
    /// Update a route statistic.
    RouteStat(RouteStat),
}

/// The learning loop's bus events (dotted names `learn.skill_drafted`, `learn.verdict`, ...).
/// Each distillation attempt of a job ends in exactly one of `SkillDrafted`, `DraftRejected`,
/// `DistillSkipped` or `DistillDeferred`, and a job is distilled at most once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LearnBusEvent {
    /// A draft passed the linter.
    SkillDrafted {
        /// The job it came from.
        job: JobId,
        /// The skill.
        skill: SkillRef,
        /// What is held for review.
        held: DraftHold,
    },
    /// A draft was rejected.
    DraftRejected {
        /// The job.
        job: JobId,
    },
    /// The job was not distilled.
    DistillSkipped {
        /// The job.
        job: JobId,
        /// Why.
        reason: SkipReason,
    },
    /// The job's distillation was put off, for a later batch.
    DistillDeferred {
        /// The job.
        job: JobId,
        /// Why.
        reason: DeferReason,
    },
    /// A later undo removed a draft's only evidence.
    DraftWithdrawn {
        /// The job.
        job: JobId,
        /// The skill.
        skill: SkillRef,
    },
    /// Lines of a skill wait for approval.
    PendingApproval {
        /// The skill.
        skill: SkillRef,
        /// The lines.
        lines: Vec<u32>,
    },
    /// A skill moved up the ladder.
    Promoted {
        /// The skill.
        skill: SkillRef,
        /// Its new level.
        level: Level,
    },
    /// A skill moved down the ladder.
    Demoted {
        /// The skill.
        skill: SkillRef,
        /// Its new level.
        level: Level,
    },
    /// A forget finished.
    Forgot {
        /// What it removed.
        report: ForgetReport,
    },
    /// A replay step ran.
    ReplayStep {
        /// The job.
        job: JobId,
        /// The step.
        step: String,
        /// Whether its checks passed.
        ok: bool,
    },
    /// A replay handed off to a worker.
    ReplayHandoff {
        /// The job.
        job: JobId,
        /// The step that failed.
        step: String,
        /// Why.
        reason: String,
    },
    /// The regret window closed.
    Verdict {
        /// The job.
        job: JobId,
        /// The verdict.
        verdict: Verdict,
    },
    /// A worker proposed a route.
    RouteProposed {
        /// The proposal.
        proposal: RouteProposalId,
        /// The app.
        app: String,
    },
    /// A session's snapshot no longer matches its manifest.
    SnapshotTampered {
        /// The session.
        session: WorkerSessionId,
    },
}

/// What part of a draft is held for review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DraftHold {
    /// Nothing.
    None,
    /// Some lines.
    Lines,
    /// The whole draft.
    Whole,
}

/// Why a job was not distilled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// It failed.
    Failed,
    /// Its verdict is unclear.
    Unclear,
    /// It was too short to learn from.
    TooShort,
    /// The user asked not to learn from it.
    NoLearn,
    /// It used a skill; only its statistics count.
    StatsOnly,
    /// It did not run on Claude (fast-path jobs).
    NotOnClaude,
    /// The librarian proposed nothing.
    NothingProposed,
    /// Distillation is off.
    DistillOff,
}

/// Why a job's distillation was put off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeferReason {
    /// The plan share cap.
    PlanCap,
    /// Memory admission refused the librarian.
    AdmissionRefused,
    /// The librarian failed.
    LibrarianFailed {
        /// Its error.
        error: String,
    },
    /// Cancelled, or left `Queued` or `Running` by a stopped runtime.
    Cancelled,
}

/// One row per job in `librarian_runs` (section 2.10 item 2): idle distillation takes only jobs
/// with no row or a `Deferred` outcome. `Done { Rejected }` is final.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DistillState {
    /// Waiting for a batch.
    Queued,
    /// In a batch.
    Running {
        /// The batch.
        batch: u32,
    },
    /// Finished.
    Done {
        /// How.
        outcome: DistillOutcome,
    },
}

/// How a job's distillation finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DistillOutcome {
    /// A draft.
    Drafted {
        /// The skill.
        skill: SkillRef,
        /// What is held.
        held: DraftHold,
    },
    /// Rejected.
    Rejected,
    /// Skipped.
    Skipped(SkipReason),
    /// Put off.
    Deferred(DeferReason),
}

pub(super) fn register(r: &mut Registry) {
    r.add::<LearnEvent>();
    r.add::<ProposeOutcome>();
    r.add::<LearnCmd>();
    r.add::<LearnBusEvent>();
    r.add::<DraftHold>();
    r.add::<SkipReason>();
    r.add::<DeferReason>();
    r.add::<DistillState>();
    r.add::<DistillOutcome>();
}

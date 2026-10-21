//! Learning types (BUILD.md section 3.3.13): trajectories, skills and recipes, facts and
//! phrasings, route proposals and statistics, the learning store's events and commands, and the
//! replay engine's data.
//!
//! Provisional: WP7 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! `eileen-skills` holds the pure functions over these types (template, ladder, lint,
//! provenance, slots, facts, routes, sources, shadow, verdict, the replay machine, forget plans);
//! `eileen-learn` holds the IO.

use crate::schema::Registry;

mod engine;
mod events;
mod facts;
mod routes;
mod skills;
mod trajectory;

pub use engine::{
    CheckPlan, HandoffBrief, Question, ReplayRefusal, ReplayState, ReplayStep, StepResult,
    ToolCallPlan, Transition,
};
pub use events::{
    DeferReason, DistillOutcome, DistillState, DraftHold, LearnBusEvent, LearnCmd, LearnEvent,
    ProposeOutcome, SkipReason,
};
pub use facts::{
    FactDelta, FactKind, ForgetPlan, ForgetReport, ForgetTarget, PhrasingFact, PhrasingTarget,
    RetireReason, UserFact, VocabularyEntry,
};
pub use routes::{
    McpPackage, PackageType, ProposalStatus, Publisher, PublisherKind, RouteKind, RouteProposal,
    RouteStat, SourceStat,
};
pub use skills::{
    Check, CommitReason, Expect, LadderPolicy, Level, LintError, OnFail, Operand, Recipe,
    RecipeStep, RiskClass, SkillDoc, SkillMeta, SkillRef, SkillVersion, SlotKind, SlotSpec,
    SnapshotManifest, StepKind, Thresholds, UseRecord,
};
pub use trajectory::{
    DesktopContext, JobOrigin, Outcome, Provenance, ShadowMiss, ShadowVerdict, Signal, SkillUse,
    SlotValuesOrUnknown, Trajectory, TrajectoryStep, Verdict,
};

pub(crate) fn register(r: &mut Registry) {
    trajectory::register(r);
    skills::register(r);
    facts::register(r);
    routes::register(r);
    events::register(r);
    engine::register(r);
}

//! The librarian's toolset (`learn`): arguments only; the results are the learn module's types.
//!
//! Provisional: WP7 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{NoArgs, ToolInput, input};
use crate::ids::{JobId, SkillId};
use crate::learn::{CommitReason, FactKind, SkillDoc};
use crate::schema::Registry;

const MODULE: &str = "tools/learn";

/// `learn_trajectory_read`: one job's redacted trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnTrajectoryRead {
    /// The job.
    pub job: JobId,
}

/// `learn_skill_read`: a skill's current version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnSkillRead {
    /// The skill.
    pub id: SkillId,
}

/// `learn_skill_history`: a skill's versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnSkillHistory {
    /// The skill.
    pub id: SkillId,
}

/// `learn_skill_propose`: returns a `ProposeOutcome` (`Committed`, `PendingApproval`,
/// `PendingReview` for a whole draft with free-text taint, or `Rejected` with lint errors).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LearnSkillPropose {
    /// The skill's files.
    pub doc: SkillDoc,
    /// Why it changes.
    pub reason: CommitReason,
    /// The jobs it was learned from.
    pub evidence: Vec<JobId>,
}

/// `learn_fact_propose`: never with observed provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LearnFactPropose {
    /// The kind of fact.
    pub kind: FactKind,
    /// The fact.
    pub text: String,
    /// The jobs it was learned from.
    pub evidence: Vec<JobId>,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<LearnTrajectoryRead>(out, MODULE, "learn_trajectory_read");
    input::<NoArgs>(out, MODULE, "learn_skill_list");
    input::<LearnSkillRead>(out, MODULE, "learn_skill_read");
    input::<LearnSkillHistory>(out, MODULE, "learn_skill_history");
    input::<LearnSkillPropose>(out, MODULE, "learn_skill_propose");
    input::<LearnFactPropose>(out, MODULE, "learn_fact_propose");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<LearnTrajectoryRead>();
    r.add::<LearnSkillRead>();
    r.add::<LearnSkillHistory>();
    r.add::<LearnSkillPropose>();
    r.add::<LearnFactPropose>();
}

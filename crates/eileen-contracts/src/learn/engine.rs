//! The data the core and the skill engine exchange: ladder transitions, shadow checks and the
//! replay machine's steps.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::skills::{Expect, Level, SkillRef};
use crate::core_io::ToolResult;
use crate::ids::ToolName;
use crate::intent::SlotValues;
use crate::jobs::JobSpec;
use crate::schema::Registry;
use crate::tools::brain::AskUser;

/// What `next_level` decides for a skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// No change.
    Stay,
    /// Up the ladder (level 3 also needs the user's approval).
    Promote {
        /// The new level.
        to: Level,
    },
    /// Down the ladder.
    Demote {
        /// The new level.
        to: Level,
    },
    /// Archived after idle days.
    Archive,
    /// On probation: one shadow run before the next replay.
    Probation {
        /// Why (an app version or a tool's schema hash changed).
        reason: String,
    },
}

/// A check the core must issue, with origin `Core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckPlan {
    /// The recipe step whose postcondition it evaluates.
    pub step: String,
    /// The tool, or `None` for the step's own output.
    pub tool: Option<ToolName>,
    /// The op's tag.
    pub op: Option<String>,
    /// Its arguments, operands resolved.
    pub args: Value,
    /// A JSON pointer into the result.
    pub path: String,
    /// What the value must be.
    pub expect: Expect,
}

/// A replay in progress, kept by the core between steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReplayState {
    /// The skill.
    pub skill: SkillRef,
    /// Its slots, `ask` answers included.
    pub slots: SlotValues,
    /// The index of the main step in progress.
    pub step: u32,
    /// The index inside the current step's fallback, while one runs.
    pub fallback_step: Option<u32>,
    /// Whether the failed step has been re-run after its fallback or retry.
    pub rerun: bool,
    /// Each finished step's output, by step id, for pointers.
    pub outputs: BTreeMap<String, Value>,
}

/// Why a replay cannot start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplayRefusal {
    /// No such skill.
    UnknownSkill,
    /// The skill has no recipe.
    NoRecipe,
    /// The skill is below level 4.
    LevelTooLow,
    /// The skill is on probation.
    Probation,
    /// A slot is ambiguous; replay never runs on an ambiguous slot.
    AmbiguousSlot {
        /// The slot.
        slot: String,
    },
    /// A required slot has no value.
    MissingSlot {
        /// The slot.
        slot: String,
    },
    /// A tool the recipe calls changed its schema hash.
    ToolChanged {
        /// The tool.
        tool: ToolName,
    },
}

/// The result of the step the core just ran, fed back to the replay machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepResult {
    /// A call's result.
    Call {
        /// The step.
        step: String,
        /// The result.
        result: Box<ToolResult>,
    },
    /// A check's result.
    Check {
        /// The step.
        step: String,
        /// Whether it passed.
        passed: bool,
    },
    /// The user's answer to an `ask` step.
    Answer {
        /// The step.
        step: String,
        /// The value it resolved to.
        value: Value,
    },
    /// A delegated sub-job's report.
    Delegated {
        /// The step.
        step: String,
        /// The report.
        report: Value,
    },
    /// The user's answer to a `confirm` step.
    Confirmed {
        /// The step.
        step: String,
        /// Whether the user said yes.
        yes: bool,
    },
}

/// What the replay machine wants next. A failed postcondition with a fallback yields the
/// fallback's steps, then the re-run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStep {
    /// Call a tool.
    Call(ToolCallPlan),
    /// Run a check.
    Check(CheckPlan),
    /// Ask the user.
    Ask(Question),
    /// Start a narrow sub-job.
    Delegate(Box<JobSpec>),
    /// Ask the user to confirm.
    Confirm(AskUser),
    /// Finished.
    Done,
    /// Hand the job to a worker.
    Handoff(HandoffBrief),
}

/// A call the replay makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolCallPlan {
    /// The recipe step.
    pub step: String,
    /// The tool.
    pub tool: ToolName,
    /// The op's tag.
    pub op: Option<String>,
    /// Its arguments, operands resolved, one flat object.
    pub args: Value,
}

/// An `ask` step's question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Question {
    /// The recipe step.
    pub step: String,
    /// The slot it fills.
    pub slot: String,
    /// The words of the question.
    pub text: String,
    /// The options.
    pub options: Vec<String>,
}

/// What a worker gets when a replay hands off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HandoffBrief {
    /// The skill.
    pub skill: SkillRef,
    /// The step that failed.
    pub step: String,
    /// Why.
    pub reason: String,
    /// The steps already done.
    pub done_steps: Vec<String>,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<Transition>();
    r.add::<CheckPlan>();
    r.add::<ReplayState>();
    r.add::<ReplayRefusal>();
    r.add::<StepResult>();
    r.add::<ReplayStep>();
    r.add::<ToolCallPlan>();
    r.add::<Question>();
    r.add::<HandoffBrief>();
}

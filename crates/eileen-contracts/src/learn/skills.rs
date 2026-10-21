//! Skills, recipes, checks and the promotion ladder.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::trajectory::{SlotValuesOrUnknown, Verdict};
use crate::ids::{JobId, SkillId, ToolName, WallTime, WorkerSessionId};
use crate::schema::Registry;

/// A skill's level on the promotion ladder.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Level 1: a draft, offered to workers as a hint.
    Draft,
    /// Level 2: trusted, followed by workers.
    Trusted,
    /// Level 3: a recipe, matched to the brain as a `[match]` block.
    Recipe,
    /// Level 4: replayed with no model.
    FastPath,
}

/// One version of one skill.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SkillRef {
    /// The skill.
    pub id: SkillId,
    /// Its version.
    pub version: u32,
}

/// The kind of a slot. `Pick` is one of the options an `ask` step offered at run time, matched
/// by the choice resolver against that list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    /// A time offset ("15:30").
    Offset,
    /// A number of things.
    Count,
    /// Free text.
    Text,
    /// An app.
    App,
    /// A file.
    File,
    /// A URL.
    Url,
    /// A piece of media.
    Media,
    /// A position ("the one on the right").
    Position,
    /// A monitor.
    Monitor,
    /// One of fixed options.
    Choice {
        /// The options.
        options: Vec<String>,
    },
    /// One of the options an `ask` step offered.
    Pick,
}

/// One slot of a skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SlotSpec {
    /// Its name.
    pub name: String,
    /// Its kind.
    pub kind: SlotKind,
    /// Whether it must have a value.
    pub required: bool,
    /// Its default.
    pub default: Option<Value>,
    /// Filled only by the skill's `ask` step; never in the template, never from the request,
    /// not part of a use's slot values.
    pub asked: bool,
    /// Enumerated kinds: values a successful use has seen.
    pub seen_values: Vec<Value>,
}

/// How much care a skill needs on the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    /// Light thresholds.
    Light,
    /// Standard thresholds.
    Standard,
}

/// What the core and the resolver know about a skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillMeta {
    /// The skill.
    pub id: SkillId,
    /// Its version.
    pub version: u32,
    /// Its level.
    pub level: Level,
    /// Its risk class.
    pub risk: RiskClass,
    /// Its description.
    pub description: String,
    /// Its trigger templates.
    pub triggers: Vec<String>,
    /// Its slots.
    pub slots: Vec<SlotSpec>,
    /// The apps and version ranges the recipe was proven on.
    pub apps: Vec<(String, String)>,
    /// The schema hashes the recipe was proven with.
    pub tool_hashes: Vec<(ToolName, String)>,
    /// Whether it has a recipe.
    pub has_recipe: bool,
    /// The jobs it was learned from.
    pub evidence: Vec<JobId>,
    /// Confirmed phrasings of it.
    pub phrasings: Vec<String>,
    /// Facts bound into the evidence jobs whose values appear in the skill's literals or recipe
    /// arguments; retiring one queues a repair and drops a level.
    pub facts_used: Vec<String>,
    /// Pinned by the user; never archived.
    pub pinned: bool,
    /// Learned from a job that read untrusted text.
    pub untrusted_origin: bool,
}

/// A skill's model-free procedure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Recipe {
    /// The skill.
    pub skill: SkillRef,
    /// Checks before the first step.
    pub pre: Vec<Check>,
    /// The steps.
    pub steps: Vec<RecipeStep>,
}

/// One step of a recipe. `post` is required (non-empty) on `Call` and `Delegate` steps and
/// empty on `Ask` and `Confirm`, whose postconditions are implicit (the slot resolved; the user
/// said yes). Every `Delegate` is followed by a `Call` step with a postcondition, which is its
/// objective check in shadow mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeStep {
    /// Its id, for pointers.
    pub id: String,
    /// What it does.
    pub kind: StepKind,
    /// Checks before it.
    pub pre: Vec<Check>,
    /// Checks after it.
    pub post: Vec<Check>,
    /// How long it may take.
    pub timeout_ms: u32,
    /// What happens when it fails.
    pub on_fail: OnFail,
}

/// A value in arguments, check arguments and expectations. Serialized as a plain JSON literal,
/// `{"$slot": "t"}`, `{"$step": "id", "ptr": "/json/pointer"}` or `{"$len": <operand>}` (the
/// length of the array the inner operand resolves to). A literal that has one of those shapes,
/// or whose only key is `$lit`, is written as `{"$lit": <literal>}`, so a literal taken from a
/// page or the user's words never reads back as a reference. The linter types every pointer
/// against the op's output schema (a `Delegate` step's against the report schema).
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// A literal.
    Lit(Value),
    /// A slot's value.
    Slot(String),
    /// A pointer into an earlier step's output.
    Step {
        /// The step's id.
        step: String,
        /// A JSON pointer.
        ptr: String,
    },
    /// The length of an array.
    Len(Box<Self>),
}

impl Operand {
    /// The operand's JSON form.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let single = |key: &str, value: Value| {
            let mut map = Map::new();
            map.insert(key.to_owned(), value);
            Value::Object(map)
        };
        match self {
            Self::Lit(value) if is_reference_shaped(value) => single("$lit", value.clone()),
            Self::Lit(value) => value.clone(),
            Self::Slot(name) => single("$slot", Value::String(name.clone())),
            Self::Step { step, ptr } => {
                let mut map = Map::new();
                map.insert("$step".to_owned(), Value::String(step.clone()));
                map.insert("ptr".to_owned(), Value::String(ptr.clone()));
                Value::Object(map)
            }
            Self::Len(inner) => single("$len", inner.to_value()),
        }
    }

    /// Reads an operand from its JSON form: `{"$lit": <value>}` is that literal, and anything
    /// that is not one of the three reference forms is a literal too.
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        let Value::Object(mut map) = value else { return Self::Lit(value) };
        if map.len() == 1 {
            if let Some(Value::String(name)) = map.get("$slot") {
                return Self::Slot(name.clone());
            }
            if let Some(inner) = map.remove("$len") {
                return Self::Len(Box::new(Self::from_value(inner)));
            }
            if let Some(literal) = map.remove("$lit") {
                return Self::Lit(literal);
            }
        }
        if map.len() == 2
            && let (Some(Value::String(step)), Some(Value::String(ptr))) =
                (map.get("$step"), map.get("ptr"))
        {
            return Self::Step { step: step.clone(), ptr: ptr.clone() };
        }
        Self::Lit(Value::Object(map))
    }
}

/// True for an object [`Operand::from_value`] would not read as a plain literal: one whose only
/// key is `$slot`, `$len` or `$lit`, or whose two keys are `$step` and `ptr`. Such a literal is
/// written escaped.
fn is_reference_shaped(value: &Value) -> bool {
    let Value::Object(map) = value else { return false };
    match map.len() {
        1 => ["$slot", "$len", "$lit"].iter().any(|key| map.contains_key(*key)),
        2 => map.contains_key("$step") && map.contains_key("ptr"),
        _ => false,
    }
}

impl Serialize for Operand {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Operand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Value::deserialize(deserializer).map(Self::from_value)
    }
}

impl JsonSchema for Operand {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Operand")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let operand = generator.subschema_for::<Self>();
        json_schema!({
            "description": "A literal, {\"$slot\": name}, {\"$step\": id, \"ptr\": pointer}, {\"$len\": operand}, or {\"$lit\": literal} for a literal of one of those shapes",
            "anyOf": [
                {
                    "type": "object",
                    "properties": { "$slot": { "type": "string" } },
                    "required": ["$slot"],
                    "additionalProperties": false,
                },
                {
                    "type": "object",
                    "properties": { "$step": { "type": "string" }, "ptr": { "type": "string" } },
                    "required": ["$step", "ptr"],
                    "additionalProperties": false,
                },
                {
                    "type": "object",
                    "properties": { "$len": operand },
                    "required": ["$len"],
                    "additionalProperties": false,
                },
                {
                    "type": "object",
                    "properties": { "$lit": true },
                    "required": ["$lit"],
                    "additionalProperties": false,
                },
                true,
            ],
        })
    }
}

/// What a recipe step does. `Ask.question`, `Delegate.goal` and `Confirm.question` are literals
/// for tracing, overlap and the deny patterns (section 2.10 item 9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Call a tool; argument values may be operands.
    Call {
        /// The tool.
        tool: ToolName,
        /// The op's tag.
        op: Option<String>,
        /// Its arguments, one flat object.
        args: Value,
    },
    /// The core's clarifying question; `slot` names a slot spec with `asked = true` (kind
    /// `Pick` when `options` is set); never skipped.
    Ask {
        /// The question.
        question: String,
        /// The options, from an earlier step.
        options: Option<Operand>,
        /// The slot it fills.
        slot: String,
    },
    /// A narrow worker sub-job; its output is the sub-job's report.
    Delegate {
        /// What the sub-job does.
        goal: String,
        /// Its inputs.
        inputs: Value,
    },
    /// Ask the user to confirm; never skipped by replay.
    Confirm {
        /// The question.
        question: String,
        /// What to show with it.
        show: Option<Operand>,
    },
}

/// What happens when a step fails. `Fallback`: run these steps (no fallbacks of their own),
/// then re-run the failed step once and re-check it; a second failure hands off. A fallback
/// step may point to earlier main steps and earlier steps of its own fallback; no main step and
/// no other fallback may point into it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnFail {
    /// Hand the job to a worker.
    Handoff,
    /// Retry once (idempotent ops only).
    RetryOnce,
    /// Stop.
    Abort,
    /// Run these steps, then re-run the failed step once.
    Fallback(Vec<RecipeStep>),
}

/// A check. Checks may only call ops that are read-only and tier 0; the linter enforces it
/// against the connector manifests. With `tool` `None`, the check reads the step's own output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Check {
    /// The tool to call, or `None` for the step's own output.
    pub tool: Option<ToolName>,
    /// The op's tag.
    pub op: Option<String>,
    /// Its arguments.
    pub args: Value,
    /// A JSON pointer into the result.
    pub path: String,
    /// What the value must be.
    pub expect: Expect,
}

/// What a checked value must be. `AtLeast`, `AtLeastRel`, `Within` and `Count` take numeric
/// operands only (an array is wrapped in `Len`); `Count` compares the length of the array at
/// `path` with its bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// Equal to.
    Equals(Operand),
    /// Contains.
    Contains(Operand),
    /// At least.
    AtLeast(Operand),
    /// At least a factor of another value.
    AtLeastRel {
        /// The other value.
        of: Operand,
        /// The factor.
        factor: f64,
    },
    /// Within a tolerance of another value.
    Within {
        /// The other value.
        of: Operand,
        /// The tolerance.
        tol: f64,
    },
    /// An array's length between bounds.
    Count {
        /// The least length.
        min: Operand,
        /// The greatest length.
        max: Option<Operand>,
    },
    /// Present.
    Exists,
    /// Truthy.
    Truthy,
}

/// A skill's files: the full content of `SKILL.md`, its references and its recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillDoc {
    /// The skill.
    pub id: SkillId,
    /// `SKILL.md`.
    pub skill_md: String,
    /// Reference files, by path inside the skill folder.
    pub references: BTreeMap<String, String>,
    /// `recipe.json`.
    pub recipe: Option<Recipe>,
}

/// One stored version of a skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillVersion {
    /// The skill.
    pub skill: SkillId,
    /// The version.
    pub version: u32,
    /// When it was committed.
    pub at: WallTime,
    /// Why.
    pub reason: CommitReason,
    /// The jobs it was learned from.
    pub evidence: Vec<JobId>,
    /// Its files.
    pub doc: SkillDoc,
}

/// Why a skill version was committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommitReason {
    /// Distilled by the librarian.
    Distill,
    /// Repaired after a failure or a retired fact.
    Repair,
    /// Edited by the user.
    UserEdit,
    /// Corrected by the user.
    UserCorrection,
    /// Rolled back.
    Rollback,
    /// Promoted.
    Promote,
    /// Demoted.
    Demote,
}

/// Which skill files a session's snapshot holds: (skill, path, BLAKE3 hash) per file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotManifest {
    /// The session.
    pub session: WorkerSessionId,
    /// The files.
    pub files: Vec<(SkillRef, String, String)>,
}

/// The promotion ladder's policy (defaults in section 2.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LadderPolicy {
    /// Light skills' thresholds.
    pub light: Thresholds,
    /// Standard skills' thresholds.
    pub standard: Thresholds,
    /// Failures in the last five uses that drop a level.
    pub demote_failures_in_last5: u8,
    /// Idle days that archive a skill (never deletion; pinned skills are never archived).
    pub archive_idle_days: u16,
}

impl Default for LadderPolicy {
    fn default() -> Self {
        Self {
            light: Thresholds {
                l2_successes: 1,
                l2_distinct_slot_values: 1,
                l3_agreeing_shadows: 1,
                l4_clean_replays: 3,
                l4_clean_pct_last_20: 0,
            },
            standard: Thresholds {
                l2_successes: 3,
                l2_distinct_slot_values: 2,
                l3_agreeing_shadows: 3,
                l4_clean_replays: 10,
                l4_clean_pct_last_20: 95,
            },
            demote_failures_in_last5: 2,
            archive_idle_days: 30,
        }
    }
}

/// Per level: level 2 needs successes and distinct slot values, level 3 agreeing shadows plus
/// approval, level 4 clean replays and a clean share of the last 20 (0: no share required).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Thresholds {
    /// Level 2: successful uses.
    pub l2_successes: u8,
    /// Level 2: distinct slot values among them.
    pub l2_distinct_slot_values: u8,
    /// Level 3: agreeing shadow runs.
    pub l3_agreeing_shadows: u8,
    /// Level 4: clean replays.
    pub l4_clean_replays: u8,
    /// Level 4: the clean share of the last 20 uses, percent.
    pub l4_clean_pct_last_20: u8,
}

/// One use of a skill, for the ladder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UseRecord {
    /// The job.
    pub job: JobId,
    /// When.
    pub at: WallTime,
    /// Its verdict.
    pub verdict: Verdict,
    /// Whether an objective check passed.
    pub objective_check: bool,
    /// The slot values it was attributed with.
    pub slot_values: SlotValuesOrUnknown,
    /// Whether it was a replay.
    pub replay: bool,
    /// Whether the replay handed off.
    pub handoff: bool,
    /// The apps touched and their versions.
    pub app_versions: Vec<(String, String)>,
}

/// One rule the linter found broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintError {
    /// The rule's id.
    pub rule: String,
    /// The recipe step, when it is about one.
    pub step: Option<String>,
    /// The line of `SKILL.md`, when it is about one.
    pub line: Option<u32>,
    /// What is wrong.
    pub message: String,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<Level>();
    r.add::<SkillRef>();
    r.add::<SlotKind>();
    r.add::<SlotSpec>();
    r.add::<RiskClass>();
    r.add::<SkillMeta>();
    r.add::<Recipe>();
    r.add::<RecipeStep>();
    r.add::<Operand>();
    r.add::<StepKind>();
    r.add::<OnFail>();
    r.add::<Check>();
    r.add::<Expect>();
    r.add::<SkillDoc>();
    r.add::<SkillVersion>();
    r.add::<CommitReason>();
    r.add::<SnapshotManifest>();
    r.add::<LadderPolicy>();
    r.add::<Thresholds>();
    r.add::<UseRecord>();
    r.add::<LintError>();
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::{Map, Value, json};

    use super::Operand;

    /// JSON that looks like an operand's reference forms, and plain JSON beside it.
    fn tricky_literal() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i32>().prop_map(Value::from),
            "[a-z/$]{0,6}".prop_map(Value::String),
        ];
        leaf.prop_recursive(3, 16, 3, |inner| {
            let key = prop::sample::select(vec!["$slot", "$step", "ptr", "$len", "$lit", "a"]);
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
                prop::collection::vec((key, inner), 0..3).prop_map(|pairs| Value::Object(
                    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<Map<_, _>>()
                )),
            ]
        })
    }

    fn operand() -> impl Strategy<Value = Operand> {
        let name = "[a-z$]{0,5}";
        let leaf = prop_oneof![
            tricky_literal().prop_map(Operand::Lit),
            name.prop_map(Operand::Slot),
            (name, "[a-z/]{0,6}").prop_map(|(step, ptr)| Operand::Step { step, ptr }),
        ];
        leaf.prop_recursive(3, 8, 1, |inner| inner.prop_map(|op| Operand::Len(Box::new(op))))
    }

    proptest! {
        #[test]
        fn every_operand_reads_back_as_itself(op in operand()) {
            prop_assert_eq!(&Operand::from_value(op.to_value()), &op);
            let text = serde_json::to_string(&op).unwrap();
            prop_assert_eq!(serde_json::from_str::<Operand>(&text).unwrap(), op);
        }
    }

    #[test]
    fn a_literal_shaped_like_a_reference_stays_a_literal() {
        for literal in [
            json!({"$slot": "t"}),
            json!({"$step": "goto", "ptr": "/target_frame"}),
            json!({"$len": [1, 2]}),
            json!({"$lit": 5}),
        ] {
            let op = Operand::Lit(literal.clone());
            assert_eq!(op.to_value(), json!({ "$lit": literal }), "written escaped");
            assert_eq!(Operand::from_value(op.to_value()), op, "{literal}");
        }
    }

    #[test]
    fn operands_read_and_write_their_four_forms() {
        let cases = [
            (json!(930.0), Operand::Lit(json!(930.0))),
            (json!({"$slot": "t"}), Operand::Slot("t".to_owned())),
            (
                json!({"$step": "goto", "ptr": "/target_frame"}),
                Operand::Step { step: "goto".to_owned(), ptr: "/target_frame".to_owned() },
            ),
            (
                json!({"$len": {"$slot": "faces"}}),
                Operand::Len(Box::new(Operand::Slot("faces".to_owned()))),
            ),
        ];
        for (text, operand) in cases {
            assert_eq!(serde_json::from_value::<Operand>(text.clone()).unwrap(), operand, "{text}");
            assert_eq!(serde_json::to_value(&operand).unwrap(), text, "{text}");
        }
    }

    #[test]
    fn objects_that_are_not_references_stay_literals() {
        for text in [json!({"$slot": 3}), json!({"$slot": "t", "extra": 1}), json!({"ptr": "/a"})] {
            assert_eq!(
                serde_json::from_value::<Operand>(text.clone()).unwrap(),
                Operand::Lit(text.clone()),
                "{text}"
            );
        }
    }
}

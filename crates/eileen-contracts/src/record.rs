//! Recording and replay formats (BUILD.md section 3.3.16): the simulator, the replayer, the
//! flight recorder, the stub harness and scenarios share them, and each parser of them has a
//! fuzz target.
//!
//! Large values in a recorded input are replaced by `{"$blob": "b3:..."}` ([`BlobRef`]), and
//! speech that was not addressed to Eileen by a placeholder, when the line is written.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::bus::FirstRunPhase;
use crate::core_io::{ApprovalRow, Input};
use crate::harness::HookReply;
use crate::ids::{
    IncidentId, JobId, MonoTime, ParseError, Personal, RecordingId, SessionId, TraceCtx, TurnId,
    WallTime,
};
use crate::jobs::{Job, PlanUsage};
use crate::learn::SkillRef;
use crate::schema::Registry;

/// A content-addressed blob: `b3:` and the BLAKE3 hash in 64 lower-case hex characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobRef(String);

impl BlobRef {
    /// The blob for a 32-byte BLAKE3 digest.
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(format!("b3:{}", crate::ids::hex_lower(&digest)))
    }

    /// The text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for BlobRef {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let valid = text.strip_prefix("b3:").is_some_and(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(ParseError::Invalid {
                what: "BlobRef",
                rule: "b3: and 64 lower-case hex characters",
            })
        }
    }
}

impl Serialize for BlobRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BlobRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<'de, str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for BlobRef {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("BlobRef")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^b3:[0-9a-f]{64}$",
            "description": "A content-addressed blob: b3: and its BLAKE3 hash",
        })
    }
}

/// One recorded input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputRecord {
    /// The core-wide replay order.
    pub seq: u64,
    /// Monotonic time.
    pub mono: MonoTime,
    /// Wall time.
    pub at: WallTime,
    /// Correlation.
    pub ctx: TraceCtx,
    /// The input.
    pub input: Input,
}

/// One line of `replay/inputs.jsonl` and of the flight recorder's input lane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordLine {
    /// An input.
    Input(Box<InputRecord>),
    /// The core's answer to a gating hook input.
    HookAnswer(HookAnswerRecord),
}

/// The core's `Effect::Hook` answer to a gating hook input (`PreToolUse` or
/// `PermissionRequest`), written by the recorder when the core emits it; `input` is the `seq`
/// of the `Input::Hook` record it answers. Scenario mode compares the live answer with it,
/// strict mode compares it like a bus event, and `record trim` keeps it with its input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HookAnswerRecord {
    /// The `seq` of the hook input it answers.
    pub input: u64,
    /// When the core answered.
    pub mono: MonoTime,
    /// The answer.
    pub reply: HookReply,
}

/// A point strict replay can start from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Checkpoint {
    /// The last input applied.
    pub seq: u64,
    /// Monotonic time then.
    pub mono: MonoTime,
    /// The core RNG's state.
    pub rng_state: [u64; 4],
    /// The core's state.
    pub core: CoreSnapshotV1,
}

/// Every field the core needs to resume. `eileen-core` converts to and from it; a new field is
/// added with a default, so old checkpoints still load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CoreSnapshotV1 {
    /// The snapshot's version, 1.
    pub v: u16,
    /// The jobs.
    pub jobs: Vec<Job>,
    /// Whether the microphone is muted.
    pub muted: bool,
    /// When the conversation window closes, if it is open.
    pub conversation_until: Option<MonoTime>,
    /// The open approval questions.
    pub approvals: Vec<ApprovalRow>,
    /// The job holding the mouse and keyboard.
    pub input_lock: Option<JobId>,
    /// The settings version.
    pub settings_version: u64,
    /// The skill versions the core has loaded.
    pub skill_versions: Vec<SkillRef>,
    /// The plan usage.
    pub plan_usage: PlanUsage,
    /// The first run's phase, while it runs.
    pub first_run: Option<FirstRunPhase>,
    /// The core's counters (timers, requests, questions), by name.
    pub counters: BTreeMap<String, u64>,
}

/// A recording's manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordingManifest {
    /// Its id.
    pub id: RecordingId,
    /// What made it.
    pub origin: Origin,
    /// When it started.
    pub started: WallTime,
    /// When it ended.
    pub ended: WallTime,
    /// The version of Eileen that recorded it.
    pub eileen_version: String,
    /// The commit that recorded it.
    pub recorded_at_commit: String,
    /// A hash of the configuration.
    pub config_hash: String,
    /// The audio lanes (opt in).
    pub audio: Vec<AudioLane>,
    /// The turns it holds.
    pub turns: Vec<TurnId>,
    /// The jobs it holds.
    pub jobs: Vec<JobId>,
}

/// What made a recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// `eileen record`.
    Record,
    /// An incident capture.
    Incident,
    /// `eileen scenario save`.
    Scenario,
    /// A fleet run.
    Fleet,
}

/// One audio lane of a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioLane {
    /// The audio source.
    pub source: u16,
    /// The file.
    pub file: String,
    /// Its consent record, when the audio is a person's own voice.
    pub consent: Option<String>,
}

/// An incident bundle's manifest (the observability report's section 4.3 fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IncidentManifest {
    /// Its id.
    pub id: IncidentId,
    /// What triggered it.
    pub trigger: String,
    /// The user's note.
    pub note: Option<Personal<String>>,
    /// When.
    pub at: WallTime,
    /// The core session.
    pub session: SessionId,
    /// The turns it holds.
    pub turns: Vec<TurnId>,
    /// The jobs it holds.
    pub jobs: Vec<JobId>,
    /// The version of Eileen.
    pub eileen_version: String,
    /// The commit.
    pub recorded_at_commit: String,
    /// The build profile.
    pub build_profile: String,
    /// The operating system.
    pub os: String,
    /// CPU load, percent.
    pub cpu_load_pct: u8,
    /// Free memory, MB.
    pub free_ram_mb: u32,
    /// The processes running.
    pub processes: Vec<String>,
    /// Whether it holds personal data.
    pub contains_personal: bool,
    /// Whether it holds audio.
    pub contains_audio: bool,
    /// How many values were redacted.
    pub redactions: u32,
    /// Minidumps in it.
    pub dumps: Vec<String>,
}

/// One line of `tests/harness-scripts/*.ndjson`, played by `eileen-stub-harness`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StubScriptLine {
    /// One stream-json line, emitted as is.
    Event {
        /// The line.
        json: Value,
    },
    /// Wait.
    Delay {
        /// How long, ms.
        ms: u32,
    },
    /// Exit with a code.
    Crash {
        /// The code.
        code: i32,
    },
    /// Emit bytes that do not parse.
    Malformed {
        /// The bytes, as text.
        bytes: String,
    },
    /// Ask for permission, as the CLI's hook would.
    PermissionRequest {
        /// The tool.
        tool_name: String,
        /// Its input.
        tool_input: Value,
    },
    /// Emit a narration line.
    Narration {
        /// The line.
        line: String,
    },
    /// Wait for the next user line before continuing.
    AwaitUser,
}

/// A minimised simulator failure, in `crates/eileen-sim/regressions/<name>.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimRegression {
    /// The sim profile.
    pub profile: String,
    /// The seed that found it.
    pub seed: Option<u64>,
    /// The commit that found it.
    pub found_at_commit: String,
    /// Decoded inputs, never a buffer of random bytes.
    pub inputs: Vec<InputRecord>,
    /// Each injected fault as an explicit record, never a buggify site ordinal.
    pub faults: Vec<FaultRecord>,
    /// The invariant that failed.
    pub invariant: String,
}

/// One injected fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FaultRecord {
    /// The input's `seq` it was injected at.
    pub seq: u64,
    /// The fault site's name.
    pub site: String,
    /// What was injected.
    pub fault: String,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<BlobRef>();
    r.add::<InputRecord>();
    r.add::<RecordLine>();
    r.add::<HookAnswerRecord>();
    r.add::<Checkpoint>();
    r.add::<CoreSnapshotV1>();
    r.add::<RecordingManifest>();
    r.add::<Origin>();
    r.add::<AudioLane>();
    r.add::<IncidentManifest>();
    r.add::<StubScriptLine>();
    r.add::<SimRegression>();
    r.add::<FaultRecord>();
}

#[cfg(test)]
mod tests {
    use super::{BlobRef, CoreSnapshotV1};

    #[test]
    fn blob_refs_are_b3_and_64_hex_characters() {
        let blob = BlobRef::from_digest([0xab; 32]);
        assert_eq!(blob.as_str(), format!("b3:{}", "ab".repeat(32)), "from a digest");
        assert_eq!(blob.as_str().parse::<BlobRef>(), Ok(blob.clone()), "round trip");
        assert!("b3:abc".parse::<BlobRef>().is_err(), "short");
        assert!(format!("sha:{}", "ab".repeat(32)).parse::<BlobRef>().is_err(), "prefix");
        assert!(format!("b3:{}", "AB".repeat(32)).parse::<BlobRef>().is_err(), "upper case");
    }

    #[test]
    fn an_old_checkpoint_without_newer_fields_still_loads() {
        let snapshot: CoreSnapshotV1 = serde_json::from_str(r#"{"v": 1, "muted": true}"#).unwrap();
        assert!(snapshot.muted, "the field it had");
        assert!(snapshot.jobs.is_empty() && snapshot.counters.is_empty(), "the rest default");
    }
}

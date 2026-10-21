//! Recording and replay formats (BUILD.md section 3.3.16): the simulator, the replayer, the
//! flight recorder, the stub harness and scenarios share them, and each parser of them has a
//! fuzz target.
//!
//! When a line is written, speech that was not addressed to Eileen is replaced by a placeholder,
//! and large values by blob stubs. A written line is the JSON of a [`RecordLine`] in which any
//! string longer than the writer's limit may stand as `{"$blob": "b3:..."}` (a [`BlobRef`] into
//! the blob store), wherever the string sits: an image's base64, a tool's text result, a large
//! argument inside a `serde_json::Value`. An object of the line that would read as a stub (its
//! only key is `$blob`, or `$lit`) is written as `{"$lit": <it>}`, so nothing a model or a page
//! wrote is ever taken for a stub. [`stub_blobs`] writes this form and [`resolve_blobs`] reads
//! it back before the line is parsed, so the types never see a stub, and the two are exact
//! inverses for every JSON value.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

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
        let valid = text
            .strip_prefix("b3:")
            .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(crate::ids::is_lower_hex));
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

/// The key of a blob stub, `{"$blob": "b3:..."}`, which stands for a large string in a written
/// line.
pub const BLOB_KEY: &str = "$blob";

/// The key that escapes a written object which would otherwise read as a stub or an escape:
/// `{"$lit": {...}}` holds the object itself.
pub const LITERAL_KEY: &str = "$lit";

/// Why a written line's stubs could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobError {
    /// A stub names a blob the store does not hold.
    Missing(BlobRef),
    /// A `$blob` that is not a blob reference, or a `$lit` that does not hold an object.
    Malformed,
}

impl fmt::Display for BlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(blob) => write!(f, "the blob {blob} is not in the store"),
            Self::Malformed => f.write_str("a $blob or $lit object of the wrong shape"),
        }
    }
}

impl std::error::Error for BlobError {}

/// The one key of a single-key object.
fn single_key(map: &Map<String, Value>) -> Option<&str> {
    let mut keys = map.keys();
    match (keys.next(), keys.next()) {
        (Some(key), None) => Some(key.as_str()),
        _ => None,
    }
}

/// Writes a line's JSON with every string longer than `max_inline` bytes moved to the blob
/// store: `store` keeps the string and returns its [`BlobRef`], and the string's place holds
/// `{"$blob": "b3:..."}`. An object of the line whose one key is `$blob` or `$lit` is written
/// as `{"$lit": <it>}`, so [`resolve_blobs`] gives back exactly the value it was given.
pub fn stub_blobs<F>(value: Value, max_inline: usize, store: &mut F) -> Value
where
    F: FnMut(String) -> BlobRef,
{
    match value {
        Value::String(text) if text.len() > max_inline => {
            let mut stub = Map::new();
            stub.insert(BLOB_KEY.to_owned(), Value::String(store(text).0));
            Value::Object(stub)
        }
        Value::Array(items) => Value::Array(
            items.into_iter().map(|item| stub_blobs(item, max_inline, store)).collect(),
        ),
        Value::Object(map) => {
            let reserved = matches!(single_key(&map), Some(BLOB_KEY | LITERAL_KEY));
            let written: Map<String, Value> = map
                .into_iter()
                .map(|(key, field)| (key, stub_blobs(field, max_inline, store)))
                .collect();
            if reserved {
                let mut escaped = Map::new();
                escaped.insert(LITERAL_KEY.to_owned(), Value::Object(written));
                Value::Object(escaped)
            } else {
                Value::Object(written)
            }
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

/// Reads a written line's JSON back: every stub is replaced by the string `fetch` returns for
/// it, and every `$lit` escape by the object it holds. Run it before the line is parsed into a
/// [`RecordLine`], whose types never see a stub.
pub fn resolve_blobs<F>(value: Value, fetch: &mut F) -> Result<Value, BlobError>
where
    F: FnMut(&BlobRef) -> Option<String>,
{
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| resolve_blobs(item, fetch))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Value::Object(mut map) => match single_key(&map) {
            Some(BLOB_KEY) => {
                let reference = map.remove(BLOB_KEY);
                let blob: BlobRef = match reference {
                    Some(Value::String(text)) => text.parse().map_err(|_| BlobError::Malformed)?,
                    _ => return Err(BlobError::Malformed),
                };
                fetch(&blob).map(Value::String).ok_or(BlobError::Missing(blob))
            }
            Some(LITERAL_KEY) => match map.remove(LITERAL_KEY) {
                Some(Value::Object(inner)) => resolve_fields(inner, fetch),
                _ => Err(BlobError::Malformed),
            },
            Some(_) | None => resolve_fields(map, fetch),
        },
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => Ok(other),
    }
}

/// Resolves each field of an object, keeping the object itself as it is.
fn resolve_fields<F>(map: Map<String, Value>, fetch: &mut F) -> Result<Value, BlobError>
where
    F: FnMut(&BlobRef) -> Option<String>,
{
    map.into_iter()
        .map(|(key, field)| resolve_blobs(field, fetch).map(|field| (key, field)))
        .collect::<Result<Map<_, _>, _>>()
        .map(Value::Object)
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

/// A minimised simulator failure, in `crates/eileen-sim/regressions/<name>.toml`. TOML has no
/// `null` and no integer above `i64::MAX`, so two fields are written in forms it can hold: each
/// input as one line of JSON (an `InputRecord`, as `replay/inputs.jsonl` writes it), and the
/// seed as a decimal string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SimRegression {
    /// The sim profile.
    pub profile: String,
    /// The seed that found it, as a decimal string.
    #[serde(default, with = "seed_text")]
    #[schemars(with = "Option<String>", regex(pattern = r"^[0-9]{1,20}$"))]
    pub seed: Option<u64>,
    /// The commit that found it.
    pub found_at_commit: String,
    /// Decoded inputs, never a buffer of random bytes: each one line of JSON.
    #[serde(with = "inputs_as_json")]
    #[schemars(schema_with = "inputs_schema")]
    pub inputs: Vec<InputRecord>,
    /// Each injected fault as an explicit record, never a buggify site ordinal.
    pub faults: Vec<FaultRecord>,
    /// The invariant that failed.
    pub invariant: String,
}

/// The schema of [`SimRegression::inputs`]: strings that each hold one JSON `InputRecord`.
fn inputs_schema(generator: &mut SchemaGenerator) -> Schema {
    let record = generator.subschema_for::<InputRecord>();
    json_schema!({
        "type": "array",
        "items": {
            "type": "string",
            "contentMediaType": "application/json",
            "contentSchema": record,
        },
    })
}

/// [`SimRegression::inputs`] as strings of JSON.
mod inputs_as_json {
    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    use super::InputRecord;

    pub(super) fn serialize<S: Serializer>(
        inputs: &[InputRecord],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let lines = inputs
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(S::Error::custom)?;
        serializer.collect_seq(lines)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<InputRecord>, D::Error> {
        Vec::<String>::deserialize(deserializer)?
            .iter()
            .map(|line| serde_json::from_str(line).map_err(D::Error::custom))
            .collect()
    }
}

/// [`SimRegression::seed`] as a decimal string; an integer is read too.
mod seed_text {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seed {
        Text(String),
        Number(u64),
    }

    #[expect(clippy::ref_option, reason = "serde's `with` passes the field by reference")]
    pub(super) fn serialize<S: Serializer>(
        seed: &Option<u64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match seed {
            Some(seed) => serializer.serialize_some(&seed.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        match Option::<Seed>::deserialize(deserializer)? {
            None => Ok(None),
            Some(Seed::Number(seed)) => Ok(Some(seed)),
            Some(Seed::Text(text)) => text.parse().map(Some).map_err(D::Error::custom),
        }
    }
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
    use std::collections::BTreeMap;

    use proptest::prelude::*;
    use serde_json::{Value, json};

    use super::{
        BlobError, BlobRef, CoreSnapshotV1, FaultRecord, InputRecord, RecordLine, SimRegression,
        resolve_blobs, stub_blobs,
    };
    use crate::connectors::{ConnectorEvent, SidecarEvent, SidecarId};
    use crate::core_io::{Caller, Content, Input, ToolCallRequest, ToolResult};
    use crate::desktop::{DesktopChange, DesktopDelta};
    use crate::ids::{MonoTime, ToolName, TraceCtx, Untrusted, WallTime};
    use crate::jobs::CallOrigin;
    use crate::speech::VoiceEvent;

    fn record(seq: u64, input: Input) -> InputRecord {
        InputRecord {
            seq,
            mono: MonoTime(seq * 1_000_000),
            at: WallTime(1_792_627_199_000_000),
            ctx: TraceCtx::default(),
            input,
        }
    }

    /// Inputs whose JSON holds a `null` in a position TOML has no form for: a tool call's
    /// arguments, and an enum variant whose only payload is `None`.
    fn awkward_inputs() -> Vec<InputRecord> {
        let call = ToolCallRequest {
            caller: Caller::Internal,
            origin: CallOrigin::Core,
            tool: ToolName::new("apps_media").unwrap(),
            args: Untrusted::new(json!({ "action": "status", "session": null })),
            paths: Vec::new(),
            urls: Vec::new(),
            traceparent: None,
        };
        let foreground = DesktopDelta { seq: 3, changes: vec![DesktopChange::Foreground(None)] };
        vec![
            record(1, Input::ToolCall(call)),
            record(2, Input::Voice(VoiceEvent::Calibration(None))),
            record(
                3,
                Input::Sidecar(
                    SidecarId::Desktop,
                    SidecarEvent::Event(ConnectorEvent::Desktop(foreground)),
                ),
            ),
        ]
    }

    #[test]
    fn a_regression_round_trips_through_its_toml_file() {
        let regression = SimRegression {
            profile: "default".to_owned(),
            seed: Some(u64::MAX),
            found_at_commit: "8444b4f".to_owned(),
            inputs: awkward_inputs(),
            faults: vec![FaultRecord {
                seq: 2,
                site: "brain.stall".to_owned(),
                fault: "delay 5000 ms".to_owned(),
            }],
            invariant: "one_brain_answer_per_turn".to_owned(),
        };
        let text = toml::to_string(&regression).unwrap();
        assert_eq!(toml::from_str::<SimRegression>(&text).unwrap(), regression, "{text}");
        let unseeded = SimRegression { seed: None, inputs: Vec::new(), ..regression };
        let text = toml::to_string(&unseeded).unwrap();
        assert_eq!(toml::from_str::<SimRegression>(&text).unwrap(), unseeded, "{text}");
    }

    #[test]
    fn blob_refs_are_b3_and_64_hex_characters() {
        let blob = BlobRef::from_digest([0xab; 32]);
        assert_eq!(blob.as_str(), format!("b3:{}", "ab".repeat(32)), "from a digest");
        assert_eq!(blob.as_str().parse::<BlobRef>(), Ok(blob.clone()), "round trip");
        assert!("b3:abc".parse::<BlobRef>().is_err(), "short");
        assert!(format!("sha:{}", "ab".repeat(32)).parse::<BlobRef>().is_err(), "prefix");
        assert!(format!("b3:{}", "AB".repeat(32)).parse::<BlobRef>().is_err(), "upper case");
    }

    /// An in-memory blob store: each string gets the next numbered reference.
    #[derive(Default)]
    struct Store(BTreeMap<BlobRef, String>);

    impl Store {
        fn put(&mut self, text: String) -> BlobRef {
            let mut digest = [0_u8; 32];
            digest[..8].copy_from_slice(&u64::try_from(self.0.len()).unwrap().to_le_bytes());
            let blob = BlobRef::from_digest(digest);
            self.0.insert(blob.clone(), text);
            blob
        }

        fn get(&self, blob: &BlobRef) -> Option<String> {
            self.0.get(blob).cloned()
        }
    }

    /// Writes a line's JSON with stubs for strings over 100 bytes, as text, and reads it back.
    fn write_and_read(value: Value) -> (Value, Result<Value, BlobError>) {
        let mut store = Store::default();
        let written = stub_blobs(value, 100, &mut |text| store.put(text));
        let text = serde_json::to_string(&written).unwrap();
        let read = resolve_blobs(serde_json::from_str(&text).unwrap(), &mut |blob| store.get(blob));
        (written, read)
    }

    /// JSON with long strings and objects keyed like stubs and escapes.
    fn json_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(Value::from),
            "[a-z:0-9]{0,12}".prop_map(Value::String),
            "x{90,110}".prop_map(Value::String),
            Just(Value::String(format!("b3:{}", "ab".repeat(32)))),
        ];
        leaf.prop_recursive(4, 32, 4, |inner| {
            let key = prop::sample::select(vec!["$blob", "$lit", "a", "b"]);
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                prop::collection::vec((key, inner), 0..3).prop_map(|pairs| {
                    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
                }),
            ]
        })
    }

    proptest! {
        #[test]
        fn stubs_resolve_to_exactly_the_value_written(value in json_value()) {
            prop_assert_eq!(write_and_read(value.clone()).1, Ok(value));
        }
    }

    #[test]
    fn a_line_with_a_stubbed_image_and_a_forged_stub_reads_back_whole() {
        let image = Content::Image { mime: "image/png".to_owned(), base64: "iVBOR".repeat(2000) };
        let result = ToolResult {
            content: vec![image],
            is_error: false,
            verification: None,
            output: None,
            duration_ms: 40,
            rung: None,
        };
        let call = "call_01K8ZQ3V2X4Y5Z6A7B8C9D0E1F".parse().unwrap();
        let forged = json!({ "$blob": format!("b3:{}", "cd".repeat(32)) });
        let request = ToolCallRequest {
            caller: Caller::Internal,
            origin: CallOrigin::Core,
            tool: ToolName::new("apps_open_path").unwrap(),
            args: Untrusted::new(json!({ "path": forged })),
            paths: Vec::new(),
            urls: Vec::new(),
            traceparent: None,
        };
        for (line, pointer, expected) in [
            (
                RecordLine::Input(Box::new(record(1, Input::ToolResult(call, result)))),
                "/input/input/tool_result/1/content/0/image/base64",
                "a stub",
            ),
            (
                RecordLine::Input(Box::new(record(2, Input::ToolCall(request)))),
                "/input/input/tool_call/args/path",
                "an escaped object",
            ),
        ] {
            let (written, read) = write_and_read(serde_json::to_value(&line).unwrap());
            let at = written.pointer(pointer).unwrap();
            match expected {
                "a stub" => assert!(at["$blob"].as_str().unwrap().starts_with("b3:"), "{at}"),
                _ => assert_eq!(at, &json!({ "$lit": forged }), "escaped"),
            }
            let parsed: RecordLine = serde_json::from_value(read.unwrap()).unwrap();
            assert_eq!(parsed, line, "{expected} reads back");
        }
    }

    #[test]
    fn a_missing_or_malformed_stub_is_an_error() {
        let resolve = |value: Value| resolve_blobs(value, &mut |_: &BlobRef| None);
        let blob = BlobRef::from_digest([0xab; 32]);
        assert_eq!(
            resolve(json!({ "$blob": blob.as_str() })),
            Err(BlobError::Missing(blob)),
            "missing"
        );
        assert_eq!(
            resolve(json!({ "$blob": "b3:short" })),
            Err(BlobError::Malformed),
            "a bad reference"
        );
        assert_eq!(
            resolve(json!({ "$lit": 3 })),
            Err(BlobError::Malformed),
            "an escape of no object"
        );
        assert_eq!(
            resolve(json!({ "$blob": 1, "a": 2 })),
            Ok(json!({ "$blob": 1, "a": 2 })),
            "two keys"
        );
    }

    #[test]
    fn an_old_checkpoint_without_newer_fields_still_loads() {
        let snapshot: CoreSnapshotV1 = serde_json::from_str(r#"{"v": 1, "muted": true}"#).unwrap();
        assert!(snapshot.muted, "the field it had");
        assert!(snapshot.jobs.is_empty() && snapshot.counters.is_empty(), "the rest default");
    }
}

//! The sections of [`super::Config`], with the defaults BUILD.md states. Where the plan names a
//! setting without a default, the default is the conservative one (off, unbound, or the default
//! model in `models.toml`) and WP7 settles it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::core_io::Rung;
use crate::desktop::FieldCategory;
use crate::fleet::FleetMode;
use crate::hud::Corner;
use crate::ids::ApprovalId;
use crate::jobs::{Depth, GrantMatcher, JobKind, ObservedSource, PoolClass, Route, Tier};
use crate::learn::{LadderPolicy, PackageType};
use crate::schema::Registry;

/// A setting that is `"auto"` or a value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoOr<T> {
    /// Eileen computes it.
    #[default]
    Auto,
    /// This value.
    Value(T),
}

impl<T: Serialize> Serialize for AutoOr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for AutoOr<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr<T> {
            Word(String),
            Value(T),
        }
        match Repr::<T>::deserialize(deserializer)? {
            Repr::Word(word) if word == "auto" => Ok(Self::Auto),
            Repr::Word(word) => {
                Err(serde::de::Error::custom(format!("expected \"auto\" or a value, not {word:?}")))
            }
            Repr::Value(value) => Ok(Self::Value(value)),
        }
    }
}

impl<T: JsonSchema> JsonSchema for AutoOr<T> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("AutoOr_{}", T::schema_name()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let value = generator.subschema_for::<T>();
        json_schema!({ "anyOf": [{ "const": "auto" }, value] })
    }
}

/// The assistant's name and manner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AssistantConfig {
    /// The name it answers to; a change runs the `NameRecognisable` check.
    pub name: String,
    /// Free text, at most 400 characters.
    pub personality: String,
    /// How much it says.
    pub verbosity: Verbosity,
    /// How playful it is.
    pub humor: Humor,
}

impl Default for AssistantConfig {
    fn default() -> Self {
        Self {
            name: "Eileen".to_owned(),
            personality: String::new(),
            verbosity: Verbosity::Normal,
            humor: Humor::Light,
        }
    }
}

/// How much the assistant says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verbosity {
    /// As little as works.
    Brief,
    /// A sentence or two.
    #[default]
    Normal,
    /// Fuller answers.
    Detailed,
}

/// How playful the assistant is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Humor {
    /// None.
    Off,
    /// A little.
    #[default]
    Light,
    /// Playful.
    Playful,
}

/// Voices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct VoiceConfig {
    /// The conversation's voice; `None` is the default voice in `models.toml`.
    pub live: Option<String>,
    /// The voice for narration; the live voice when unset.
    pub narration: Option<String>,
    /// The voice for announcements; the live voice when unset.
    pub announcements: Option<String>,
    /// The voice to fall back to when a cloned voice cannot render in time.
    pub fallback: Option<String>,
    /// Speaking rate, percent.
    pub rate_pct: u16,
    /// Mark saved and exported audio (`eileen voice render` files and the audio lanes of
    /// recordings) with the AudioSeal watermark. Opt in: no engine watermarks by default, and
    /// turning this on is what makes `eileen setup` fetch the AudioSeal models.
    pub watermark_exports: bool,
    /// The engine per cloned voice name.
    pub engines: BTreeMap<String, VoiceEngine>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            live: None,
            narration: None,
            announcements: None,
            fallback: None,
            rate_pct: 100,
            watermark_exports: false,
            engines: BTreeMap::new(),
        }
    }
}

/// The engines a cloned voice can render with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceEngine {
    /// Pocket TTS.
    Pocket,
    /// Chatterbox Turbo.
    ChatterboxTurbo,
    /// Chatterbox Nano.
    ChatterboxNano,
}

/// The HUD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct HudConfig {
    /// The screen corner.
    pub corner: Corner,
    /// Scale, percent.
    pub scale_pct: u16,
    /// Opacity, percent.
    pub opacity_pct: u8,
    /// Show what was heard.
    pub show_transcript: bool,
    /// Show suggestions.
    pub suggestions: bool,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self {
            corner: Corner::BottomRight,
            scale_pct: 100,
            opacity_pct: 90,
            show_transcript: true,
            suggestions: true,
        }
    }
}

/// The conversation window and the addressee score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ConversationConfig {
    /// How long the window stays open after Eileen's last turn, seconds.
    pub window_s: u16,
    /// How long an unnamed follow-up counts, seconds.
    pub follow_up_s: u16,
    /// Score unnamed speech when the user is alone.
    pub alone_scoring: bool,
    /// The points an unnamed utterance needs to be addressed.
    pub addressee_min_points: u8,
    /// How far under the reference level a turn may be, dB.
    pub level_margin_db: u8,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            window_s: 45,
            follow_up_s: 8,
            alone_scoring: true,
            addressee_min_points: 2,
            level_margin_db: 6,
        }
    }
}

/// How each event kind is announced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AnnouncementsConfig {
    /// Per dotted event name.
    pub events: BTreeMap<String, AnnounceMode>,
    /// Per job kind, overriding the event's mode.
    pub job_kinds: BTreeMap<JobKind, AnnounceMode>,
}

/// How an event is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnounceMode {
    /// Speak at once, interrupting.
    Interrupt,
    /// Speak when the conversation is idle.
    WhenIdle,
    /// Do not speak.
    Silent,
    /// Show on the HUD only.
    HudOnly,
}

/// Approvals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ApprovalsConfig {
    /// How long a question can be answered by voice, seconds.
    pub voice_window_s: u16,
    /// When an open question is declined, minutes.
    pub deadline_min: u16,
    /// How long a hook waits for an answer, seconds.
    pub hook_hold_s: u16,
    /// Tier 2 needs the action word by voice.
    pub action_word_tier2: bool,
}

impl Default for ApprovalsConfig {
    fn default() -> Self {
        Self { voice_window_s: 20, deadline_min: 30, hook_hold_s: 120, action_word_tier2: true }
    }
}

/// Models, backends and routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RoutingConfig {
    /// Model names to vendor aliases or full ids.
    pub models: BTreeMap<String, String>,
    /// The brain.
    pub brain: BackendChoice,
    /// Deep answers.
    pub deep: BackendChoice,
    /// The brain's fallback slot.
    pub fallback_brain: BackendChoice,
    /// Per job kind.
    pub kinds: BTreeMap<JobKind, KindRouting>,
    /// Keep a warm spare brain.
    pub hot_spare: bool,
    /// When the brain's session is recycled.
    pub recycle: RecycleThresholds,
    /// Start the brain on a partial transcript.
    pub speculative: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        let model = |name: &str| BackendChoice { backend: Route::Claude, model: name.to_owned() };
        Self {
            models: [("fast", "haiku"), ("mid", "sonnet"), ("strong", "opus")]
                .into_iter()
                .map(|(name, alias)| (name.to_owned(), alias.to_owned()))
                .collect(),
            brain: model("fast"),
            deep: model("strong"),
            fallback_brain: model("fast"),
            kinds: BTreeMap::new(),
            hot_spare: true,
            recycle: RecycleThresholds::default(),
            speculative: false,
        }
    }
}

/// A backend and a model name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BackendChoice {
    /// The route.
    pub backend: Route,
    /// A name from `models`, or a full id.
    pub model: String,
}

/// Routing for one job kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct KindRouting {
    /// The route.
    pub route: Option<Route>,
    /// A name from `models`, or a full id.
    pub model: Option<String>,
    /// The depth.
    pub depth: Option<Depth>,
}

/// When the brain's session is recycled (section 2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RecycleThresholds {
    /// The context size reported in the `result` usage, tokens.
    pub context_tokens: u64,
}

impl Default for RecycleThresholds {
    fn default() -> Self {
        Self { context_tokens: 60_000 }
    }
}

/// Anticipated deep answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AnticipationConfig {
    /// Whether it is on.
    pub enabled: bool,
    /// Only below this plan usage, percent.
    pub max_plan_pct: u8,
}

impl Default for AnticipationConfig {
    fn default() -> Self {
        Self { enabled: false, max_plan_pct: 50 }
    }
}

/// Memory admission and the pools (section 2.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MemoryConfig {
    /// The memory profile.
    pub profile: MemoryProfile,
    /// Memory kept free, MB; `auto` is 10 percent of RAM, 768 to 2048.
    pub reserve_mb: AutoOr<u32>,
    /// The paging limit, KB/s; `auto` is the 75th percentile plus 25 percent of 7 days of
    /// machine-wide 30 s averages outside build slots, clamped to 2,000 to 40,000; the build
    /// guard's baseline until a day exists; 5,000 when there is neither
    /// (`samples::auto_paging_limit`).
    pub max_paging_kb_s: AutoOr<u32>,
    /// Ask before queueing for longer than this, minutes.
    pub queue_ask_min: u16,
    /// Warm computer-use sessions.
    pub pool_computer_use: u8,
    /// Warm deep-answer sessions.
    pub pool_deep: u8,
    /// Which pool fills first.
    pub pool_order: Vec<PoolClass>,
    /// Build-heavy jobs at once.
    pub max_build_heavy_jobs: u8,
    /// A worker's Job Object memory limit, MB; 0 is off.
    pub worker_job_limit_mb: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            profile: MemoryProfile::Auto,
            reserve_mb: AutoOr::Auto,
            max_paging_kb_s: AutoOr::Auto,
            queue_ask_min: 10,
            pool_computer_use: 1,
            pool_deep: 1,
            pool_order: vec![PoolClass::Deep, PoolClass::ComputerUse],
            max_build_heavy_jobs: 1,
            worker_job_limit_mb: 0,
        }
    }
}

/// The memory profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProfile {
    /// Chosen from the PC's memory.
    #[default]
    Auto,
    /// Every model at full size.
    Full,
    /// The low-memory models.
    Low,
}

/// Standing grants and app permissions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PermissionsConfig {
    /// Grants that last (tier 1, or tier 2 with a logged decision).
    pub grants: Vec<ConfigGrant>,
    /// Apps whose windows are private.
    pub private_apps: Vec<String>,
    /// Apps workers may drive.
    pub desktop_apps: Vec<String>,
    /// App protocols the user approved for `apps_open_uri`.
    pub uri_schemes: Vec<String>,
}

/// A grant that lasts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigGrant {
    /// What it matches.
    pub matcher: GrantMatcher,
    /// The highest tier it allows.
    pub tier: Tier,
    /// The logged decision, required for tier 2.
    pub decision: Option<ApprovalId>,
}

/// Desktop control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DesktopConfig {
    /// A lease ends after this much idle time, seconds.
    pub lease_idle_s: u16,
    /// How long the next click's target shows first, ms.
    pub visible_intent_ms: u16,
    /// Read web pages' trees.
    pub web_tree: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self { lease_idle_s: 5, visible_intent_ms: 400, web_tree: true }
    }
}

/// Hotkeys, as chords such as `ctrl+alt+m`; `None` is unbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct HotkeysConfig {
    /// Mute and unmute.
    pub mute: Option<String>,
    /// Save an incident report.
    pub report: Option<String>,
    /// Approve the pending question.
    pub approve: Option<String>,
    /// Deny the pending question.
    pub deny: Option<String>,
    /// Hands off: stop every desktop job.
    pub hands_off: Option<String>,
    /// Hold to dictate.
    pub dictation: Option<String>,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            mute: Some("ctrl+alt+m".to_owned()),
            report: Some("ctrl+alt+shift+b".to_owned()),
            approve: None,
            deny: None,
            hands_off: None,
            dictation: None,
        }
    }
}

/// Dictation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DictationConfig {
    /// Clean up fillers and self-corrections.
    pub polish: bool,
    /// Whether to polish, per field category.
    pub categories: BTreeMap<FieldCategory, bool>,
    /// The dictation key; the `dictation` hotkey when unset.
    pub key: Option<String>,
    /// Languages to dictate in, as BCP 47 tags; the recognizer's own when empty.
    pub languages: Vec<String>,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self { polish: true, categories: BTreeMap::new(), key: None, languages: Vec::new() }
    }
}

/// The learning loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LearningConfig {
    /// Whether it is on.
    pub enabled: bool,
    /// Who distills skills.
    pub distill: Distill,
    /// The librarian runs only below this plan usage, percent.
    pub librarian_max_plan_pct: u8,
    /// Skills in a session's snapshot.
    pub snapshot_k: u8,
    /// How long "no" or "undo that" counts against a job, seconds.
    pub regret_window_s: u16,
    /// Uses before a paraphrase becomes a phrasing.
    pub phrasing_paraphrase_uses: u8,
    /// The similarity a confirmed phrasing needs, 0 to 1 (0.85 until WP7 measures it).
    pub phrasing_min_similarity: f64,
    /// The promotion ladder.
    pub ladder: LadderPolicy,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            distill: Distill::Claude,
            librarian_max_plan_pct: 50,
            snapshot_k: 12,
            regret_window_s: 120,
            phrasing_paraphrase_uses: 2,
            phrasing_min_similarity: 0.85,
            ladder: LadderPolicy::default(),
        }
    }
}

/// Who distills skills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Distill {
    /// The librarian, on Claude.
    #[default]
    Claude,
    /// Nobody.
    Off,
}

/// Approved third-party MCP servers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct McpConfig {
    /// Per app.
    pub servers: BTreeMap<String, McpServerConfig>,
}

/// One approved server, installed once at its exact version in
/// `%LOCALAPPDATA%\eileen\mcp\<app>\`. The tool names it lists go to `cache\mcp-tools.json`,
/// not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct McpServerConfig {
    /// The registry's id for it.
    pub registry_id: String,
    /// The package type.
    pub package_type: PackageType,
    /// The exact version.
    pub version: String,
    /// The command.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// The install's lockfile or file digest.
    pub digest: String,
    /// The job kinds that get it.
    pub job_kinds: Vec<JobKind>,
    /// Its rung: `app_mcp` or `dom`.
    pub rung: Rung,
    /// What its results count as (web by default, section 2.12).
    pub observes: ObservedSource,
}

/// Privacy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PrivacyConfig {
    /// The user's location, as they said it, for "near me".
    pub location: Option<String>,
    /// Off the record: nothing is learned or kept.
    pub off_the_record: bool,
    /// How long the ledger keeps lines, days.
    pub ledger_days: u16,
    /// Keep unaddressed speech in persisted sinks.
    pub keep_unaddressed: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self { location: None, off_the_record: false, ledger_days: 14, keep_unaddressed: false }
    }
}

/// When Eileen keeps quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FocusConfig {
    /// During calls.
    pub quiet_during_calls: bool,
    /// While an app is full screen.
    pub quiet_full_screen: bool,
    /// While the screen is recorded.
    pub quiet_recording: bool,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self { quiet_during_calls: true, quiet_full_screen: true, quiet_recording: true }
    }
}

/// The first run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FirstRunConfig {
    /// Whether it has run.
    pub done: bool,
    /// The clone's longest time to first audio, ms.
    pub max_clone_ttfa_ms: u32,
}

impl Default for FirstRunConfig {
    fn default() -> Self {
        Self { done: false, max_clone_ttfa_ms: 3000 }
    }
}

/// Speech engines, by their names in `models.toml`; `None` is the default there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SpeechConfig {
    /// Voice activity detection.
    pub vad: Option<String>,
    /// Recognition.
    pub asr: Option<String>,
    /// End of turn.
    pub turn: Option<String>,
    /// Pin the speech process's working set.
    pub pin_working_set: bool,
    /// Use the low-memory models.
    pub low_memory_models: bool,
}

/// The `eileen` Claude plugin.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ClaudePluginConfig {
    /// Offer learned skills to the owner's own sessions.
    pub share_skills: bool,
}

/// Logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LoggingConfig {
    /// The filter, in `tracing`'s syntax.
    pub level: String,
    /// How long log files are kept, days.
    pub keep_days: u16,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { level: "info".to_owned(), keep_days: 14 }
    }
}

/// The flight recorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FlightConfig {
    /// How many minutes it keeps.
    pub minutes: u16,
}

impl Default for FlightConfig {
    fn default() -> Self {
        Self { minutes: 10 }
    }
}

/// Incident bundles (retention, section 2.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct IncidentsConfig {
    /// How long they are kept, days.
    pub keep_days: u16,
    /// Their total size cap, MB.
    pub max_mb: u32,
}

impl Default for IncidentsConfig {
    fn default() -> Self {
        Self { keep_days: 30, max_mb: 1024 }
    }
}

/// Recordings (retention, section 2.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RecordingConfig {
    /// Record the audio lanes too (opt in).
    pub audio: bool,
    /// How long they are kept, days.
    pub keep_days: u16,
    /// Their total size cap, MB.
    pub max_mb: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self { audio: false, keep_days: 7, max_mb: 2048 }
    }
}

/// The test fleet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FleetConfig {
    /// The contributor opted in.
    pub opt_in: bool,
    /// A nightly schedule, as `HH:MM` local time.
    pub schedule: Option<String>,
    /// Offer to upload results after a run.
    pub upload: bool,
    /// Recorded (no model) or live.
    pub mode: FleetMode,
    /// Model turns a live run may spend.
    pub turn_budget: u32,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            opt_in: false,
            schedule: None,
            upload: false,
            mode: FleetMode::Recorded,
            turn_budget: 0,
        }
    }
}

pub(super) fn register(r: &mut Registry) {
    r.add::<AssistantConfig>();
    r.add::<Verbosity>();
    r.add::<Humor>();
    r.add::<VoiceConfig>();
    r.add::<VoiceEngine>();
    r.add::<HudConfig>();
    r.add::<ConversationConfig>();
    r.add::<AnnouncementsConfig>();
    r.add::<AnnounceMode>();
    r.add::<ApprovalsConfig>();
    r.add::<RoutingConfig>();
    r.add::<BackendChoice>();
    r.add::<KindRouting>();
    r.add::<RecycleThresholds>();
    r.add::<AnticipationConfig>();
    r.add::<MemoryConfig>();
    r.add::<AutoOr<u32>>();
    r.add::<MemoryProfile>();
    r.add::<PermissionsConfig>();
    r.add::<ConfigGrant>();
    r.add::<DesktopConfig>();
    r.add::<HotkeysConfig>();
    r.add::<DictationConfig>();
    r.add::<LearningConfig>();
    r.add::<Distill>();
    r.add::<McpConfig>();
    r.add::<McpServerConfig>();
    r.add::<PrivacyConfig>();
    r.add::<FocusConfig>();
    r.add::<FirstRunConfig>();
    r.add::<SpeechConfig>();
    r.add::<ClaudePluginConfig>();
    r.add::<LoggingConfig>();
    r.add::<FlightConfig>();
    r.add::<IncidentsConfig>();
    r.add::<RecordingConfig>();
    r.add::<FleetConfig>();
}

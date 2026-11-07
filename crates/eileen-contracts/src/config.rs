//! Settings and configuration (BUILD.md section 3.3.11): the whole configuration as types, and
//! the data of the settings flow (propose, check, confirm, apply, version, undo).
//!
//! Provisional: WP7 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! The doc comments become the commented default file. Every section defaults when it is
//! missing, so a config file names only what it changes. `eileen-config` holds the registry of
//! setting specs (with `&'static` keys), loading, validation and comment-keeping edits; the
//! core sees a spec through [`SettingSpecView`].

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{ApprovalId, ProposalId, WallTime};
use crate::schema::Registry;

mod auto_or;
mod sections;

pub use auto_or::AutoOr;
pub use sections::{
    AnnounceMode, AnnouncementsConfig, AnticipationConfig, ApprovalsConfig, AssistantConfig,
    BackendChoice, BenchMode, ClaudePluginConfig, ConfigGrant, ConversationConfig, DesktopConfig,
    DictationConfig, Distill, FirstRunConfig, FleetConfig, FlightConfig, FocusConfig,
    HotkeysConfig, HudConfig, Humor, IncidentsConfig, KindRouting, LearningConfig, LoggingConfig,
    MachineConfig, McpConfig, McpServerConfig, MemoryConfig, MemoryProfile, PermissionsConfig,
    PrivacyConfig, RecordingConfig, RecycleThresholds, RoutingConfig, SpeechConfig, Verbosity,
    VoiceConfig, VoiceEngine,
};

/// The whole configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Config {
    /// The assistant's name and manner.
    pub assistant: AssistantConfig,
    /// Voices.
    pub voice: VoiceConfig,
    /// The HUD.
    pub hud: HudConfig,
    /// The conversation window and the addressee score.
    pub conversation: ConversationConfig,
    /// How each event kind is announced.
    pub announcements: AnnouncementsConfig,
    /// Approvals.
    pub approvals: ApprovalsConfig,
    /// Models, backends and routes.
    pub routing: RoutingConfig,
    /// Anticipated deep answers.
    pub anticipation: AnticipationConfig,
    /// Memory admission and the pools.
    pub memory: MemoryConfig,
    /// Standing grants and app permissions.
    pub permissions: PermissionsConfig,
    /// Desktop control.
    pub desktop: DesktopConfig,
    /// Hotkeys.
    pub hotkeys: HotkeysConfig,
    /// Dictation.
    pub dictation: DictationConfig,
    /// The learning loop.
    pub learning: LearningConfig,
    /// Approved third-party MCP servers.
    pub mcp: McpConfig,
    /// Privacy.
    pub privacy: PrivacyConfig,
    /// When Eileen keeps quiet.
    pub focus: FocusConfig,
    /// The first run.
    pub first_run: FirstRunConfig,
    /// Speech engines.
    pub speech: SpeechConfig,
    /// The machine profile.
    pub machine: MachineConfig,
    /// The `eileen` Claude plugin.
    pub claude_plugin: ClaudePluginConfig,
    /// Logs.
    pub logging: LoggingConfig,
    /// The flight recorder.
    pub flight: FlightConfig,
    /// Incident bundles.
    pub incidents: IncidentsConfig,
    /// Recordings.
    pub recording: RecordingConfig,
    /// The test fleet.
    pub fleet: FleetConfig,
}

/// A setting as the core sees it: the serializable view of `eileen_config::SettingSpec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingSpecView {
    /// The dotted key, for example `assistant.name`.
    pub key: String,
    /// Its documentation.
    pub doc: String,
    /// Whether it can be changed by voice.
    pub voice: bool,
    /// Whether it can be changed on the HUD.
    pub hud: bool,
    /// Its risk.
    pub risk: Risk,
    /// When a change takes effect.
    pub apply: ApplyMode,
}

/// A setting's risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Harmless.
    Low,
    /// A change could misfire; this check runs first.
    Misfire(CheckKind),
    /// A safety key, within this bound (section 2.11).
    Safety(SafetyBound),
}

/// When a setting change takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    /// From the next turn.
    NextTurn,
    /// At once.
    Live,
    /// After a component restarts.
    Restart(Component),
}

/// A component a setting change restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    /// The speech process.
    Speech,
    /// The HUD.
    Hud,
    /// The harness and its sessions.
    Harness,
    /// The desktop sidecar.
    Desktop,
    /// The apps sidecar.
    Apps,
    /// The shell (tray and hotkeys).
    Shell,
}

/// A check a setting change runs before it applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    /// The recognizer can pick the new name out of speech.
    NameRecognisable,
    /// No other app owns the hotkey.
    HotkeyFree,
    /// The voice renders faster than real time.
    VoiceRealtime,
    /// The brain benchmark passes for the new backend.
    BrainBenchmark,
    /// A fitted setting of section 2.16 given a value instead of `auto`: the core's settings
    /// flow computes the plan with the change (`eileen_core::fit`, no IO) and, when it does not
    /// fit the PC's usual memory, says its cost and asks first.
    MachineFit,
    /// No check.
    None,
}

/// How far a safety key may be changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SafetyBound {
    /// Only with an approval from `UserInput`.
    NeedsUserApproval,
    /// Only to tier 1.
    Tier1Only,
    /// To tier 2 only with a logged decision.
    Tier2WithLoggedDecision,
    /// Never by voice or by a model.
    Never,
}

/// One change to a setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingChange {
    /// The dotted key.
    pub key: String,
    /// The new value.
    pub value: Value,
}

/// A proposed set of changes, checked and ready to confirm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Proposal {
    /// Its id.
    pub id: ProposalId,
    /// The changes.
    pub changes: Vec<SettingChange>,
    /// What would change, line by line.
    pub diff: Vec<DiffLine>,
    /// The checks and their results.
    pub checks: Vec<CheckResult>,
    /// The sentence Eileen confirms with.
    pub confirm_sentence: String,
    /// Whether it needs a yes.
    pub needs_yes: bool,
    /// What the user said.
    pub utterance: String,
    /// Where it came from.
    pub source: SettingsSource,
}

/// One line of a settings diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiffLine {
    /// The dotted key.
    pub key: String,
    /// The old value, as TOML; `None` when it was unset.
    pub from: Option<String>,
    /// The new value, as TOML.
    pub to: String,
}

/// One check's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckResult {
    /// The check.
    pub kind: CheckKind,
    /// Whether it passed.
    pub passed: bool,
    /// What it found, in one sentence.
    pub detail: String,
}

/// One version of the settings, for undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsVersion {
    /// The version number.
    pub version: u64,
    /// When it was written.
    pub at: WallTime,
    /// Where the change came from.
    pub source: SettingsSource,
    /// What the user said.
    pub utterance: Option<String>,
    /// The approval that allowed it.
    pub approval: Option<ApprovalId>,
    /// What changed.
    pub diff: Vec<DiffLine>,
    /// The whole file after the change.
    pub snapshot_toml: String,
}

/// Where a settings change came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSource {
    /// By voice.
    Voice,
    /// On the HUD.
    Hud,
    /// An edit of the file.
    File,
    /// A named profile.
    Profile(String),
    /// An undo.
    Undo,
    /// The defaults.
    Default,
    /// The brain's `settings_propose`.
    BrainProposal,
}

/// A dotted setting key.
pub type SettingKey = String;

/// Why a proposed change is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationError {
    /// The key.
    pub key: String,
    /// The line of the file, for a file edit.
    pub line: Option<u32>,
    /// What is wrong, in one sentence.
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} (line {line}): {}", self.key, self.message),
            None => write!(f, "{}: {}", self.key, self.message),
        }
    }
}

impl std::error::Error for ValidationError {}

/// From the settings driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsEvent {
    /// The config file changed on disk.
    FileChanged {
        /// Whether it is still valid.
        valid: bool,
        /// The first bad line.
        error_line: Option<u32>,
    },
    /// A proposal's checks finished.
    ChecksDone {
        /// The proposal.
        proposal: ProposalId,
        /// The results.
        checks: Vec<CheckResult>,
    },
}

/// To the settings driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsCmd {
    /// Run a proposal's checks.
    RunChecks {
        /// The proposal.
        proposal: Box<Proposal>,
    },
    /// Write a new version to the file and the history.
    WriteVersion(SettingsVersion),
    /// Apply these keys to the running components.
    ApplyLive(Vec<SettingKey>),
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Config>();
    sections::register(r);
    r.add::<SettingSpecView>();
    r.add::<Risk>();
    r.add::<ApplyMode>();
    r.add::<Component>();
    r.add::<CheckKind>();
    r.add::<SafetyBound>();
    r.add::<SettingChange>();
    r.add::<Proposal>();
    r.add::<DiffLine>();
    r.add::<CheckResult>();
    r.add::<SettingsVersion>();
    r.add::<SettingsSource>();
    r.add::<ValidationError>();
    r.add::<SettingsEvent>();
    r.add::<SettingsCmd>();
}

#[cfg(test)]
mod tests {
    use super::{AutoOr, BenchMode, Config};
    use crate::machine::Precision;

    #[test]
    fn an_empty_file_gives_the_defaults() {
        let parsed: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, Config::default(), "every section defaults");
        assert_eq!(parsed.assistant.name, "Eileen", "the name");
        assert_eq!(parsed.conversation.window_s, 45, "the conversation window");
        assert_eq!(parsed.routing.models.get("fast").map(String::as_str), Some("haiku"), "alias");
    }

    #[test]
    fn watermarking_is_opt_in() {
        assert!(!Config::default().voice.watermark_exports, "no watermark unless the user asks");
    }

    #[test]
    fn every_fitted_setting_is_auto_until_the_user_sets_it() {
        let defaults = Config::default();
        assert_eq!(defaults.routing.hot_spare, AutoOr::Auto, "row 4, the spare");
        assert_eq!(defaults.memory.pool_deep, AutoOr::Auto, "row 3");
        assert_eq!(defaults.memory.pool_computer_use, AutoOr::Auto, "row 5");
        assert_eq!(defaults.memory.max_workers, AutoOr::Auto, "row 7");
        assert_eq!(defaults.memory.max_build_heavy_jobs, AutoOr::Auto, "row 8");
        assert_eq!(defaults.speech.asr, None, "row 1: the plan chooses");
        assert_eq!(defaults.speech.tts_precision, AutoOr::Auto, "row 2");
        assert_eq!(defaults.speech.threads, AutoOr::Auto, "row 6");
        assert_eq!(defaults.speech.embed, None, "row 9: the plan chooses");
        assert_eq!(defaults.speech.pin_working_set, AutoOr::Auto, "row 10");
        assert_eq!(defaults.incidents.max_mb, AutoOr::Auto, "row 11, incidents");
        assert_eq!(defaults.recording.max_mb, AutoOr::Auto, "row 11, recordings");
        assert_eq!(defaults.machine.bench, BenchMode::OnChange, "benchmarks on a change");
        assert!(defaults.machine.announce, "plan changes are announced");
        let set: Config = serde_json::from_str(
            r#"{"routing": {"hot_spare": true}, "memory": {"max_workers": 2},
                "speech": {"tts_precision": "int8", "threads": "auto"},
                "machine": {"bench": "off"}}"#,
        )
        .unwrap();
        assert_eq!(set.routing.hot_spare, AutoOr::Value(true), "keep a spare brain anyway");
        assert_eq!(set.memory.max_workers, AutoOr::Value(2), "run at most two agents");
        assert_eq!(set.speech.tts_precision, AutoOr::Value(Precision::Int8), "a precision");
        assert_eq!(set.speech.threads, AutoOr::Auto, "auto written out");
        assert_eq!(set.machine.bench, BenchMode::Off, "no automatic benchmarks");
    }

    #[test]
    fn a_partial_section_keeps_its_other_defaults() {
        let parsed: Config = serde_json::from_str(r#"{"conversation": {"window_s": 30}}"#).unwrap();
        assert_eq!(parsed.conversation.window_s, 30, "changed");
        assert_eq!(parsed.conversation.follow_up_s, 8, "kept");
    }
}

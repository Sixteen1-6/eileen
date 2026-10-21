//! Inputs and effects of the core (BUILD.md section 3.3.3).
//!
//! The core is a state machine with one entry point. Drivers turn the outside world into
//! [`Input`] and carry out [`Effect`]; they hold no decisions. The families that belong to one
//! producing area live in that area's module (`speech`, `harness`, `connectors`, `learn`,
//! `config`, `hud`); this module holds the closed enums and the families the core itself owns.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::{ApprovalSource, BusEvent};
use crate::config::{SettingsCmd, SettingsEvent, SettingsVersion};
use crate::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use crate::desktop::PresentationState;
use crate::harness::{BrainCmd, BrainEvent, HookReply, HookRequest, WorkerCmd, WorkerEvent};
use crate::hud::{HudInput, HudState};
use crate::ids::{
    ApprovalId, CallId, ConnectorId, Envelope, JobId, MonoTime, Personal, SpeakId, ToolName,
    TurnId, Untrusted, WallTime, WorkerSessionId,
};
use crate::jobs::{CallOrigin, Job, MemoryPressure, PlanUsage, Tier};
use crate::learn::{LearnCmd, LearnEvent};
use crate::schema::Registry;
use crate::speech::{SpeechCmd, VoiceEvent};

mod facts;

pub use facts::{
    GitFact, PathAccess, PathClass, PathFact, UrlCtx, UrlFact, UrlRefusal, UrlVerdict, VouchRule,
};

/// Everything that reaches the core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// From the speech process.
    Voice(VoiceEvent),
    /// From the audio driver's playback accounting.
    Playback(PlaybackEvent),
    /// From the brain driver.
    Brain(BrainEvent),
    /// From a worker session.
    Worker(WorkerSessionId, WorkerEvent),
    /// A hook request from a worker session.
    Hook(WorkerSessionId, HookRequest),
    /// From a shim (caller verified by PID and Job Object), a replay or the core.
    ToolCall(ToolCallRequest),
    /// From a sidecar or the core-side executor.
    ToolResult(CallId, ToolResult),
    /// From a sidecar's client.
    Sidecar(SidecarId, SidecarEvent),
    /// From the learning store.
    Learn(LearnEvent),
    /// Hotkeys, the HUD, the tray, typed text and `Cli`; never screen or model text.
    User(UserInput),
    /// What the user and the PC are doing.
    Activity(ActivityEvent),
    /// Every 5 s.
    Memory(MemoryPressure),
    /// From the settings driver.
    Settings(SettingsEvent),
    /// From the store.
    Store(StoreEvent),
    /// A timer the core set has fired.
    Timer(TimerId),
}

/// Playback accounting from the audio driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackEvent {
    /// Samples handed to the device, at most every 50 ms.
    Played {
        /// The utterance.
        speak: SpeakId,
        /// Its clause.
        clause: u16,
        /// Samples of that clause played so far.
        samples_played: u32,
    },
    /// Playback paused.
    Paused {
        /// The utterance.
        speak: SpeakId,
        /// Its clause.
        clause: u16,
        /// Samples of that clause played before the pause.
        samples_played: u32,
    },
    /// Playback stopped.
    Stopped {
        /// The utterance.
        speak: SpeakId,
        /// Its clause.
        clause: u16,
        /// Samples of that clause played before the stop.
        samples_played: u32,
    },
    /// Every queued sample of the utterance was played.
    Drained {
        /// The utterance.
        speak: SpeakId,
    },
    /// The output device changed.
    DeviceChanged,
    /// The device stopped taking samples.
    Stalled,
}

/// A tool call on its way to the dispatcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolCallRequest {
    /// Which shim session or internal origin, to route the reply.
    pub caller: Caller,
    /// Who made the call.
    pub origin: CallOrigin,
    /// The tool.
    pub tool: ToolName,
    /// Its arguments, as sent.
    pub args: Untrusted<Value>,
    /// Added by the runtime before the core sees the call.
    pub paths: Vec<PathFact>,
    /// Likewise, for every URL in the arguments (section 2.12, "One URL rule").
    pub urls: Vec<UrlFact>,
    /// The caller's W3C `traceparent`, when it sent one.
    pub traceparent: Option<String>,
}

/// Where a tool call's reply goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Caller {
    /// A shim connection to the dispatcher pipe.
    Shim {
        /// The connection, numbered by the dispatcher.
        connection: u64,
        /// The role its hello named.
        role: ShimRole,
    },
    /// The core itself, a replay or the fast path.
    Internal,
}

/// The role a shim's hello names (section 3.3.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShimRole {
    /// The brain's session.
    Brain,
    /// A worker session.
    Worker(WorkerSessionId),
    /// The librarian's session.
    Librarian,
    /// The owner's own Claude session, through the plugin.
    Session,
    /// A coding worker's developer tools.
    Dev,
}

/// What a tool call returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolResult {
    /// What the model sees.
    pub content: Vec<Content>,
    /// Whether the call failed.
    pub is_error: bool,
    /// How the server checked the effect, when it did.
    pub verification: Option<Verification>,
    /// The op's structured result, matching its `OpSpec.output_schema`; recipes point into it.
    pub output: Option<Untrusted<Value>>,
    /// How long it took.
    pub duration_ms: u32,
    /// The route the call took.
    pub rung: Option<Rung>,
}

/// One block of a tool result. Never structured content beside an image: models see `output`
/// as one JSON text block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// Text.
    Text(Untrusted<String>),
    /// An image.
    Image {
        /// Its media type, for example `image/jpeg`.
        mime: String,
        /// Its bytes, base64.
        base64: String,
    },
}

/// How a server checked a call's effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Verification {
    /// What was checked, for example `value_read_back`.
    pub kind: String,
    /// What it saw.
    pub observed: Untrusted<Value>,
}

/// The route a tool or call takes to an app, from the most direct to the last resort.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    /// The app's own scripting interface.
    AppApi,
    /// The app's own MCP server.
    AppMcp,
    /// A deep link or URI scheme.
    DeepLink,
    /// The app's own actions (menus, commands) through automation.
    AppActions,
    /// UI Automation.
    Uia,
    /// A web page's DOM.
    Dom,
    /// Screenshots and pointer input.
    Vision,
}

/// What the user and the PC are doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityEvent {
    /// From Raw Input; at most one per 250 ms; carries no key or position.
    PhysicalInput,
    /// Loopback energy Eileen's own stream does not explain, or a media session playing.
    OtherAudio {
        /// Whether other audio is playing.
        playing: bool,
    },
    /// Another app is using the microphone.
    MicInUseElsewhere(bool),
    /// The session is locked.
    SessionLocked(bool),
    /// The presentation state changed.
    Presentation(PresentationState),
}

/// Input from the user's own hands and keyboard, never from screen or model text. The shell
/// emits `Approve`, `Deny` and an unmuting `SetMute` or `ToggleMute` from a hotkey or the tray
/// only when Raw Input saw the keys or the click come from a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserInput {
    /// Mute or unmute.
    SetMute(bool),
    /// Toggle mute.
    ToggleMute,
    /// Approve a question.
    Approve(ApprovalId),
    /// Deny a question.
    Deny(ApprovalId),
    /// Cancel a job.
    CancelJob(JobId),
    /// Pause every desktop job.
    PauseDesktopJobs,
    /// Admit one job queued for memory (rule 4 of section 2.5).
    StartAnyway(JobId),
    /// Typing in Eileen's own UI.
    Typed {
        /// The text.
        text: Personal<String>,
    },
    /// `eileen say` and other command-line input.
    Cli {
        /// The text.
        text: Personal<String>,
    },
    /// A HUD line.
    Hud {
        /// What was clicked.
        input: HudInput,
        /// Whether the click came with a Raw Input device handle.
        physical: bool,
    },
    /// The dictation key went down or up.
    DictationKey(KeyEdge),
    /// Save an incident.
    SaveIncident,
    /// Save the last request as a scenario.
    SaveScenario,
}

/// One edge of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyEdge {
    /// Pressed.
    Down,
    /// Released.
    Up,
}

/// From the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoreEvent {
    /// The store loaded at start.
    Loaded {
        /// The jobs it holds.
        jobs: Vec<Job>,
        /// The current settings version.
        settings_version: u64,
    },
    /// A purge finished.
    Purged {
        /// The jobs purged.
        jobs: Vec<JobId>,
        /// Rows removed.
        rows: u64,
    },
}

/// A timer the core set, numbered by the core.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct TimerId(pub u64);

/// Everything the core asks the drivers to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// To the speech process.
    Speech(SpeechCmd),
    /// To the audio driver.
    Audio(AudioCmd),
    /// To the brain driver; every command names a slot.
    Brain(BrainCmd),
    /// To a worker session.
    Worker(WorkerSessionId, WorkerCmd),
    /// The answer to a gating hook request.
    Hook(WorkerSessionId, HookReply),
    /// Forward a call to a connector.
    ToolForward {
        /// The call.
        call: CallId,
        /// Where it goes.
        connector: ConnectorId,
        /// The tool.
        tool: ToolName,
        /// The arguments, rebuilt by the dispatcher.
        args: Value,
        /// How long the connector has.
        deadline_ms: u32,
    },
    /// Reply to a caller.
    ToolReply {
        /// Who asked.
        caller: Caller,
        /// The call.
        call: CallId,
        /// The result.
        result: ToolResult,
    },
    /// To a sidecar's client.
    Sidecar(SidecarId, SidecarCmd),
    /// A full HUD snapshot; the driver rate-limits to 10 per second.
    Hud(Box<HudState>),
    /// To the learning store.
    Learn(LearnCmd),
    /// To the settings driver.
    Settings(SettingsCmd),
    /// To the store.
    Persist(Record),
    /// Publish on the bus.
    Emit(BusEvent),
    /// Set a timer.
    SetTimer {
        /// The timer.
        id: TimerId,
        /// When it fires.
        at: MonoTime,
    },
    /// Cancel a timer.
    CancelTimer(TimerId),
    /// Capture an incident.
    Incident(IncidentTrigger),
    /// Show a toast.
    Notify(Toast),
}

impl Effect {
    /// The family the runtime routes this effect by.
    #[must_use]
    pub const fn family(&self) -> EffectFamily {
        match self {
            Self::Speech(_) => EffectFamily::Speech,
            Self::Audio(_) => EffectFamily::Audio,
            Self::Brain(_) => EffectFamily::Brain,
            Self::Worker(..) => EffectFamily::Worker,
            Self::Hook(..) => EffectFamily::Hook,
            Self::ToolForward { .. } => EffectFamily::ToolForward,
            Self::ToolReply { .. } => EffectFamily::ToolReply,
            Self::Sidecar(..) => EffectFamily::Sidecar,
            Self::Hud(_) => EffectFamily::Hud,
            Self::Learn(_) => EffectFamily::Learn,
            Self::Settings(_) => EffectFamily::Settings,
            Self::Persist(_) => EffectFamily::Persist,
            Self::Emit(_) => EffectFamily::Emit,
            Self::SetTimer { .. } | Self::CancelTimer(_) => EffectFamily::Timer,
            Self::Incident(_) => EffectFamily::Incident,
            Self::Notify(_) => EffectFamily::Notify,
        }
    }
}

/// The families of [`Effect`] a component can accept (`Component::accepts`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EffectFamily {
    /// `Effect::Speech`.
    Speech,
    /// `Effect::Audio`.
    Audio,
    /// `Effect::Brain`.
    Brain,
    /// `Effect::Worker`.
    Worker,
    /// `Effect::Hook`.
    Hook,
    /// `Effect::ToolForward`.
    ToolForward,
    /// `Effect::ToolReply`.
    ToolReply,
    /// `Effect::Sidecar`.
    Sidecar,
    /// `Effect::Hud`.
    Hud,
    /// `Effect::Learn`.
    Learn,
    /// `Effect::Settings`.
    Settings,
    /// `Effect::Persist`.
    Persist,
    /// `Effect::Emit`.
    Emit,
    /// `Effect::SetTimer` and `Effect::CancelTimer`.
    Timer,
    /// `Effect::Incident`.
    Incident,
    /// `Effect::Notify`.
    Notify,
}

/// To the audio driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioCmd {
    /// Start or stop sending microphone audio to the speech process.
    SetCapture(bool),
    /// Duck other audio by this many decibels (negative).
    Duck {
        /// The gain change, dB.
        db: i8,
    },
    /// Undo the duck.
    Unduck,
    /// Pause an utterance.
    Pause {
        /// The utterance.
        speak: SpeakId,
    },
    /// Resume an utterance.
    Resume {
        /// The utterance.
        speak: SpeakId,
    },
    /// Stop an utterance.
    Stop {
        /// The utterance.
        speak: SpeakId,
    },
    /// Play a cached phrase.
    PlayPhrase {
        /// The phrase cache key.
        key: String,
        /// The voice it was rendered in.
        voice: String,
    },
    /// Play an earcon.
    Earcon(Earcon),
    /// Record from the microphone.
    Record {
        /// How long.
        seconds: u16,
        /// What for.
        purpose: RecordPurpose,
    },
}

/// A short non-speech sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Earcon {
    /// The request was routed to the brain or a job.
    Routed,
    /// Something finished.
    Done,
    /// Something needs the user.
    Attention,
    /// Something failed.
    Error,
    /// The microphone was muted.
    Muted,
    /// The microphone was unmuted.
    Unmuted,
}

/// Why the microphone is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordPurpose {
    /// The first run's clip, for the listening calibration and the optional clone.
    FirstRun,
    /// Voice enrollment for the speaker check.
    Enrollment,
    /// A voice to clone, with its consent sentence.
    Clone,
}

/// A row for the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Record {
    /// Insert or update a job.
    JobUpsert(Box<Job>),
    /// Append a bus event to the `events` table.
    EventAppend(Envelope<BusEvent>),
    /// An approval question and its answer.
    ApprovalRow(ApprovalRow),
    /// A journaled reversible call.
    JournalRow(JournalRow),
    /// A settings version.
    SettingsVersion(SettingsVersion),
    /// The latest plan usage.
    PlanUsage(PlanUsage),
    /// A forget: the jobs' rows and every event of those turns.
    PurgeJobs {
        /// The jobs.
        jobs: Vec<JobId>,
        /// The turns.
        turns: Vec<TurnId>,
    },
}

/// One approval question and how it was resolved, in the `approvals` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovalRow {
    /// The question.
    pub approval: ApprovalId,
    /// The job that asked, if any.
    pub job: Option<JobId>,
    /// The tier of the call.
    pub tier: Tier,
    /// What the user was shown.
    pub summary: String,
    /// The answer; `None` while open.
    pub allowed: Option<bool>,
    /// Which form of the user's input resolved it.
    pub by: Option<ApprovalSource>,
    /// When it was asked.
    pub asked: WallTime,
    /// When it was resolved.
    pub resolved: Option<WallTime>,
}

/// One reversible call in the journal, so "undo that" can reverse it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JournalRow {
    /// The job it ran in.
    pub job: JobId,
    /// The call.
    pub call: CallId,
    /// The tool.
    pub tool: ToolName,
    /// The op's tag, for tools with ops.
    pub op: Option<String>,
    /// How to reverse it, from the op's `inverse`.
    pub inverse: Option<String>,
    /// The call's output, which carries the inputs the inverse needs.
    pub output: Option<Untrusted<Value>>,
    /// When it ran.
    pub at: WallTime,
}

/// Why an incident is captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentTrigger {
    /// The user said "something went wrong" or pressed the report hotkey.
    UserReport,
    /// A `never!` or `always!` soft assertion failed.
    SoftAssertion {
        /// The assertion's site name.
        site: String,
    },
    /// A component stopped answering.
    Hang {
        /// The component.
        component: String,
    },
    /// A component crashed.
    Crash {
        /// The component.
        component: String,
    },
    /// A job failed.
    JobFailed {
        /// The job.
        job: JobId,
    },
}

/// A Windows toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Toast {
    /// The title line.
    pub title: String,
    /// The body.
    pub body: String,
    /// The job it is about, if any.
    pub job: Option<JobId>,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Input>();
    facts::register(r);
    r.add::<PlaybackEvent>();
    r.add::<ToolCallRequest>();
    r.add::<Caller>();
    r.add::<ShimRole>();
    r.add::<ToolResult>();
    r.add::<Content>();
    r.add::<Verification>();
    r.add::<Rung>();
    r.add::<ActivityEvent>();
    r.add::<UserInput>();
    r.add::<KeyEdge>();
    r.add::<StoreEvent>();
    r.add::<TimerId>();
    r.add::<Effect>();
    r.add::<EffectFamily>();
    r.add::<AudioCmd>();
    r.add::<Earcon>();
    r.add::<RecordPurpose>();
    r.add::<Record>();
    r.add::<ApprovalRow>();
    r.add::<JournalRow>();
    r.add::<IncidentTrigger>();
    r.add::<Toast>();
    r.add::<Envelope<Input>>();
    r.add::<Envelope<Effect>>();
    r.add::<Envelope<BusEvent>>();
}

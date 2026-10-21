//! Bus events (BUILD.md section 3.3.4).
//!
//! Published to the HUD, the loopback endpoint, the Claude plugin and the `events` table, each
//! in an `Envelope`. Speech that was not addressed to Eileen is not a bus event, and neither is
//! any partial transcript: `Heard` is emitted once per turn, with the final transcript, after the
//! turn has been routed somewhere other than "ignored". Event names are in the past tense; the
//! dotted names (`turn.ended`, `job.completed`) are generated from these variants.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::SettingChange;
use crate::connectors::SidecarId;
use crate::desktop::Rect;
use crate::ids::{ApprovalId, IncidentId, JobId, Personal, SpeakId, Untrusted, WallTime};
use crate::jobs::{JobKind, PlanUsage, Route, Tier, TurnAuthority, TurnSource};
use crate::learn::LearnBusEvent;
use crate::schema::Registry;

/// What happened, for everyone listening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BusEvent {
    /// A turn began.
    TurnStarted {
        /// Where it came from.
        source: TurnSource,
        /// Whether it may use the full brain tools.
        authority: TurnAuthority,
    },
    /// The final transcript, only once the turn is not ignored.
    Heard {
        /// The transcript.
        text: Personal<String>,
    },
    /// Where the turn went.
    TurnRouted {
        /// The route.
        route: TurnRoute,
        /// Why, in a few words.
        reason: String,
    },
    /// Eileen started speaking.
    Speaking {
        /// The utterance.
        speak: SpeakId,
        /// Why she speaks.
        cause: SpeakCause,
        /// What she says.
        text: String,
    },
    /// Eileen stopped speaking.
    SpeakingStopped {
        /// The utterance.
        speak: SpeakId,
        /// Characters the user heard.
        heard_chars: u32,
        /// Whether the user interrupted.
        interrupted: bool,
        /// Whether she resumed after a backchannel.
        resumed: bool,
    },
    /// The microphone was muted or unmuted.
    MuteChanged {
        /// Whether it is muted now.
        muted: bool,
    },
    /// The conversation window opened or closed.
    ConversationWindow {
        /// Whether it is open.
        open: bool,
        /// How long it stays open, ms.
        until_ms: u32,
    },
    /// The brain is ready.
    BrainReady {
        /// The resolved model id.
        model: String,
    },
    /// A job was accepted.
    JobAccepted {
        /// The job.
        job: JobId,
        /// Its spoken name.
        name: String,
        /// Its ordinal among live jobs.
        ordinal: Option<u8>,
        /// Its kind.
        kind: JobKind,
        /// Its route.
        route: Route,
        /// The resolved model id.
        model: String,
        /// Why this route and model.
        reason: String,
    },
    /// A job is waiting.
    JobQueued {
        /// The job.
        job: JobId,
        /// What it waits for.
        reason: QueueReason,
    },
    /// A job started.
    JobStarted {
        /// The job.
        job: JobId,
        /// Whether it took a warm pooled session.
        warm: bool,
    },
    /// A job's narration line.
    JobNarration {
        /// The job.
        job: JobId,
        /// The line.
        line: Untrusted<String>,
    },
    /// A job made progress.
    JobProgressed {
        /// The job.
        job: JobId,
        /// The progress line.
        line: Untrusted<String>,
        /// Progress in percent, when known.
        pct: Option<u8>,
    },
    /// A job is blocked.
    JobBlocked {
        /// The job.
        job: JobId,
        /// On what.
        reason: String,
    },
    /// A question for the user.
    ApprovalRequested {
        /// The question.
        approval: ApprovalId,
        /// The job that asks, if any.
        job: Option<JobId>,
        /// The tier of the call.
        tier: Tier,
        /// What the user is shown.
        summary: String,
        /// Whether and how it can be answered by voice.
        voice: VoiceApproval,
        /// When it is declined by default.
        deadline: WallTime,
    },
    /// A question was answered.
    ApprovalResolved {
        /// The question.
        approval: ApprovalId,
        /// The answer.
        allowed: bool,
        /// Which form of the user's input answered it.
        by: ApprovalSource,
    },
    /// A question expired.
    ApprovalExpired {
        /// The question.
        approval: ApprovalId,
    },
    /// A job finished.
    JobCompleted {
        /// The job.
        job: JobId,
        /// Its summary.
        summary: Untrusted<String>,
        /// Files it made.
        artifacts: Vec<Personal<String>>,
    },
    /// A job failed.
    JobFailed {
        /// The job.
        job: JobId,
        /// Why.
        error: String,
    },
    /// A job was cancelled.
    JobCancelled {
        /// The job.
        job: JobId,
        /// By whom.
        by: CancelSource,
    },
    /// A job was interrupted by a core restart.
    JobInterrupted {
        /// The job.
        job: JobId,
    },
    /// Where the next click will land, shown before it happens.
    DesktopIntent {
        /// The target, in physical virtual-screen pixels.
        rect: Rect,
        /// What it is.
        label: String,
        /// How long it is shown, ms.
        until_ms: u32,
    },
    /// Who holds the mouse and keyboard.
    InputLock {
        /// The job, or `None` when released.
        holder: Option<JobId>,
    },
    /// A job holds the foreground.
    ForegroundLease {
        /// The job.
        job: JobId,
        /// Seconds left.
        seconds_left: u16,
    },
    /// The user's own hands moved during a lease.
    UserTookOver {
        /// The job that stopped.
        job: JobId,
    },
    /// Desktop jobs were paused or continued.
    DesktopJobsPaused {
        /// Whether they are paused now.
        paused: bool,
    },
    /// The plan usage changed.
    PlanUsage(PlanUsage),
    /// From the learning loop.
    Learn(LearnBusEvent),
    /// One plain sentence and an optional fix (section 2.13).
    UserError(UserError),
    /// A setting changed.
    SettingChanged {
        /// The key.
        key: String,
        /// The old value, as TOML.
        from: String,
        /// The new value, as TOML.
        to: String,
        /// The new settings version.
        version: u64,
    },
    /// The first run moved on.
    FirstRun(FirstRunPhase),
    /// Recording started or stopped.
    RecordingChanged {
        /// Whether it records now.
        on: bool,
    },
    /// An incident was captured.
    IncidentCaptured {
        /// The incident.
        id: IncidentId,
    },
    /// A sidecar was restarted.
    SidecarRestarted {
        /// Which.
        which: SidecarId,
        /// Why.
        reason: String,
    },
    /// A slow subscriber missed events.
    Lagged {
        /// How many.
        missed: u64,
    },
}

/// Where a turn went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnRoute {
    /// The deterministic fast path.
    FastPath,
    /// The brain's `act` tools.
    Act,
    /// A learned recipe.
    Replay,
    /// The brain.
    Brain,
    /// A clarifying question.
    Clarify,
    /// Not addressed to Eileen.
    Ignored,
    /// Dictation into a field.
    Dictation,
}

/// What a job waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    /// Memory admission.
    Memory,
    /// A build slot.
    BuildSlot,
    /// The plan's limits.
    Plan,
}

/// Why Eileen speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpeakCause {
    /// An answer to the user's turn.
    Reply,
    /// A job announcement.
    Announcement,
    /// A job's narration, read aloud.
    Narration,
    /// An approval question.
    Approval,
    /// A clarifying question.
    Clarification,
    /// A user error's sentence.
    Error,
    /// The first run.
    FirstRun,
    /// A line from the owner's own Claude session.
    SessionSay,
}

/// Whether an approval can be answered by voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceApproval {
    /// By saying the action word in the voice window.
    ActionWord {
        /// The word.
        word: String,
        /// Whether the enrolled speaker must match.
        speaker_match_required: bool,
    },
    /// Only on the HUD.
    HudOnly {
        /// Why not by voice.
        reason: HudOnlyReason,
    },
}

/// Why an approval is HUD only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HudOnlyReason {
    /// Other audio is playing.
    OtherAudio,
    /// Another app uses the microphone.
    MicInUse,
    /// The call could not be parsed into a summary.
    Unparsed,
    /// The call runs code.
    RunsCode,
    /// A fleet run, where tier 2 is auto-denied.
    Fleet,
}

/// Which form of the user's input resolved an approval; recorded in the approvals table and on
/// the bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSource {
    /// Physical, from the strip's Raw Input check.
    HudClick,
    /// Physical, from the shell's Raw Input check.
    Hotkey,
    /// The action word in the voice window.
    Voice {
        /// The word heard.
        action_word: String,
        /// `None` when the speaker check is not enrolled.
        speaker_match: Option<bool>,
    },
    /// Declined when `approvals.deadline_min` passed.
    Deadline,
    /// The job was cancelled or failed while the question was open.
    JobEnded,
}

/// Who cancelled a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelSource {
    /// The user, by voice.
    Voice,
    /// The user, on the HUD.
    Hud,
    /// The user, with a hotkey.
    Hotkey,
    /// The user, from the tray.
    Tray,
    /// The user, from the command line.
    Cli,
    /// Its fan-out parent was cancelled.
    Parent,
    /// Eileen is shutting down.
    Shutdown,
}

/// An error a user can meet: a stable kind, one plain sentence built from a template in Eileen's
/// own code, and a fix when one exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UserError {
    /// The stable kind.
    pub kind: UserErrorKind,
    /// The sentence.
    pub sentence: String,
    /// The fix, when one exists.
    pub fix: Option<FixAction>,
}

/// The stable kinds of user error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserErrorKind {
    /// A job kind v0.1 does not run (`ml_project`).
    JobKindUnavailable,
    /// A route v0.1 does not build.
    RouteUnavailable,
    /// The vendor CLI is not signed in.
    SignedOut,
    /// The plan's limit is reached.
    PlanLimit,
    /// Not enough memory to start the job.
    MemoryLow,
    /// No app matches the name.
    AppNotFound,
    /// An app's scripting interface is off or unreachable.
    AppUnreachable,
    /// The microphone is unavailable.
    MicUnavailable,
    /// A speech model is missing or failed its check.
    SpeechModelMissing,
    /// A tool call failed.
    ToolFailed,
    /// A setting change was refused.
    SettingRefused,
    /// The network is unreachable.
    NetworkUnavailable,
    /// Something inside Eileen failed; an incident was captured.
    Internal,
}

/// A fix the user can run from an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FixAction {
    /// Open a URI (a settings page, a help page).
    OpenUri {
        /// The URI.
        uri: String,
    },
    /// Run a doctor check.
    Doctor {
        /// The area, for example `claude` or `resolve`.
        area: String,
    },
    /// Change a setting, through the normal settings flow.
    Setting {
        /// The change.
        change: SettingChange,
    },
    /// Retry a job.
    Retry {
        /// The job.
        job: JobId,
    },
    /// Explain an incident.
    Explain {
        /// The incident.
        incident: IncidentId,
    },
}

/// Where the first run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirstRunPhase {
    /// The welcome and the three choices.
    Welcome,
    /// The guided tour.
    Tour,
    /// The surprise path: recording the user's clip.
    Recording,
    /// The surprise path: rendering the clone.
    Rendering,
    /// The surprise path: the reveal.
    Reveal,
    /// Skipped.
    Skipped,
    /// Finished.
    Done,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<BusEvent>();
    r.add::<TurnRoute>();
    r.add::<QueueReason>();
    r.add::<SpeakCause>();
    r.add::<VoiceApproval>();
    r.add::<HudOnlyReason>();
    r.add::<ApprovalSource>();
    r.add::<CancelSource>();
    r.add::<UserError>();
    r.add::<UserErrorKind>();
    r.add::<FixAction>();
    r.add::<FirstRunPhase>();
}

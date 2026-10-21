//! HUD state (BUILD.md section 3.3.6) and the HUD's line protocol.
//!
//! Provisional: WP8 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! Wire: core to HUD `{"v":1,"state":{...}}` per line ([`HudStateLine`]); HUD to core
//! `{"v":1,"input":{...},"physical":true}` per line ([`HudInputLine`]); at most
//! [`MAX_LINE_BYTES`] per line. `physical` is false when the click arrived with no Raw Input
//! device handle; the core ignores non-physical approvals, mute changes, draft and line
//! approvals, "start it anyway" and first-run consent.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::VoiceApproval;
use crate::config::DiffLine;
use crate::desktop::Rect;
use crate::ids::{
    ApprovalId, JobId, Personal, ProposalId, RouteProposalId, SkillId, TurnId, Untrusted, WallTime,
};
use crate::jobs::{JobStatus, Tier};
use crate::record::BlobRef;
use crate::schema::Registry;

/// The protocol version both line kinds carry.
pub const PROTOCOL_VERSION: u16 = 1;

/// The longest line either side may send, 64 KiB.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// One core-to-HUD line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HudStateLine {
    /// The protocol version, [`PROTOCOL_VERSION`].
    pub v: u16,
    /// The full state.
    pub state: HudState,
}

/// One HUD-to-core line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HudInputLine {
    /// The protocol version, [`PROTOCOL_VERSION`].
    pub v: u16,
    /// What was clicked or typed.
    pub input: HudInput,
    /// Whether the click came with a Raw Input device handle.
    pub physical: bool,
}

/// Everything the HUD shows, as one snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HudState {
    /// The state's own version, bumped when its shape changes.
    pub v: u16,
    /// Where and how the HUD is drawn.
    pub layout: HudLayout,
    /// The microphone.
    pub mic: MicState,
    /// Seconds left in the conversation window.
    pub conversation: Option<u16>,
    /// What was heard.
    pub heard: Option<HeardLine>,
    /// What Eileen says.
    pub speaking: Option<String>,
    /// The live jobs, at most 8.
    pub jobs: Vec<HudJob>,
    /// The open approval questions.
    pub approvals: Vec<HudApproval>,
    /// Where the next click will land.
    pub intent: Option<HudIntent>,
    /// The name of the job holding the mouse and keyboard.
    pub input_lock: Option<String>,
    /// A foreground lease.
    pub lease: Option<HudLease>,
    /// The live thumbnail of a window a job works in.
    pub watch: Option<HudWatch>,
    /// The open panel.
    pub panel: Option<HudPanel>,
    /// Suggestions, at most 3.
    pub suggestions: Vec<String>,
    /// The first run's view, while it runs.
    pub first_run: Option<FirstRunView>,
    /// The small indicators.
    pub indicators: Indicators,
}

/// Where and how the HUD is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudLayout {
    /// The screen corner.
    pub corner: Corner,
    /// Scale, percent.
    pub scale_pct: u16,
    /// Opacity, percent.
    pub opacity_pct: u8,
    /// Whether the transcript line shows.
    pub show_transcript: bool,
    /// High contrast.
    pub high_contrast: bool,
}

/// A screen corner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    /// Top left.
    TopLeft,
    /// Top right.
    TopRight,
    /// Bottom left.
    BottomLeft,
    /// Bottom right.
    #[default]
    BottomRight,
}

/// The microphone, as the HUD shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MicState {
    /// The speech models are loading.
    LoadingSpeech,
    /// Listening, nothing heard.
    Listening,
    /// Hearing speech.
    Hearing {
        /// The input level, percent.
        level_pct: u8,
    },
    /// Muted.
    Muted,
    /// Dictating into a field.
    Dictating {
        /// The field's app.
        target: String,
    },
    /// The speech process is restarting.
    SpeechRestarting,
}

/// What was heard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HeardLine {
    /// The transcript.
    pub text: Personal<String>,
    /// Whether it is final.
    pub is_final: bool,
    /// Whether it was addressed to Eileen, once decided.
    pub addressed: Option<bool>,
    /// Its turn, for "that was for you".
    pub turn: TurnId,
}

/// A job on the HUD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudJob {
    /// The job.
    pub id: JobId,
    /// Its spoken name.
    pub name: String,
    /// Its ordinal.
    pub ordinal: Option<u8>,
    /// Its route's display name.
    pub route: String,
    /// Where it is.
    pub status: JobStatus,
    /// Its last line.
    pub line: Option<Untrusted<String>>,
    /// Progress, percent.
    pub pct: Option<u8>,
    /// Whether the user is focused on it.
    pub focus: bool,
    /// Whether it is paused.
    pub paused: bool,
}

/// An approval question on the HUD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudApproval {
    /// The question.
    pub id: ApprovalId,
    /// What the user is shown.
    pub summary: String,
    /// The tier of the call.
    pub tier: Tier,
    /// Seconds left to answer by voice.
    pub seconds_left_voice: Option<u16>,
    /// When it is declined by default.
    pub deadline: WallTime,
    /// Whether and how it can be answered by voice.
    pub voice: VoiceApproval,
}

/// Where the next click will land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudIntent {
    /// The target.
    pub rect: Rect,
    /// What it is.
    pub label: String,
}

/// A foreground lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HudLease {
    /// A job holds the foreground.
    Active {
        /// The job.
        job: JobId,
        /// Seconds left.
        seconds_left: u16,
    },
    /// The user took over.
    TookOver {
        /// The job that stopped.
        job: JobId,
    },
}

/// The live thumbnail of a window a job works in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HudWatch {
    /// The window.
    pub hwnd: i64,
    /// What to call it.
    pub label: String,
}

/// The HUD's open panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HudPanel {
    /// The brain's `---hud---` part.
    Markdown(Untrusted<String>),
    /// What Eileen has learned.
    Learned {
        /// One line per skill.
        lines: Vec<String>,
    },
    /// What Eileen knows about the user.
    Facts {
        /// One line per fact.
        lines: Vec<Personal<String>>,
    },
    /// The files a scenario share would send, with redactions highlighted.
    ScenarioPreview {
        /// The files.
        files: Vec<String>,
        /// How many values were redacted.
        redactions: u32,
    },
    /// A question with options and an optional image.
    Confirm {
        /// The image.
        image: Option<BlobRef>,
        /// The question.
        question: String,
        /// The options.
        options: Vec<String>,
        /// The question's id, for the answer.
        id: u64,
    },
    /// Images to pick from.
    ContactSheet {
        /// The images.
        items: Vec<ContactItem>,
        /// The sheet's id, for the answer.
        id: u64,
    },
    /// A job's changes.
    Diff {
        /// The job.
        job: JobId,
        /// The diff stat.
        stat: String,
        /// The test summary.
        tests: String,
    },
    /// A settings proposal.
    Settings(SettingsPanel),
    /// A drafted skill held for review.
    DraftReview {
        /// The skill.
        skill: SkillId,
        /// Its text.
        text: Untrusted<String>,
        /// The lines held for approval.
        held_lines: Vec<u32>,
    },
    /// A route proposal, shown only from registry fields.
    RouteProposal {
        /// The proposal.
        proposal: RouteProposalId,
        /// The app.
        app: String,
        /// The server's name.
        server: Untrusted<String>,
        /// The verified namespace's display.
        publisher: Option<String>,
        /// The namespace is a domain listed for this app.
        from_app_vendor: bool,
        /// Matched to a registry entry.
        verified: bool,
        /// The package.
        package: Option<Untrusted<String>>,
        /// The source repository.
        source_url: Untrusted<String>,
        /// The worker's note, fenced.
        note: Untrusted<String>,
    },
    /// Voices to pick from.
    VoicePicker {
        /// The voices.
        voices: Vec<String>,
        /// A sample of each.
        samples: Vec<BlobRef>,
    },
    /// A user error.
    Error {
        /// The sentence.
        sentence: String,
        /// The fix's button label.
        fix_label: Option<String>,
    },
    /// The version and the credits.
    About {
        /// The version.
        version: String,
        /// The release channel.
        channel: String,
        /// Each component and model with its licence and attribution line.
        credits: Vec<Credit>,
    },
    /// A question from the owner's own Claude session.
    SessionQuestion {
        /// The question's id.
        id: u64,
        /// The session's repository.
        repo: Option<Personal<String>>,
        /// The question.
        question: Untrusted<String>,
    },
}

/// One image on a contact sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContactItem {
    /// The image.
    pub image: BlobRef,
    /// Its label.
    pub label: String,
}

/// A settings proposal on the HUD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsPanel {
    /// The proposal.
    pub proposal: ProposalId,
    /// What would change.
    pub diff: Vec<DiffLine>,
    /// The sentence Eileen confirms with.
    pub confirm_sentence: String,
    /// Whether it needs a yes.
    pub needs_yes: bool,
}

/// One credits line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Credit {
    /// The component or model.
    pub component: String,
    /// Its licence.
    pub licence: String,
    /// Its attribution line.
    pub attribution: String,
}

/// The first run's view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FirstRunView {
    /// The choices on offer.
    pub choices: Vec<FirstRunChoice>,
    /// Whether the own-voice consent box is ticked.
    pub consent_own_voice: bool,
    /// Progress, percent.
    pub progress_pct: Option<u8>,
    /// Whether the reveal is showing.
    pub reveal: bool,
}

/// The small indicators.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[expect(clippy::struct_excessive_bools, reason = "one flag per indicator")]
pub struct Indicators {
    /// The learning loop is working.
    pub learning: bool,
    /// Recording is on.
    pub recording: bool,
    /// Off the record.
    pub off_the_record: bool,
    /// Plan usage, percent.
    pub plan_usage_pct: Option<u8>,
    /// The brain is warming up.
    pub brain_warming: bool,
    /// A voice is rendering.
    pub rendering_voice: bool,
    /// Memory is low.
    pub low_memory: bool,
    /// Desktop jobs are paused.
    pub desktop_jobs_paused: bool,
}

/// What a HUD click or text box sends. Every variant a click can send has a grammar entry or a
/// hotkey; spoken first-run choices and consent are recognised by the core from the microphone,
/// never accepted from a HUD line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HudInput {
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
    /// Continue desktop jobs.
    ContinueDesktopJobs,
    /// Save the last request as a scenario.
    SaveScenario,
    /// Close the panel.
    DismissPanel,
    /// Approve one held line of a draft.
    ApproveSkillLine {
        /// The skill.
        skill: SkillId,
        /// The line.
        line: u32,
    },
    /// Approve a whole draft.
    ApproveDraft {
        /// The skill.
        skill: SkillId,
    },
    /// Answer a confirm panel, a contact sheet, a clarifying question or a route proposal.
    Answer {
        /// The question's id.
        id: u64,
        /// The chosen option.
        option: u8,
    },
    /// "That was for you", by tap.
    RunHeardLine {
        /// The turn.
        turn: TurnId,
    },
    /// Change a setting, through the normal settings flow.
    SetSetting {
        /// The key.
        key: String,
        /// The value.
        value: Value,
    },
    /// A first-run choice.
    FirstRun {
        /// The choice.
        choice: FirstRunChoice,
        /// Whether the own-voice consent box is ticked.
        consent_own_voice: bool,
    },
    /// Admit a queued job.
    StartAnyway(JobId),
    /// Run an error's fix.
    Fix {
        /// The error's id.
        error: u64,
    },
    /// Return the foreground.
    Back,
    /// An audio clip dropped on the strip: starts voice enrollment.
    DropFile {
        /// The file.
        path: Personal<String>,
    },
}

/// The first run's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FirstRunChoice {
    /// The guided tour.
    Tour,
    /// Skip it.
    Skip,
    /// The surprise path.
    Surprise,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<HudStateLine>();
    r.add::<HudInputLine>();
    r.add::<HudState>();
    r.add::<HudLayout>();
    r.add::<Corner>();
    r.add::<MicState>();
    r.add::<HeardLine>();
    r.add::<HudJob>();
    r.add::<HudApproval>();
    r.add::<HudIntent>();
    r.add::<HudLease>();
    r.add::<HudWatch>();
    r.add::<HudPanel>();
    r.add::<ContactItem>();
    r.add::<SettingsPanel>();
    r.add::<Credit>();
    r.add::<FirstRunView>();
    r.add::<Indicators>();
    r.add::<HudInput>();
    r.add::<FirstRunChoice>();
}

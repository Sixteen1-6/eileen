//! The intent resolver's inputs and answers (BUILD.md section 3.3.10). `eileen_intent::Resolver`
//! implements the `IntentResolver` port over these types.
//!
//! Provisional: WP7 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core_io::Rung;
use crate::ids::{ApprovalId, Personal, ToolName, WallTime};
use crate::jobs::Depth;
use crate::learn::{FactKind, ForgetTarget, Level, SkillRef};
use crate::schema::Registry;
use crate::tools::brain::JobRef;

/// The slots of a match, by slot name.
pub type SlotValues = BTreeMap<String, SlotValue>;

/// What the resolver knows about the moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolveCtx {
    /// The focused app.
    pub focused_app: Option<String>,
    /// Whether media is playing.
    pub media_active: bool,
    /// Whether dictation is on.
    pub dictation: bool,
    /// What the core is waiting for from the user.
    pub awaiting: Option<Awaiting>,
    /// The assistant's name.
    pub assistant_name: String,
    /// How many monitors there are.
    pub monitors: u8,
    /// Recent media, newest first.
    pub media_history: Vec<MediaRef>,
    /// What Eileen's own last turn mentioned.
    pub last_turn: Option<LastTurnRefs>,
}

/// What the core is waiting for from the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Awaiting {
    /// A yes or no to an approval question.
    Approval(ApprovalId),
    /// An answer to a clarifying question.
    Answer {
        /// The question's id.
        id: u64,
        /// The options offered.
        options: Vec<String>,
    },
    /// A first-run choice.
    FirstRun,
}

/// One item of the media history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MediaRef {
    /// Its title.
    pub title: String,
    /// Its URL, for web media.
    pub url: Option<String>,
    /// The app that played it.
    pub app: String,
    /// When it last played.
    pub at: WallTime,
}

/// What Eileen's own last turn mentioned, filled by the core from that turn (never from screen
/// or model text it did not speak or show): the addressee gate's follow-up feature and the fact
/// path's feedback scope read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LastTurnRefs {
    /// How long ago it ended, ms.
    pub age_ms: u32,
    /// HUD items of that turn (answer items, contact-sheet entries), in order.
    pub items: Vec<String>,
    /// The job it announced or named.
    pub job: Option<JobRef>,
    /// Apps it mentioned, as slot values.
    pub apps: Vec<String>,
    /// Files it mentioned, as slot values.
    pub files: Vec<String>,
    /// Media it mentioned, as slot values.
    pub media: Vec<String>,
    /// Its spoken content words, normalised (stop words dropped, stems).
    pub content_words: Vec<String>,
    /// The deep answer's topic, when it was one.
    pub topic: Option<String>,
}

/// Whether an unnamed utterance resolves against Eileen's last turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FollowUp {
    /// It does not.
    None,
    /// A pronoun or ellipsis the slot resolvers bound to one of the last turn's items, job,
    /// apps, files or media.
    Referent {
        /// The slot.
        slot: String,
        /// Its value.
        value: Value,
    },
    /// A content word or the topic shared with the last turn.
    Shared {
        /// The word.
        word: String,
    },
}

/// The resolver's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// L1 safety and control, built-in commands and the fact phrases.
    Command {
        /// The command.
        command: CoreCommand,
        /// Its slots.
        slots: SlotValues,
    },
    /// A plain tool call.
    Tool {
        /// The tool.
        tool: ToolName,
        /// Its arguments.
        args: Value,
        /// Where the match came from.
        source: ToolSource,
    },
    /// An L1 template of a learned skill: level 4 replays now; level 3 goes to the brain as the
    /// `[match]` block; levels 1 and 2 come back as hints.
    Skill {
        /// The skill.
        skill: SkillRef,
        /// Its slots.
        slots: SlotValues,
        /// Its level.
        level: Level,
    },
    /// Ambiguous, including an ambiguous slot ("15 seconds or 15 minutes?").
    Ambiguous {
        /// The clarifying question.
        question: String,
        /// The options.
        options: Vec<String>,
    },
    /// L2: skills for the brain to consider, with their scores.
    Hints(Vec<(SkillRef, u16)>),
    /// L3, with any depth override.
    Brain {
        /// The depth the user asked for.
        depth: Option<Depth>,
        /// Whether the addressee gate was unsure.
        addressee_unsure: bool,
    },
}

/// Where a tool match came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    /// A grammar entry.
    Grammar,
    /// A phrasing the user confirmed.
    ConfirmedPhrasing,
    /// An exact repeat of an earlier request (slot-free only).
    ExactRepeat,
}

/// One slot's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlotValue {
    /// One value.
    Resolved(Value),
    /// Several candidates; replay never runs on an ambiguous slot.
    Ambiguous {
        /// The candidates.
        candidates: Vec<Value>,
    },
}

/// What a grammar entry of section 2.4 asks the core to do itself (entries that are plain tool
/// calls resolve to `Resolution::Tool` instead). One variant per entry family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoreCommand {
    /// Mute (safety and control, precedence 1).
    Mute,
    /// Unmute.
    Unmute,
    /// Stop speaking.
    Stop,
    /// Cancel a job, or the current one.
    Cancel {
        /// The job.
        job: Option<JobRef>,
    },
    /// Go on after an interruption.
    GoOn,
    /// "That was for you."
    ThatWasForYou,
    /// Undo the last action.
    Undo,
    /// Undo the last fact.
    UndoFact,
    /// Forget something.
    Forget(ForgetTarget),
    /// "Something went wrong."
    SomethingWentWrong,
    /// Save that as a test.
    SaveScenario,
    /// Stop recording.
    StopRecording,
    /// Go off the record, or back on.
    OffTheRecord(bool),
    /// Pause the agents.
    PauseAgents,
    /// What time is it (built-in grammar, precedence 2).
    WhatTime,
    /// How is it going.
    HowsItGoing,
    /// What have you learned.
    WhatHaveYouLearned,
    /// What do you know about me.
    WhatDoYouKnowAboutMe,
    /// Learn from that now.
    LearnFromThatNow,
    /// Record this session.
    RecordSession,
    /// Use a voice.
    UseVoice {
        /// The voice.
        name: String,
        /// The engine.
        engine: Option<String>,
    },
    /// Back to the normal voice.
    BackToNormal,
    /// Forget my voice.
    ForgetMyVoice,
    /// Rename the assistant.
    Rename {
        /// The new name.
        name: String,
    },
    /// Take dictation.
    TakeDictation,
    /// Stop dictating.
    StopDictating,
    /// Think this hard.
    Depth(Depth),
    /// Show me how.
    ShowMeHow,
    /// What can you do here.
    WhatCanYouDoHere,
    /// Show a job's changes.
    ShowChanges {
        /// The job.
        job: JobRef,
    },
    /// Open recent work.
    OpenRecentWork,
    /// Start it anyway.
    StartAnyway,
    /// Fix it.
    FixIt,
    /// Add that server.
    AddThatServer,
    /// Show route suggestions.
    ShowRouteSuggestions,
    /// Explain that.
    ExplainThat,
    /// About Eileen.
    About,
    /// A fact phrase.
    Fact(FactCommand),
}

/// A fact the user stated, from the deterministic fact path. A `FactCommand` of kind `Phrasing`
/// is stored as a `PhrasingFact` (a `SlotAlias` target with its subject set), never as a
/// `UserFact`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FactCommand {
    /// The kind of fact.
    pub kind: FactKind,
    /// The user's normalised final transcript, at most 200 characters, referents in brackets.
    pub text: Personal<String>,
    /// The subject key (section 2.10 item 11): the thing, the phrase of "X means Y", the
    /// bracketed referent of "I loved that `[place]`", or the dimension a refinement word names;
    /// `None` for "remember that".
    pub subject: Option<String>,
    /// Feedback: the current answer's topic only when the statement refers to it; project: the
    /// repository or app.
    pub scope: Option<String>,
    /// Which earlier facts it retires.
    pub retire: RetireRule,
}

/// Which earlier facts a new one retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetireRule {
    /// None.
    None,
    /// Active facts of this kind and subject key whose scope is this one or none (every scope
    /// when this one is `None`); for kind `Phrasing`, the alias phrasings of this slot kind and
    /// subject key.
    SameSubject,
}

/// What Eileen can do here, as the `capabilities` tool and "what can you do here" report it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Capabilities {
    /// The focused app.
    pub focused: Option<String>,
    /// Each app's routes, ranked.
    pub apps: Vec<AppRoutes>,
    /// Learned skills that apply here.
    pub skills: Vec<SkillRef>,
}

/// One app's routes, ranked by their declared rungs and the recorded route statistics
/// (`eileen_skills::routes::rank`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AppRoutes {
    /// The app.
    pub app: String,
    /// What it can do.
    pub verbs: Vec<String>,
    /// Its routes, best first.
    pub rungs: Vec<RankedRung>,
}

/// One route of an app, with its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RankedRung {
    /// The route.
    pub rung: Rung,
    /// Successes recorded.
    pub successes: u32,
    /// Failures recorded.
    pub failures: u32,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<ResolveCtx>();
    r.add::<Awaiting>();
    r.add::<MediaRef>();
    r.add::<LastTurnRefs>();
    r.add::<FollowUp>();
    r.add::<Resolution>();
    r.add::<ToolSource>();
    r.add::<SlotValue>();
    r.add::<CoreCommand>();
    r.add::<FactCommand>();
    r.add::<RetireRule>();
    r.add::<Capabilities>();
    r.add::<AppRoutes>();
    r.add::<RankedRung>();
}

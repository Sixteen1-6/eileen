//! The minimal core through `Core::step`, with a scripted resolver: the gate, the fast path,
//! one brain turn and mute; barge-in is in `barge_in`, the one job in `jobs`.

use eileen_contracts::bus::{BusEvent, TurnRoute};
use eileen_contracts::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use eileen_contracts::connectors::{
    AppCapability, ConnectorManifest, Platform, RunsIn, SidecarEvent, SidecarId,
};
use eileen_contracts::core_io::{
    AudioCmd, Earcon, Effect, Input, PlaybackEvent, TimerId, UserInput,
};
use eileen_contracts::desktop::FieldCategory;
use eileen_contracts::harness::{BrainCmd, BrainEvent, BrainSlot};
use eileen_contracts::hud::{HudState, MicState};
use eileen_contracts::ids::{
    ConnectorId, Envelope, MonoTime, Personal, SkillId, SpeakId, TraceCtx, TurnId, Untrusted,
    WallTime,
};
use eileen_contracts::intent::{
    Capabilities, CoreCommand, FollowUp, Resolution, ResolveCtx, SlotValues,
};
use eileen_contracts::jobs::{Cost, TurnAuthority};
use eileen_contracts::learn::{
    CheckPlan, ForgetTarget, Outcome, PhrasingFact, Recipe, ReplayRefusal, ReplayState, ReplayStep,
    RouteStat, ShadowVerdict, SkillMeta, SkillRef, StepResult, TrajectoryStep, Transition,
    UseRecord, VocabularyEntry,
};
use eileen_contracts::ports::{
    Cleaned, DictationCleaner, IntentResolver, SettingsValidator, SkillEngine,
};
use eileen_contracts::record::CoreSnapshotV1;
use eileen_contracts::speech::{SpeechCmd, TurnEndReason, VoiceEvent};
use eileen_contracts::tools::ToolSpec;
use eileen_contracts::tools::brain::JobRef;
use eileen_stdx::rng::CoreRng;

use crate::{Core, CoreConfig, CorePorts, fresh_snapshot};

/// 2026-01-01T15:05:00Z, in microseconds: every step's wall clock starts here.
const AFTERNOON_US: i64 = 1_767_225_600_000_000 + 905 * 60_000_000;

/// The ports the minimal core never calls.
struct NoPort;

const UNCALLED: &str = "the minimal core does not call this port";

impl DictationCleaner for NoPort {
    fn clean(&self, _: &str, _: FieldCategory, _: &[VocabularyEntry]) -> Cleaned {
        panic!("{UNCALLED}");
    }
}

impl SettingsValidator for NoPort {
    fn validate(&self, _: &Config, _: &[SettingChange]) -> Result<Proposal, ValidationError> {
        panic!("{UNCALLED}");
    }
    fn specs(&self) -> &[SettingSpecView] {
        panic!("{UNCALLED}");
    }
}

impl SkillEngine for NoPort {
    fn rebuild(&mut self, _: &[SkillMeta], _: &[Recipe], _: &[ConnectorManifest], _: &[RouteStat]) {
        panic!("{UNCALLED}");
    }
    fn route_line(&self, _: &str, _: &AppCapability) -> Option<String> {
        panic!("{UNCALLED}");
    }
    fn next_level(&self, _: &SkillId, _: &[UseRecord]) -> Transition {
        panic!("{UNCALLED}");
    }
    fn success(&self, _: &Outcome) -> bool {
        panic!("{UNCALLED}");
    }
    fn start_replay(&self, _: &SkillRef, _: &SlotValues) -> Result<ReplayState, ReplayRefusal> {
        panic!("{UNCALLED}");
    }
    fn next(&self, _: &mut ReplayState, _: Option<&StepResult>) -> ReplayStep {
        panic!("{UNCALLED}");
    }
    fn shadow_checks(&self, _: &SkillRef, _: &SlotValues, _: &[TrajectoryStep]) -> Vec<CheckPlan> {
        panic!("{UNCALLED}");
    }
    fn shadow_agree(&self, _: &SkillRef, _: &SlotValues, _: &[TrajectoryStep]) -> ShadowVerdict {
        panic!("{UNCALLED}");
    }
}

/// A resolver that knows a few phrases and the follow-ups it is told about.
struct Scripted {
    follow_ups: Vec<&'static str>,
}

impl IntentResolver for Scripted {
    fn rebuild(&mut self, _: &[SkillMeta], _: &[PhrasingFact]) {}

    fn resolve(&self, utterance: &str, _: &ResolveCtx) -> Resolution {
        let command = match utterance {
            "what time is it" => Some(CoreCommand::WhatTime),
            "stop" => Some(CoreCommand::Stop),
            "go on" => Some(CoreCommand::GoOn),
            "mute" => Some(CoreCommand::Mute),
            "unmute" => Some(CoreCommand::Unmute),
            "cancel" => Some(CoreCommand::Cancel { job: None }),
            "cancel agent one" => Some(CoreCommand::Cancel { job: Some(JobRef::Ordinal(1)) }),
            "cancel the hello file" => {
                Some(CoreCommand::Cancel { job: Some(JobRef::Name("The hello file".to_owned())) })
            }
            "cancel the other file" => {
                Some(CoreCommand::Cancel { job: Some(JobRef::Name("the other file".to_owned())) })
            }
            "cancel the render" | "cancel the research" | "cancel it" | "cancel claude's job" => {
                let words = utterance.trim_start_matches("cancel ").to_owned();
                Some(CoreCommand::Cancel { job: Some(JobRef::Name(words)) })
            }
            "how's it going" => Some(CoreCommand::HowsItGoing),
            "forget that" => Some(CoreCommand::Forget(ForgetTarget::Fact { id: "f1".to_owned() })),
            "off the record" => Some(CoreCommand::OffTheRecord(true)),
            _ => None,
        };
        command.map_or(Resolution::Brain { depth: None, addressee_unsure: false }, |command| {
            Resolution::Command { command, slots: SlotValues::new() }
        })
    }

    fn resolve_semantic(&self, utterance: &str, _: &[f32], ctx: &ResolveCtx) -> Resolution {
        self.resolve(utterance, ctx)
    }

    fn follow_up(&self, utterance: &str, ctx: &ResolveCtx) -> FollowUp {
        if ctx.last_turn.is_some() && self.follow_ups.contains(&utterance) {
            FollowUp::Shared { word: "render".to_owned() }
        } else {
            FollowUp::None
        }
    }

    fn introspect(
        &self,
        _: &[ToolSpec],
        _: &[AppCapability],
        _: &[RouteStat],
        _: Option<&str>,
    ) -> Capabilities {
        Capabilities { focused: None, apps: Vec::new(), skills: Vec::new() }
    }
}

fn ports(follow_ups: Vec<&'static str>) -> CorePorts {
    CorePorts {
        resolver: Box::new(Scripted { follow_ups }),
        cleaner: Box::new(NoPort),
        settings: Box::new(NoPort),
        skills: Box::new(NoPort),
    }
}

/// A core with a virtual clock, whose speech process is up: the HUD states it sends are kept
/// apart from the other effects, in `hud`.
struct Harness {
    core: Core,
    rng: CoreRng,
    seq: u64,
    now_ms: u64,
    turns: u64,
    hud: Option<HudState>,
    /// How many HUD states the core has sent.
    hud_sent: usize,
}

impl Harness {
    fn new() -> Self {
        Self::with(CoreConfig::default(), Vec::new())
    }

    fn with(config: CoreConfig, follow_ups: Vec<&'static str>) -> Self {
        let mut h = Self::booting(config, follow_ups);
        h.step(Input::User(UserInput::SetMute(false)));
        h.step(speech(ready()));
        h
    }

    /// A core restarted from `snapshot`, as a new process with its clock and random numbers
    /// starting again, whose speech process is up; with the effects of its first step.
    fn resumed(snapshot: CoreSnapshotV1) -> (Self, Vec<Effect>) {
        let mut h = Self::booting(CoreConfig::default(), Vec::new());
        h.core = Core::new(CoreConfig::default(), ports(Vec::new()), Some(snapshot));
        let first = h.step(Input::User(UserInput::SetMute(false)));
        h.step(speech(ready()));
        (h, first)
    }

    /// A core that has taken no step yet.
    fn booting(config: CoreConfig, follow_ups: Vec<&'static str>) -> Self {
        Self {
            core: Core::new(config, ports(follow_ups), None),
            rng: CoreRng::from_seed(3),
            seq: 0,
            now_ms: 1_000,
            turns: 0,
            hud: None,
            hud_sent: 0,
        }
    }

    /// One step; the HUD state it sends, if any, goes to `hud`.
    fn step(&mut self, body: Input) -> Vec<Effect> {
        self.seq += 1;
        let mono = MonoTime(self.now_ms * 1_000_000);
        let at = WallTime(AFTERNOON_US + i64::try_from(self.now_ms).unwrap() * 1_000);
        let input = Envelope { seq: self.seq, at, mono, ctx: TraceCtx::default(), body };
        let mut effects = Vec::new();
        self.core.step(mono, &mut self.rng, input, &mut effects);
        let mut rest = Vec::new();
        for effect in effects {
            if let Effect::Hud(state) = effect.body {
                self.hud = Some(*state);
                self.hud_sent += 1;
            } else {
                rest.push(effect.body);
            }
        }
        rest
    }

    /// What the HUD shows instead of Eileen saying it.
    fn hud_line(&self) -> Option<&str> {
        self.hud.as_ref().and_then(|state| state.speaking.as_deref())
    }

    /// The microphone as the HUD shows it.
    fn mic(&self) -> Option<&MicState> {
        self.hud.as_ref().map(|state| &state.mic)
    }

    fn wait(&mut self, ms: u64) {
        self.now_ms += ms;
    }

    /// The user says `text` as a whole turn; returns the effects of its end of turn.
    fn say(&mut self, text: &str) -> Vec<Effect> {
        self.turns += 1;
        let turn = TurnId::from_parts(self.turns, u128::from(self.turns) + 77);
        self.step(Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }));
        self.wait(400);
        self.step(Input::Voice(end_of_turn(turn, text)))
    }
}

/// From the speech process's sidecar client.
const fn speech(event: SidecarEvent) -> Input {
    Input::Sidecar(SidecarId::Speech, event)
}

/// The speech process's `Ready`, which serves no tools.
fn ready() -> SidecarEvent {
    SidecarEvent::Ready {
        manifest: Box::new(ConnectorManifest {
            id: ConnectorId::new("pc.speech").unwrap(),
            version: "0.1.0".to_owned(),
            runs: RunsIn::Sidecar { binary: "eileen-speech".to_owned() },
            platforms: vec![Platform::Windows],
            tools: Vec::new(),
            events: Vec::new(),
            checks: Vec::new(),
            senses_room: false,
            requires_opt_in: false,
        }),
    }
}

fn end_of_turn(turn: TurnId, text: &str) -> VoiceEvent {
    VoiceEvent::EndOfTurn {
        turn,
        text: Personal::new(text.to_owned()),
        smart_turn_pct: 90,
        silence_ms: 300,
        asr_confidence_pct: 95,
        reason: TurnEndReason::SmartTurn,
        source: 0,
        level_db: -30,
    }
}

fn events(effects: &[Effect]) -> Vec<&BusEvent> {
    effects
        .iter()
        .filter_map(|e| if let Effect::Emit(event) = e { Some(event) } else { None })
        .collect()
}

fn spoken(effects: &[Effect]) -> Vec<(SpeakId, Vec<String>)> {
    effects
        .iter()
        .filter_map(|e| {
            if let Effect::Speech(SpeechCmd::Speak { speak, clauses, .. }) = e {
                Some((*speak, clauses.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn brain_turn(effects: &[Effect]) -> Option<(TurnId, String)> {
    effects.iter().find_map(|e| {
        if let Effect::Brain(BrainCmd::Turn { turn, message, slot: BrainSlot::Primary, .. }) = e {
            Some((*turn, message.clone()))
        } else {
            None
        }
    })
}

fn timer_set(effects: &[Effect]) -> Option<(TimerId, MonoTime)> {
    effects
        .iter()
        .rev()
        .find_map(|e| if let Effect::SetTimer { id, at } = e { Some((*id, *at)) } else { None })
}

fn delta(turn: TurnId, text: &str) -> Input {
    Input::Brain(BrainEvent::TextDelta { instance: 1, turn, text: Untrusted::new(text.to_owned()) })
}

fn done(turn: TurnId) -> Input {
    Input::Brain(BrainEvent::TurnDone {
        instance: 1,
        turn,
        cost: Cost::default(),
        context_tokens: 0,
        is_error: false,
    })
}

/// `eileen say <text>`.
fn cli(text: &str) -> Input {
    Input::User(UserInput::Cli { text: Personal::new(text.to_owned()) })
}

/// Starts a brain turn for `text` and returns it.
fn ask_brain(h: &mut Harness, text: &str) -> TurnId {
    let effects = h.say(text);
    brain_turn(&effects).expect("routed to the brain").0
}

#[test]
fn a_new_core_reports_a_fresh_snapshot_with_its_counters() {
    let core = Core::new(CoreConfig::default(), ports(Vec::new()), None);
    let snapshot = core.snapshot();
    assert_eq!(snapshot.v, 1, "version 1");
    assert!(!snapshot.muted, "not muted");
    assert_eq!(snapshot.counters.get("effect_seq"), Some(&0), "no effect yet");
    assert_eq!(fresh_snapshot().jobs.len(), 0, "a fresh snapshot holds no jobs");
}

#[test]
fn a_core_resumes_muted_with_its_counters() {
    let saved = CoreSnapshotV1 {
        muted: true,
        counters: [("effect_seq".to_owned(), 40), ("timer_ids".to_owned(), 7)]
            .into_iter()
            .collect(),
        ..fresh_snapshot()
    };
    let mut h = Harness::new();
    h.core = Core::new(CoreConfig::default(), ports(Vec::new()), Some(saved));
    assert!(h.core.turns().muted(), "resumed muted");
    let effects = h.step(Input::User(UserInput::SetMute(false)));
    assert!(
        effects.contains(&Effect::Audio(AudioCmd::SetCapture(true))),
        "unmuting starts capture"
    );
    // The first step also starts speech: `Start`, its load deadline and the HUD's state.
    assert_eq!(h.core.snapshot().counters.get("effect_seq"), Some(&46), "seq continues from 40");
}

#[test]
fn what_time_is_answered_by_the_core_with_no_brain_turn() {
    let mut h = Harness::new();
    let effects = h.say("Eileen, what time is it?");
    let routes: Vec<&BusEvent> = events(&effects);
    assert!(
        matches!(
            routes.first(),
            Some(BusEvent::TurnStarted { authority: TurnAuthority::Addressed, .. })
        ),
        "{routes:?}"
    );
    assert!(
        matches!(routes.get(1), Some(BusEvent::TurnRouted { route: TurnRoute::FastPath, .. })),
        "routed before it is heard: {routes:?}"
    );
    assert!(
        matches!(routes.get(2), Some(BusEvent::Heard { text }) if text.get() == "Eileen, what time is it?"),
        "{routes:?}"
    );
    assert_eq!(
        spoken(&effects).first().map(|s| s.1.clone()),
        Some(vec!["It's 3:05 pm.".to_owned()]),
        "spoken"
    );
    assert_eq!(brain_turn(&effects), None, "no brain turn");
}

#[test]
fn room_talk_outside_the_window_is_greyed_and_never_a_bus_event() {
    let mut h = Harness::new();
    let effects = h.say("want coffee?");
    assert!(effects.is_empty(), "nothing at all: {effects:?}");
}

#[test]
fn a_built_in_command_is_addressed_without_the_name() {
    let mut h = Harness::new();
    let effects = h.say("what time is it");
    assert!(!spoken(&effects).is_empty(), "rule 4 addresses a full built-in command");
}

#[test]
fn a_follow_up_inside_the_window_is_addressed_and_room_talk_is_not() {
    let mut h = Harness::with(CoreConfig::default(), vec!["and how long will it take"]);
    let effects = h.say("Eileen, what time is it?");
    let (speak, _) = spoken(&effects)[0].clone();
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 13, samples: 1_000 }));
    let effects = h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::ConversationWindow { open: true, .. })),
        "the window opens when Eileen stops: {effects:?}"
    );
    h.wait(1_000);
    assert!(h.say("want coffee?").is_empty(), "inside the window, room talk stays greyed");
    let effects = h.say("and how long will it take?");
    assert!(brain_turn(&effects).is_some(), "a follow-up that resolves passes to the brain");
}

#[test]
fn the_window_closes_on_its_timer() {
    let mut h = Harness::new();
    let effects = h.say("Eileen, what time is it?");
    let (speak, _) = spoken(&effects)[0].clone();
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 13, samples: 1_000 }));
    let effects = h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    let (timer, at) = timer_set(&effects).expect("the window timer");
    h.now_ms = at.0 / 1_000_000;
    let effects = h.step(Input::Timer(timer));
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::ConversationWindow { open: false, .. })),
        "closed: {effects:?}"
    );
    assert!(h.say("and the other one").is_empty(), "outside the window again");
}

#[test]
fn a_brain_turn_starts_with_the_earcon_and_is_spoken_clause_by_clause() {
    let mut h = Harness::new();
    let effects = h.say("Eileen, tell me about rivers");
    let earcon = effects.iter().position(|e| *e == Effect::Audio(AudioCmd::Earcon(Earcon::Routed)));
    let turn_at = effects.iter().position(|e| matches!(e, Effect::Brain(BrainCmd::Turn { .. })));
    assert!(earcon.is_some() && earcon < turn_at, "the earcon comes first, in the same step");
    let (turn, message) = brain_turn(&effects).unwrap();
    assert!(message.contains("[heard] Eileen, tell me about rivers"), "{message}");
    assert!(message.starts_with("[assistant] name Eileen;"), "{message}");
    let effects = h.step(delta(turn, "Rivers carry water to the sea. They also"));
    let first = spoken(&effects);
    assert_eq!(first.len(), 1, "one clause so far: {effects:?}");
    assert_eq!(first[0].1, ["Rivers carry water to the sea."], "the first clause");
    let speak = first[0].0;
    h.step(delta(turn, " shape valleys."));
    let effects = h.step(done(turn));
    assert_eq!(
        spoken(&effects),
        [(speak, vec!["They also shape valleys.".to_owned()])],
        "same utterance"
    );
    for clause in 0..2 {
        h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause, chars: 20, samples: 1_000 }));
    }
    let effects = h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SpeakingStopped { interrupted: false, heard_chars: 55, .. }
        )),
        "the whole answer was heard: {effects:?}"
    );
}

#[test]
fn text_from_another_instance_or_turn_is_dropped() {
    let mut h = Harness::new();
    h.step(Input::Brain(BrainEvent::Ready {
        which: BrainSlot::Primary,
        instance: 2,
        route: eileen_contracts::jobs::Route::Claude,
        model: "fast-model".to_owned(),
    }));
    let turn = ask_brain(&mut h, "Eileen, tell me something");
    assert!(
        spoken(&h.step(delta(turn, "Late text from the old process. "))).is_empty(),
        "instance 1"
    );
    let other = TurnId::from_parts(99, 99);
    let late = Input::Brain(BrainEvent::TextDelta {
        instance: 2,
        turn: other,
        text: Untrusted::new("Another turn. ".to_owned()),
    });
    assert!(spoken(&h.step(late)).is_empty(), "another turn");
    let ours = Input::Brain(BrainEvent::TextDelta {
        instance: 2,
        turn,
        text: Untrusted::new("Here it is. ".to_owned()),
    });
    assert_eq!(spoken(&h.step(ours)).len(), 1, "the instance it was sent to");
}

#[test]
fn mute_stops_capture_and_nothing_is_spoken_while_muted() {
    let mut h = Harness::new();
    let effects = h.step(Input::User(UserInput::SetMute(true)));
    assert_eq!(
        effects,
        [
            Effect::Audio(AudioCmd::Earcon(Earcon::Muted)),
            Effect::Audio(AudioCmd::SetCapture(false)),
            Effect::Emit(BusEvent::MuteChanged { muted: true }),
        ],
        "the earcon at the change, then capture stops"
    );
    assert!(h.say("Eileen, what time is it?").is_empty(), "no speech is heard while muted");
    let effects =
        h.step(Input::User(UserInput::Typed { text: Personal::new("what time is it".to_owned()) }));
    assert!(spoken(&effects).is_empty(), "a typed command is answered, but not aloud: {effects:?}");
    let effects = h.step(Input::User(UserInput::ToggleMute));
    assert!(
        effects.contains(&Effect::Emit(BusEvent::MuteChanged { muted: false })),
        "toggled back"
    );
}

#[test]
fn saying_mute_mutes() {
    let mut h = Harness::new();
    let effects = h.say("Eileen, mute");
    assert!(effects.contains(&Effect::Audio(AudioCmd::SetCapture(false))), "{effects:?}");
    assert!(h.core.snapshot().muted, "the snapshot says so");
}

#[test]
fn other_resolutions_go_to_the_brain() {
    let mut h = Harness::new();
    let effects = h.say("Eileen, how's it going");
    assert!(brain_turn(&effects).is_some(), "a core command the minimal core does not answer");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::TurnRouted { route: TurnRoute::Brain, .. })),
        "routed to the brain"
    );
}

#[test]
fn eileen_say_cannot_unmute() {
    let mut h = Harness::new();
    h.step(Input::User(UserInput::SetMute(true)));
    let effects = h.step(cli("unmute"));
    assert!(
        !effects.contains(&Effect::Audio(AudioCmd::SetCapture(true))),
        "capture stays off: {effects:?}"
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Emit(BusEvent::MuteChanged { .. }))),
        "no mute change: {effects:?}"
    );
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::TurnRouted { route: TurnRoute::FastPath, reason } if reason == "unmute_by_hand"
        )),
        "the bus says why: {effects:?}"
    );
    assert!(h.core.turns().muted(), "still muted");
    assert!(spoken(&effects).is_empty(), "nothing is spoken while muted");
    let effects =
        h.step(Input::User(UserInput::Typed { text: Personal::new("unmute".to_owned()) }));
    assert!(
        effects.contains(&Effect::Audio(AudioCmd::SetCapture(true))),
        "text typed in Eileen's own UI is the user's: {effects:?}"
    );
    let effects = h.step(cli("mute"));
    assert!(
        effects.contains(&Effect::Audio(AudioCmd::SetCapture(false))),
        "eileen say may still mute: {effects:?}"
    );
}

#[test]
fn a_repeated_end_of_turn_is_routed_once() {
    let mut h = Harness::new();
    let turn = TurnId::from_parts(40, 40);
    h.step(Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }));
    h.wait(400);
    let first = h.step(Input::Voice(end_of_turn(turn, "what time is it")));
    let again = h.step(Input::Voice(end_of_turn(turn, "what time is it")));
    let started = |effects: &[Effect]| {
        events(effects).iter().filter(|e| matches!(e, BusEvent::TurnStarted { .. })).count()
    };
    assert_eq!((started(&first), spoken(&first).len()), (1, 1), "the first: {first:?}");
    assert_eq!((started(&again), spoken(&again).len()), (0, 0), "the repeat: {again:?}");
    let asked = ask_brain(&mut h, "Eileen, tell me about rivers");
    h.step(done(asked));
    let again = h.step(Input::Voice(end_of_turn(asked, "Eileen, tell me about rivers")));
    assert_eq!(brain_turn(&again), None, "a finished brain turn is not sent again: {again:?}");
}

#[test]
fn an_ambient_turn_never_rides_on_an_addressed_one() {
    let mut h = Harness::new();
    let ambient = brain_turn(&h.step(cli("dispatch a job to clean my files"))).expect("sent");
    assert!(ambient.1.contains("clean my files"), "{}", ambient.1);
    let effects = h.say("Eileen, tell me about rivers");
    let (_, message) = brain_turn(&effects).expect("a new brain turn");
    assert!(message.contains("clean my files") && message.contains("rivers"), "both: {message}");
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Brain(BrainCmd::Turn { authority: TurnAuthority::Ambient, .. })
        )),
        "the merged turn has the weaker authority: {effects:?}"
    );
    let effects = h.say("Eileen, tell me about lakes");
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Brain(BrainCmd::Turn { authority: TurnAuthority::Ambient, .. })
        )),
        "and keeps it when merged again: {effects:?}"
    );
}

#[test]
fn an_unanswered_turn_is_merged_into_the_next_of_the_same_authority() {
    let mut h = Harness::new();
    h.say("Eileen, tell me about rivers");
    let (_, message) = brain_turn(&h.say("Eileen, and lakes")).expect("a new brain turn");
    assert!(
        message.contains("[heard] Eileen, tell me about rivers Eileen, and lakes"),
        "{message}"
    );
}

#[test]
fn safety_commands_not_built_yet_are_answered_and_never_reach_the_brain() {
    for text in ["Eileen, forget that", "Eileen, off the record"] {
        let mut h = Harness::new();
        let effects = h.say(text);
        assert_eq!(brain_turn(&effects), None, "{text}: no brain turn");
        assert_eq!(
            spoken(&effects).first().map(|s| s.1.clone()),
            Some(vec!["I can't do that yet.".to_owned()]),
            "{text}: said so"
        );
        assert!(
            events(&effects).iter().any(|e| matches!(
                e,
                BusEvent::TurnRouted { route: TurnRoute::FastPath, reason } if reason == "not_built"
            )),
            "{text}: {effects:?}"
        );
    }
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let effects = h.say("Eileen, forget that");
    assert!(
        effects.contains(&Effect::Brain(BrainCmd::Interrupt { slot: BrainSlot::Primary })),
        "the brain turn in flight is interrupted: {effects:?}"
    );
    assert!(spoken(&h.step(delta(turn, "Rivers carry water. "))).is_empty(), "its text is dropped");
}

mod barge_in;
mod jobs;
mod life;
mod steering;

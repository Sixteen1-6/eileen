//! A finished utterance through the addressee gate and the router (BUILD.md section 2.2 steps 3
//! to 6): the gate decides whether it was meant for Eileen, the resolver says what it means, and
//! the core answers the safety and control commands and "what time is it" itself; every other
//! resolution goes to the brain.
//!
//! Unmuting is for the user's hands (section 3.3.3, after `UserInput`): an `eileen say` turn,
//! which is ambient (sections 2.3 and 2.12), may mute, stop and cancel but never unmute; the
//! refusal is routed with the reason `unmute_by_hand`, since nothing is spoken while muted. The
//! safety and control commands the minimal core does not run yet are answered with a sentence
//! and never reach the brain, which could otherwise claim to have done them (section 2.4,
//! precedence 1).

use std::collections::VecDeque;

use eileen_contracts::bus::{BusEvent, SpeakCause, TurnRoute};
use eileen_contracts::ids::{MonoTime, Personal, TurnId};
use eileen_contracts::intent::{
    CoreCommand, FollowUp, LastTurnRefs, Resolution, ResolveCtx, ToolSource,
};
use eileen_contracts::jobs::{Depth, TurnAuthority, TurnSource};
use eileen_contracts::tools::brain::JobRef;

use crate::addressee::{self, Decision, Facts, Name};
use crate::speech::Owner;
use crate::turn::Verdict;
use crate::{Core, Cx, clock, turn_ctx};

/// What Eileen says to a safety or control command the minimal core does not run yet.
const NOT_BUILT: &str = "I can't do that yet.";

/// How many speech turns each [`TurnSet`] remembers: the speech process hears one turn at a
/// time, so a turn's later events come long before eight newer turns have started.
const TURNS_KEPT: usize = 8;

/// A few recent speech turns, newest last, at most `TURNS_KEPT`: the turns already routed or
/// dropped when speech went down, so a repeated `EndOfTurn` is not routed again, and the turns
/// heard late, whose speech began in a replayed frame.
#[derive(Debug, Default)]
pub(crate) struct TurnSet(VecDeque<TurnId>);

impl TurnSet {
    /// Whether `turn` is in the set.
    pub(crate) fn contains(&self, turn: TurnId) -> bool {
        self.0.contains(&turn)
    }

    /// Adds `turn`, forgetting the oldest when full; false when it was in the set already.
    pub(crate) fn insert(&mut self, turn: TurnId) -> bool {
        if self.0.contains(&turn) {
            return false;
        }
        if self.0.len() >= TURNS_KEPT {
            self.0.pop_front();
        }
        self.0.push_back(turn);
        true
    }
}

/// Whether `resolution` is a safety or control command (section 2.4, precedence 1) that the
/// minimal core does not run yet.
const fn safety_not_built(resolution: &Resolution) -> bool {
    let Resolution::Command { command, .. } = resolution else { return false };
    matches!(
        command,
        CoreCommand::ThatWasForYou
            | CoreCommand::Undo
            | CoreCommand::UndoFact
            | CoreCommand::Forget(_)
            | CoreCommand::SomethingWentWrong
            | CoreCommand::SaveScenario
            | CoreCommand::StopRecording
            | CoreCommand::OffTheRecord(_)
            | CoreCommand::PauseAgents
    )
}

/// The commands the minimal core answers itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fast {
    Mute,
    Unmute,
    Stop,
    Cancel(Option<JobRef>),
    GoOn,
    WhatTime,
}

impl Fast {
    /// The command a resolution names, when the minimal core answers it.
    pub(crate) fn of(resolution: &Resolution) -> Option<Self> {
        let Resolution::Command { command, .. } = resolution else { return None };
        if let CoreCommand::Cancel { job } = command {
            return Some(Self::Cancel(job.clone()));
        }
        let simple = [
            (CoreCommand::Mute, Self::Mute),
            (CoreCommand::Unmute, Self::Unmute),
            (CoreCommand::Stop, Self::Stop),
            (CoreCommand::GoOn, Self::GoOn),
            (CoreCommand::WhatTime, Self::WhatTime),
        ];
        simple.into_iter().find(|(known, _)| known == command).map(|(_, fast)| fast)
    }

    /// The command's name, for the bus and the spans.
    pub(crate) const fn name(&self) -> &'static str {
        match self {
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Stop => "stop",
            Self::Cancel(_) => "cancel",
            Self::GoOn => "go_on",
            Self::WhatTime => "what_time",
        }
    }
}

/// Whether a resolution is a full match of a built-in command (rule 4): a core command, or a
/// grammar entry that is a plain tool call. Learned templates and phrasings are not.
pub(crate) const fn is_full_command(resolution: &Resolution) -> bool {
    matches!(
        resolution,
        Resolution::Command { .. } | Resolution::Tool { source: ToolSource::Grammar, .. }
    )
}

/// `TurnRouted`, then `Heard`: a turn is heard on the bus only once it is routed.
fn emit_routed(cx: &mut Cx<'_>, routed: Routed<'_>, route: TurnRoute, reason: &str) {
    let ctx = turn_ctx(routed.turn);
    cx.emit(ctx, BusEvent::TurnRouted { route, reason: reason.to_owned() });
    cx.emit(ctx, BusEvent::Heard { text: Personal::new(routed.text.to_owned()) });
}

/// An addressed turn on its way to the router.
#[derive(Debug, Clone, Copy)]
struct Routed<'t> {
    turn: TurnId,
    text: &'t str,
    authority: TurnAuthority,
}

impl Core {
    /// The speech process's final transcript of `turn`. A turn already handled is not routed
    /// again.
    pub(crate) fn end_of_turn(&mut self, cx: &mut Cx<'_>, turn: TurnId, text: &str) {
        if !self.handled.insert(turn) {
            return;
        }
        self.turns.end_of_turn();
        self.partial = None;
        if self.turns.muted() {
            return;
        }
        self.utterance(cx, turn, text, TurnSource::Voice);
    }

    /// Text typed in Eileen's own UI, or sent from the command line, as a turn of its own.
    pub(crate) fn typed(&mut self, cx: &mut Cx<'_>, text: &str, source: TurnSource) {
        let turn = TurnId::from_parts(cx.mint(), cx.random());
        self.utterance(cx, turn, text, source);
    }

    /// The assistant's name and the calibration's aliases for it.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names = vec![self.config.config.assistant.name.clone()];
        if let Some(calibration) = &self.config.calibration {
            names.extend(calibration.name_aliases.iter().map(|alias| alias.get().clone()));
        }
        names
    }

    /// What the resolver is told: the name, and what Eileen's own last turn mentioned.
    pub(crate) fn resolve_ctx(&self, now: MonoTime) -> ResolveCtx {
        let last_turn = self.last_spoken.as_ref().map(|last| LastTurnRefs {
            age_ms: u32::try_from(now.0.saturating_sub(last.at.0) / 1_000_000).unwrap_or(u32::MAX),
            items: Vec::new(),
            job: last.job.map(JobRef::Id),
            apps: Vec::new(),
            files: Vec::new(),
            media: Vec::new(),
            content_words: last.words.clone(),
            topic: None,
        });
        ResolveCtx {
            focused_app: None,
            media_active: false,
            dictation: false,
            awaiting: None,
            assistant_name: self.config.config.assistant.name.clone(),
            monitors: 1,
            media_history: Vec::new(),
            last_turn,
        }
    }

    /// One utterance through the gate; an addressed one goes on to the router. A spoken turn
    /// heard late is addressed only when it begins with the name (section 2.8).
    fn utterance(&mut self, cx: &mut Cx<'_>, turn: TurnId, text: &str, source: TurnSource) {
        let named = addressee::strip_name(text, &self.names());
        let ctx = self.resolve_ctx(cx.now);
        let resolution = self.ports.resolver.resolve(&named.rest, &ctx);
        let voice = matches!(source, TurnSource::Voice);
        let late = voice && self.late.contains(turn);
        self.spans.open_turn(turn, &source);
        if voice
            && self.turns.barge_in()
            && !named.named()
            && addressee::is_backchannel(&named.rest)
        {
            self.spans.addressee(turn, Decision::Greyed.name(), "backchannel");
            self.resolve_barge_in(cx, Verdict::Resume);
            self.spans.close_turn_if_done(turn);
            return;
        }
        let decision = if voice {
            let facts = Facts {
                name: named.name,
                late,
                in_window: self.window.is_open(cx.now) || self.turns.speaking().is_some(),
                command: is_full_command(&resolution),
            };
            let resolver = &self.ports.resolver;
            addressee::decide(facts, || {
                !matches!(resolver.follow_up(&named.rest, &ctx), FollowUp::None)
            })
        } else {
            Decision::Typed
        };
        self.spans.addressee(turn, decision.name(), decision.reason());
        if !decision.addressed() {
            if self.turns.barge_in() {
                self.resolve_barge_in(cx, Verdict::Resume);
            }
            self.spans.close_turn_if_done(turn);
            return;
        }
        self.window.addressed(cx.now);
        let authority = if matches!(source, TurnSource::Cli) {
            TurnAuthority::Ambient
        } else {
            TurnAuthority::Addressed
        };
        cx.emit(turn_ctx(turn), BusEvent::TurnStarted { source, authority });
        self.route(cx, Routed { turn, text, authority }, &resolution);
        self.spans.close_turn_if_done(turn);
    }

    /// The fast path for the commands the minimal core answers, a sentence for the safety
    /// commands it does not run yet, and the brain for the rest.
    fn route(&mut self, cx: &mut Cx<'_>, routed: Routed<'_>, resolution: &Resolution) {
        if let Some(fast) = Fast::of(resolution) {
            self.fast_path(cx, routed, fast);
        } else if safety_not_built(resolution) {
            self.not_built(cx, routed);
        } else {
            let depth =
                if let Resolution::Brain { depth, .. } = resolution { *depth } else { None };
            self.route_to_brain(cx, routed, depth);
        }
    }

    fn fast_path(&mut self, cx: &mut Cx<'_>, routed: Routed<'_>, fast: Fast) {
        let refused = fast == Fast::Unmute && routed.authority == TurnAuthority::Ambient;
        let name = if refused { "unmute_by_hand" } else { fast.name() };
        emit_routed(cx, routed, TurnRoute::FastPath, name);
        self.spans.router(routed.turn, "fast_path", name);
        self.spans.fast_path(routed.turn, name);
        if fast != Fast::Unmute {
            self.interrupt_brain(cx);
        }
        match fast {
            Fast::Mute => self.set_mute(cx, true),
            Fast::Unmute if refused => {}
            Fast::Unmute => self.set_mute(cx, false),
            Fast::Stop => self.stop_speaking(cx, true),
            Fast::Cancel(job) => {
                self.stop_speaking(cx, true);
                self.cancel_spoken(cx, routed.turn, job.as_ref());
            }
            Fast::GoOn => self.go_on(cx, routed.turn),
            Fast::WhatTime => {
                self.stop_speaking(cx, true);
                let line = clock::spoken_time(cx.at, &self.config.local_time);
                self.say(cx, Owner::turn(routed.turn), SpeakCause::Reply, &line);
            }
        }
    }

    /// A safety or control command the minimal core does not run: said so, and the brain is
    /// not asked.
    fn not_built(&mut self, cx: &mut Cx<'_>, routed: Routed<'_>) {
        emit_routed(cx, routed, TurnRoute::FastPath, "not_built");
        self.spans.router(routed.turn, "fast_path", "not_built");
        self.interrupt_brain(cx);
        self.stop_speaking(cx, true);
        self.say(cx, Owner::turn(routed.turn), SpeakCause::Reply, NOT_BUILT);
    }

    /// The brain, with the depth the user's words set (an L1 override), if any.
    fn route_to_brain(&mut self, cx: &mut Cx<'_>, routed: Routed<'_>, depth: Option<Depth>) {
        emit_routed(cx, routed, TurnRoute::Brain, "brain");
        self.spans.router(routed.turn, "brain", "not a core command");
        self.stop_speaking(cx, true);
        self.send_brain_turn(cx, routed.turn, routed.text, routed.authority, depth);
    }

    /// "Go on": a paused barge-in resumes from the paused sample; after a stop, the part the
    /// user did not hear is spoken again.
    fn go_on(&mut self, cx: &mut Cx<'_>, turn: TurnId) {
        if self.turns.barge_in() {
            self.resolve_barge_in(cx, Verdict::Resume);
        } else if let Some(rest) = self.turns.take_rest() {
            self.say(cx, Owner::turn(turn), SpeakCause::Reply, &rest);
        }
    }

    /// The barge-in timer fired: a pause is decided from the latest partial transcript.
    pub(crate) fn barge_in_deadline(&mut self, cx: &mut Cx<'_>) {
        let mut out = Vec::new();
        if self.turns.barge_in_deadline(&mut out) {
            let verdict = self.verdict_from_partial(cx.now);
            self.turns.resolve_barge_in(verdict, &mut out);
        }
        self.apply(cx, out);
    }

    /// Within 600 ms of a pause: an addressed partial (the name, a full command, a follow-up)
    /// stops Eileen, except "go on"; a backchannel, unaddressed speech or nothing heard resumes.
    /// A partial of a turn heard late is addressed only when it begins with the name.
    fn verdict_from_partial(&self, now: MonoTime) -> Verdict {
        let Some((turn, text)) = &self.partial else { return Verdict::Resume };
        let named = addressee::strip_name(text, &self.names());
        if !named.named() && addressee::is_backchannel(&named.rest) {
            return Verdict::Resume;
        }
        let ctx = self.resolve_ctx(now);
        let resolution = self.ports.resolver.resolve(&named.rest, &ctx);
        if Fast::of(&resolution) == Some(Fast::GoOn) {
            return Verdict::Resume;
        }
        let addressed = if self.late.contains(*turn) {
            named.name == Name::Leads
        } else {
            named.named()
                || is_full_command(&resolution)
                || !matches!(self.ports.resolver.follow_up(&named.rest, &ctx), FollowUp::None)
        };
        if addressed { Verdict::Stop } else { Verdict::Resume }
    }
}

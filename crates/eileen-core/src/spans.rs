//! The core's spans (BUILD.md section 2.13; names and parents from `eileen_trace::names`).
//!
//! A span lives as long as what it measures, across steps: a turn from its end of speech
//! until its brain turn and its reply have ended, a job until its terminal state. The
//! decisions inside one step (`addressee`, `router.decide`, `fastpath.run`) open and close in
//! it. A child keeps its parent open in the subscriber until it closes, so a job (the one span
//! allowed to outlive its turn) holds the turn's record open with it.

use std::collections::BTreeMap;

use eileen_contracts::bus::SpeakCause;
use eileen_contracts::ids::{JobId, SpeakId, TurnId};
use eileen_contracts::jobs::{JobKind, Route, TurnSource};
use eileen_trace::names;
use tracing::Span;
use tracing::field::Empty;

/// Where an utterance's `speak` span sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpeakParent {
    /// A reply, under its turn.
    Turn(TurnId),
    /// An announcement with no open turn: a trace of its own.
    Root,
}

#[derive(Debug)]
struct TurnSpans {
    turn: Span,
    brain: Option<Span>,
    reply: Option<SpeakId>,
}

#[derive(Debug)]
struct SpeakSpans {
    turn: Option<TurnId>,
    play: Span,
    barge: Option<Span>,
    speak: Span,
}

#[derive(Debug)]
struct JobSpans {
    session: Option<Span>,
    job: Span,
}

/// Every open span, by what it measures.
#[derive(Debug, Default)]
pub(crate) struct Spans {
    turns: BTreeMap<TurnId, TurnSpans>,
    speaks: BTreeMap<SpeakId, SpeakSpans>,
    jobs: BTreeMap<JobId, JobSpans>,
}

impl Spans {
    /// Opens `turn`'s span; false when it is open already.
    pub(crate) fn open_turn(&mut self, turn: TurnId, source: &TurnSource) -> bool {
        if self.turns.contains_key(&turn) {
            return false;
        }
        let span = tracing::info_span!(
            parent: None,
            names::TURN,
            turn_id = %turn,
            source = ?source,
            outcome = Empty,
        );
        self.turns.insert(turn, TurnSpans { turn: span, brain: None, reply: None });
        true
    }

    fn turn_id(&self, turn: TurnId) -> Option<tracing::Id> {
        self.turns.get(&turn).and_then(|spans| spans.turn.id())
    }

    /// The gate's decision, inside `turn`.
    pub(crate) fn addressee(&self, turn: TurnId, decision: &str, reason: &str) {
        let parent = self.turn_id(turn);
        let span = tracing::info_span!(parent: parent, names::ADDRESSEE, decision, reason);
        drop(span);
    }

    /// The route a turn took, inside `turn`; the turn records it as its outcome.
    pub(crate) fn router(&self, turn: TurnId, route: &str, reason: &str) {
        let parent = self.turn_id(turn);
        let span = tracing::info_span!(parent: parent, names::ROUTER_DECIDE, route, reason);
        drop(span);
        if let Some(spans) = self.turns.get(&turn) {
            spans.turn.record("outcome", route);
        }
    }

    /// A fast-path command, inside `turn`.
    pub(crate) fn fast_path(&self, turn: TurnId, command: &str) {
        let parent = self.turn_id(turn);
        let span = tracing::info_span!(parent: parent, names::FASTPATH_RUN, command);
        drop(span);
    }

    /// A brain turn began for `turn`.
    pub(crate) fn open_brain(&mut self, turn: TurnId) {
        let parent = self.turn_id(turn);
        if let Some(spans) = self.turns.get_mut(&turn) {
            spans.brain = Some(tracing::info_span!(parent: parent, names::BRAIN_TURN));
        }
    }

    /// `turn`'s brain turn ended.
    pub(crate) fn close_brain(&mut self, turn: TurnId) {
        if let Some(spans) = self.turns.get_mut(&turn) {
            spans.brain = None;
        }
        self.close_turn_if_done(turn);
    }

    /// Closes `turn`'s span once neither a brain turn nor a reply is open under it.
    pub(crate) fn close_turn_if_done(&mut self, turn: TurnId) {
        let done = self.turns.get(&turn).is_some_and(|s| s.brain.is_none() && s.reply.is_none());
        if done {
            self.turns.remove(&turn);
        }
    }

    /// An utterance began: its `speak` and `audio.play` spans.
    pub(crate) fn open_speak(
        &mut self,
        speak: SpeakId,
        parent: SpeakParent,
        cause: SpeakCause,
        chars: usize,
    ) {
        let turn = match parent {
            SpeakParent::Turn(turn) if self.turns.contains_key(&turn) => Some(turn),
            SpeakParent::Turn(_) | SpeakParent::Root => None,
        };
        let parent_id = turn.and_then(|turn| self.turn_id(turn));
        let speak_span = tracing::info_span!(
            parent: parent_id,
            names::SPEAK,
            speak_id = %speak,
            cause = ?cause,
            chars,
        );
        let play = tracing::info_span!(
            parent: speak_span.id(),
            names::AUDIO_PLAY,
            played_samples = Empty,
            heard_chars = Empty,
            interrupted = Empty,
        );
        if let Some(turn) = turn
            && let Some(spans) = self.turns.get_mut(&turn)
        {
            spans.reply = Some(speak);
        }
        self.speaks.insert(speak, SpeakSpans { turn, play, barge: None, speak: speak_span });
    }

    /// A barge-in began on `speak`.
    pub(crate) fn open_barge_in(&mut self, speak: SpeakId) {
        if let Some(spans) = self.speaks.get_mut(&speak) {
            let parent = spans.speak.id();
            spans.barge =
                Some(tracing::info_span!(parent: parent, names::BARGEIN, detect_ms = Empty));
        }
    }

    /// The barge-in on `speak` ended.
    pub(crate) fn close_barge_in(&mut self, speak: SpeakId) {
        if let Some(spans) = self.speaks.get_mut(&speak) {
            spans.barge = None;
        }
    }

    /// An utterance ended: its spans close, and its turn's once nothing else is open there.
    pub(crate) fn close_speak(&mut self, speak: SpeakId, heard_chars: u32, interrupted: bool) {
        let Some(spans) = self.speaks.remove(&speak) else { return };
        spans.play.record("heard_chars", heard_chars);
        spans.play.record("interrupted", interrupted);
        let SpeakSpans { turn, play, barge, speak: speak_span } = spans;
        drop(barge);
        drop(play);
        drop(speak_span);
        if let Some(turn) = turn {
            if let Some(turn_spans) = self.turns.get_mut(&turn)
                && turn_spans.reply == Some(speak)
            {
                turn_spans.reply = None;
            }
            self.close_turn_if_done(turn);
        }
    }

    /// A job was accepted, under the turn that dispatched it when that turn's span is open.
    pub(crate) fn open_job(&mut self, job: JobId, turn: TurnId, kind: JobKind, route: Route) {
        let parent = self.turn_id(turn);
        let span = tracing::info_span!(
            parent: parent,
            names::JOB,
            job_id = %job,
            kind = ?kind,
            route = ?route,
            origin_turn = %turn,
        );
        self.jobs.insert(job, JobSpans { session: None, job: span });
    }

    /// The job's worker session started.
    pub(crate) fn open_session(&mut self, job: JobId, route: Route) {
        if let Some(spans) = self.jobs.get_mut(&job) {
            let parent = spans.job.id();
            spans.session =
                Some(tracing::info_span!(parent: parent, names::WORKER_SESSION, route = ?route));
        }
    }

    /// The job reached its terminal state.
    pub(crate) fn close_job(&mut self, job: JobId) {
        if let Some(spans) = self.jobs.remove(&job) {
            let JobSpans { session, job: job_span } = spans;
            drop(session);
            drop(job_span);
        }
    }
}

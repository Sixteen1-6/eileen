//! Eileen speaking (BUILD.md sections 2.2 and 2.8): starting an utterance, carrying out what the
//! turn-taking machine asks for, the conversation window that opens when she stops, mute, and
//! the announcements that wait for a quiet moment. What she cannot say aloud, while muted or
//! while speech is down, goes to the HUD's line instead (section 6.3).

use eileen_contracts::bus::{BusEvent, SpeakCause};
use eileen_contracts::core_io::Effect;
use eileen_contracts::ids::{JobId, SpeakId, TraceCtx, TurnId};

use crate::addressee::content_words;
use crate::announce::Due;
use crate::clauses::Cutter;
use crate::spans::SpeakParent;
use crate::turn::{Out, Verdict};
use crate::{Core, Current, Cx, LastSpoken, TimerKind};

/// Nanoseconds in a millisecond.
const NS_PER_MS: u64 = 1_000_000;

/// What an utterance belongs to: a turn it answers, or a job it announces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Owner {
    turn: Option<TurnId>,
    job: Option<JobId>,
}

impl Owner {
    /// A reply to `turn`.
    pub(crate) const fn turn(turn: TurnId) -> Self {
        Self { turn: Some(turn), job: None }
    }

    /// An announcement about `job`.
    pub(crate) const fn job(job: JobId) -> Self {
        Self { turn: None, job: Some(job) }
    }
}

/// `text` cut into clauses the way a brain reply is.
pub(crate) fn clauses_of(text: &str) -> Vec<String> {
    let mut cutter = Cutter::default();
    let mut clauses = cutter.push(text);
    clauses.extend(cutter.finish());
    clauses
}

/// An ended utterance, as the turn-taking machine reports it.
struct Ended {
    speak: SpeakId,
    text: String,
    heard_chars: u32,
    interrupted: bool,
    resumed: bool,
}

impl Core {
    /// Says `text` whole: it is cut into clauses and closed at once. While Eileen cannot speak
    /// (muted, or speech is down) it goes to the HUD instead; nothing is said while another
    /// utterance holds the floor.
    pub(crate) fn say(
        &mut self,
        cx: &mut Cx<'_>,
        owner: Owner,
        cause: SpeakCause,
        text: &str,
    ) -> Option<SpeakId> {
        if !self.can_speak() {
            self.hud.show(owner, text);
            return None;
        }
        let speak = self.start_speaking(cx, owner, cause, clauses_of(text))?;
        let mut out = Vec::new();
        self.turns.close(speak, &mut out);
        self.apply(cx, out);
        Some(speak)
    }

    /// Starts an utterance of `clauses`, left open for more.
    pub(crate) fn start_speaking(
        &mut self,
        cx: &mut Cx<'_>,
        owner: Owner,
        cause: SpeakCause,
        clauses: Vec<String>,
    ) -> Option<SpeakId> {
        let speak = SpeakId::from_parts(cx.ms(), cx.random());
        let chars: usize = clauses.iter().map(|clause| clause.chars().count()).sum();
        let voice = self.voice_for(cause);
        let mut out = Vec::new();
        if !self.turns.speak(speak, cause, &voice, clauses, &mut out) {
            return None;
        }
        self.hud.clear();
        self.current = Some(Current { speak, turn: owner.turn, job: owner.job });
        let parent = owner.turn.map_or(SpeakParent::Root, SpeakParent::Turn);
        self.spans.open_speak(speak, parent, cause, chars);
        self.apply(cx, out);
        Some(speak)
    }

    /// The voice for an utterance: the announcement voice for announcements, else the live
    /// voice; empty for the default voice in `models.toml`.
    pub(crate) fn voice_for(&self, cause: SpeakCause) -> String {
        let voice = &self.config.config.voice;
        let chosen = match cause {
            SpeakCause::Announcement => voice.announcements.as_ref().or(voice.live.as_ref()),
            SpeakCause::Narration => voice.narration.as_ref().or(voice.live.as_ref()),
            SpeakCause::Reply
            | SpeakCause::Approval
            | SpeakCause::Clarification
            | SpeakCause::Error
            | SpeakCause::FirstRun
            | SpeakCause::SessionSay => voice.live.as_ref(),
        };
        chosen.cloned().unwrap_or_default()
    }

    /// What the utterance `speak` belongs to, while it holds the floor.
    fn owner_of(&self, speak: SpeakId) -> Owner {
        self.current
            .filter(|current| current.speak == speak)
            .map_or_else(Owner::default, |current| Owner { turn: current.turn, job: current.job })
    }

    /// The correlation of the utterance holding the floor.
    fn current_ctx(&self) -> TraceCtx {
        self.current.map_or_else(TraceCtx::default, |current| TraceCtx {
            turn: current.turn,
            job: current.job,
            call: None,
        })
    }

    /// Carries out what the turn-taking machine asked for, in order.
    pub(crate) fn apply(&mut self, cx: &mut Cx<'_>, outs: Vec<Out>) {
        for out in outs {
            let ctx = self.current_ctx();
            match out {
                Out::Audio(cmd) => cx.push(ctx, Effect::Audio(cmd)),
                Out::Speech(cmd) => cx.push(ctx, Effect::Speech(cmd)),
                Out::Started { speak, cause, text } => {
                    cx.emit(ctx, BusEvent::Speaking { speak, cause, text });
                }
                Out::Stopped { speak, text, heard_chars, interrupted, resumed } => {
                    self.spoke(cx, Ended { speak, text, heard_chars, interrupted, resumed });
                }
                Out::Unspoken { speak, text } => {
                    self.hud.show(self.owner_of(speak), &text);
                    self.brain_reply_on_hud(speak);
                }
                Out::BargeIn { speak } => self.spans.open_barge_in(speak),
                Out::BargeInEnded { speak } => self.spans.close_barge_in(speak),
                Out::DecideBy(at) => self.timers.set(cx, TimerKind::BargeIn, at),
                Out::Muted(muted) => cx.emit(TraceCtx::default(), BusEvent::MuteChanged { muted }),
            }
        }
    }

    /// An utterance ended: `SpeakingStopped`, its spans close, and the conversation window
    /// opens.
    fn spoke(&mut self, cx: &mut Cx<'_>, ended: Ended) {
        let ctx = self.current_ctx();
        let Ended { speak, text, heard_chars, interrupted, resumed } = ended;
        cx.emit(ctx, BusEvent::SpeakingStopped { speak, heard_chars, interrupted, resumed });
        self.spans.close_speak(speak, heard_chars, interrupted);
        let job = self.current.filter(|current| current.speak == speak).and_then(|c| c.job);
        if self.current.is_some_and(|current| current.speak == speak) {
            self.current = None;
        }
        self.last_spoken = Some(LastSpoken { at: cx.now, words: content_words(&text), job });
        let conversation = self.config.config.conversation;
        let until = self.window.spoke(cx.now, conversation.follow_up_s, conversation.window_s);
        let until_ms =
            u32::try_from(until.0.saturating_sub(cx.now.0) / NS_PER_MS).unwrap_or(u32::MAX);
        cx.emit(TraceCtx::default(), BusEvent::ConversationWindow { open: true, until_ms });
        self.timers.set(cx, TimerKind::Window, until);
    }

    /// The conversation window's timer fired.
    pub(crate) fn window_deadline(&mut self, cx: &mut Cx<'_>) {
        if self.window.expire(cx.now) {
            cx.emit(TraceCtx::default(), BusEvent::ConversationWindow { open: false, until_ms: 0 });
        }
    }

    /// Stops whatever Eileen is saying; `interrupted` keeps its heard prefix and its rest.
    pub(crate) fn stop_speaking(&mut self, cx: &mut Cx<'_>, interrupted: bool) {
        let mut out = Vec::new();
        self.turns.stop(interrupted, &mut out);
        self.apply(cx, out);
    }

    /// Applies a verdict on a barge-in.
    pub(crate) fn resolve_barge_in(&mut self, cx: &mut Cx<'_>, verdict: Verdict) {
        let mut out = Vec::new();
        self.turns.resolve_barge_in(verdict, &mut out);
        self.apply(cx, out);
    }

    /// Mutes or unmutes: capture stops or starts, an earcon plays at the change, and nothing is
    /// spoken while muted. What the user had not heard of an utterance muting stopped goes to
    /// the HUD, and when it was the reply in flight, the rest of the reply follows it there.
    pub(crate) fn set_mute(&mut self, cx: &mut Cx<'_>, muted: bool) {
        let stopped = self.turns.speaking().filter(|_| muted);
        let owner = stopped.map_or_else(Owner::default, |speak| self.owner_of(speak));
        let mut out = Vec::new();
        let changed = self.turns.set_mute(muted, &mut out);
        if muted {
            self.partial = None;
        }
        self.apply(cx, out);
        if changed && let Some(speak) = stopped {
            // Text of it already on the HUD, lost when speech went down, follows what was not
            // heard.
            if let Some(rest) = self.turns.rest() {
                self.hud.lead(owner, rest);
            }
            self.brain_reply_on_hud(speak);
        }
    }

    /// After every step: speaks the next announcement once the conversation has been quiet for
    /// a second, or sets the timer that brings the step that will. Announcements wait while
    /// Eileen cannot speak, muted or with speech down.
    pub(crate) fn announce(&mut self, cx: &mut Cx<'_>) {
        let quiet = self.turns.quiet() && self.can_speak();
        self.announcements.note_quiet(quiet, cx.now);
        match self.announcements.due(cx.now) {
            Due::Never => {}
            Due::At(at) => self.timers.set(cx, TimerKind::Announce, at),
            Due::Now => {
                if let Some(next) = self.announcements.pop() {
                    self.say(cx, Owner::job(next.job), SpeakCause::Announcement, &next.text);
                }
            }
        }
    }
}

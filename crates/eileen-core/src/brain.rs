//! One brain turn at a time (BUILD.md sections 2.2 step 6 and 2.3): the Routed earcon at once,
//! one `BrainCmd::Turn` to the primary slot with the transcript and any heard prefix, the
//! streamed text cut into clauses and spoken as one utterance, and `TurnDone`. Text is accepted
//! only for the turn in flight and from the instance it was sent to. A reply Eileen cannot say
//! aloud, because the microphone is muted, speech is down or the speech process made no clause
//! of it, is shown on the HUD from there to its end (section 6.3).
//!
//! The core keeps the authority it sent each turn with: a job starts only from the turn in
//! flight and with that authority, never with what a tool call claims (section 2.3). A turn that
//! replaces one still unanswered holds both utterances with the weaker of their authorities, so
//! an `eileen say` request never rides on a later spoken turn's authority (section 2.12) and
//! neither request is dropped.

use eileen_contracts::bus::SpeakCause;
use eileen_contracts::config::{Humor, Verbosity};
use eileen_contracts::core_io::{AudioCmd, Earcon, Effect};
use eileen_contracts::harness::{BrainCmd, BrainEvent, BrainSlot};
use eileen_contracts::ids::{SpeakId, TurnId};
use eileen_contracts::jobs::TurnAuthority;

use crate::clauses::Cutter;
use crate::speech::Owner;
use crate::{Core, Cx, turn_ctx};

/// What Eileen says when a brain turn fails before she said anything.
const NO_ANSWER: &str = "Sorry, I could not get an answer to that.";

/// The brain as the core sees it.
#[derive(Debug, Default)]
pub(crate) struct Brain {
    /// The primary instance, from its `Ready`.
    instance: Option<u32>,
    live: Option<Live>,
}

/// The brain turn in flight.
#[derive(Debug)]
pub(crate) struct Live {
    turn: TurnId,
    /// The instance the turn was sent to; the first to answer when none was ready.
    instance: Option<u32>,
    /// The authority the core sent the turn with.
    authority: TurnAuthority,
    /// The user's words it holds, for a turn that replaces it before it answered.
    transcript: String,
    cutter: Cutter,
    /// The reply's utterance, once it started.
    speak: Option<SpeakId>,
    /// The reply was stopped: nothing more of it is spoken or shown.
    silenced: bool,
    /// The reply goes to the HUD's line from here to its end.
    on_hud: bool,
}

impl Brain {
    /// The turn in flight and the authority the core sent it with.
    pub(crate) fn live_turn(&self) -> Option<(TurnId, TurnAuthority)> {
        self.live.as_ref().map(|live| (live.turn, live.authority))
    }
}

impl Core {
    /// Sends `turn` to the primary brain. A turn still in flight is interrupted, and when it
    /// had not answered yet the new turn holds both utterances, with the weaker authority.
    pub(crate) fn send_brain_turn(
        &mut self,
        cx: &mut Cx<'_>,
        turn: TurnId,
        transcript: &str,
        authority: TurnAuthority,
    ) {
        let unanswered = self.interrupt_brain(cx).filter(|old| old.speak.is_none());
        // `Ambient` orders after `Addressed`, so the larger is the weaker.
        let (words, authority) = match unanswered {
            Some(old) => (format!("{} {transcript}", old.transcript), old.authority.max(authority)),
            None => (transcript.to_owned(), authority),
        };
        let heard = self.turns.take_heard();
        let message = self.message(&words, heard.as_deref());
        if !self.turns.muted() {
            cx.push(turn_ctx(turn), Effect::Audio(AudioCmd::Earcon(Earcon::Routed)));
        }
        let turn_cmd = BrainCmd::Turn { slot: BrainSlot::Primary, turn, message, authority };
        cx.push(turn_ctx(turn), Effect::Brain(turn_cmd));
        self.turns.think(turn);
        self.spans.open_brain(turn);
        self.brain.live = Some(Live {
            turn,
            instance: self.brain.instance,
            authority,
            transcript: words,
            cutter: Cutter::default(),
            speak: None,
            silenced: false,
            on_hud: false,
        });
    }

    /// Interrupts the brain turn in flight, if any, and returns it.
    pub(crate) fn interrupt_brain(&mut self, cx: &mut Cx<'_>) -> Option<Live> {
        let old = self.brain.live.take()?;
        let interrupt = BrainCmd::Interrupt { slot: BrainSlot::Primary };
        cx.push(turn_ctx(old.turn), Effect::Brain(interrupt));
        self.turns.thought(old.turn);
        self.spans.close_brain(old.turn);
        Some(old)
    }

    /// The turn's message: the `[assistant]` block from settings, then `[heard]` with the
    /// transcript and, after a barge-in, the exact prefix the user heard.
    fn message(&self, transcript: &str, heard: Option<&str>) -> String {
        let assistant = &self.config.config.assistant;
        let mut first = format!(
            "[assistant] name {}; verbosity {}; humor {}",
            assistant.name,
            verbosity_word(assistant.verbosity),
            humor_word(assistant.humor)
        );
        if !assistant.personality.is_empty() {
            first.push_str("; personality \"");
            first.push_str(&assistant.personality);
            first.push('"');
        }
        let mut lines = vec![first, format!("[heard] {transcript}")];
        if let Some(heard) = heard {
            lines.push(format!("        [interrupted] they heard: \"{heard}\""));
        }
        lines.join("\n")
    }

    /// From the brain driver.
    pub(crate) fn brain_event(&mut self, cx: &mut Cx<'_>, event: BrainEvent) {
        match event {
            BrainEvent::Ready { which, instance, .. } => {
                if which == BrainSlot::Primary {
                    self.brain.instance = Some(instance);
                }
            }
            BrainEvent::TextDelta { instance, turn, text } => {
                if self.accepts(instance, turn) {
                    let clauses = self.brain.live.as_mut().map(|live| live.cutter.push(text.get()));
                    self.reply(cx, clauses.unwrap_or_default(), false);
                }
            }
            BrainEvent::TurnDone { instance, turn, is_error, .. } => {
                if self.accepts(instance, turn) {
                    self.brain_done(cx, turn, is_error);
                }
            }
            BrainEvent::Exited { which, instance, .. } => {
                if which == BrainSlot::Primary && self.brain.instance == Some(instance) {
                    self.brain.instance = None;
                }
                let turn = self.brain.live.as_ref().map(|live| live.turn);
                if let Some(turn) = turn
                    && self.accepts(instance, turn)
                {
                    self.brain_done(cx, turn, true);
                }
            }
            BrainEvent::RateLimit(_) => {}
        }
    }

    /// Whether text for `turn` from `instance` belongs to the turn in flight.
    fn accepts(&mut self, instance: u32, turn: TurnId) -> bool {
        let Some(live) = self.brain.live.as_mut().filter(|live| live.turn == turn) else {
            return false;
        };
        if let Some(sent) = live.instance {
            return sent == instance;
        }
        live.instance = Some(instance);
        true
    }

    /// The turn ended: the last clause is spoken and the reply closed.
    fn brain_done(&mut self, cx: &mut Cx<'_>, turn: TurnId, is_error: bool) {
        let rest = self.brain.live.as_mut().and_then(|live| live.cutter.finish());
        let answered =
            self.brain.live.as_ref().is_some_and(|live| live.speak.is_some() || live.on_hud);
        let mut clauses: Vec<String> = rest.into_iter().collect();
        if is_error && !answered && clauses.is_empty() {
            clauses.push(NO_ANSWER.to_owned());
        }
        self.reply(cx, clauses, true);
        self.brain.live = None;
        self.turns.thought(turn);
        self.spans.close_brain(turn);
    }

    /// Speaks clauses of the reply: the first starts its utterance, the rest are added to it.
    /// While Eileen cannot speak they go to the HUD's line instead: a new line at the first, and
    /// added to it after.
    fn reply(&mut self, cx: &mut Cx<'_>, clauses: Vec<String>, close: bool) {
        let can_speak = self.can_speak();
        let Some(live) = self.brain.live.as_mut() else { return };
        let (turn, speak) = (live.turn, live.speak);
        if live.silenced {
            return;
        }
        if live.on_hud || !can_speak {
            let text = clauses.join(" ");
            if text.trim().is_empty() {
                return;
            }
            if live.on_hud {
                self.hud.append(&text);
            } else {
                live.on_hud = true;
                self.hud.show(&text);
            }
            return;
        }
        match speak {
            Some(speak) if self.turns.speaking() == Some(speak) => {
                let voice = self.voice_for(SpeakCause::Reply);
                let mut out = Vec::new();
                self.turns.append(speak, &voice, clauses, &mut out);
                if close {
                    self.turns.close(speak, &mut out);
                }
                self.apply(cx, out);
            }
            Some(_) => self.silence_reply(),
            None => {
                if clauses.is_empty() {
                    return;
                }
                self.stop_speaking(cx, false);
                let started =
                    self.start_speaking(cx, Owner::turn(turn), SpeakCause::Reply, clauses);
                let Some(speak) = started else {
                    self.silence_reply();
                    return;
                };
                if let Some(live) = self.brain.live.as_mut() {
                    live.speak = Some(speak);
                }
                if close {
                    let mut out = Vec::new();
                    self.turns.close(speak, &mut out);
                    self.apply(cx, out);
                }
            }
        }
    }

    const fn silence_reply(&mut self) {
        if let Some(live) = self.brain.live.as_mut() {
            live.silenced = true;
        }
    }

    /// The utterance `speak` will not be heard to its end, and its text is on the HUD: when it
    /// is the reply in flight, the rest of the reply follows it there.
    pub(crate) fn brain_reply_on_hud(&mut self, speak: SpeakId) {
        if let Some(live) = self.brain.live.as_mut().filter(|live| live.speak == Some(speak)) {
            live.on_hud = true;
        }
    }
}

const fn verbosity_word(verbosity: Verbosity) -> &'static str {
    match verbosity {
        Verbosity::Brief => "brief",
        Verbosity::Normal => "normal",
        Verbosity::Detailed => "detailed",
    }
}

const fn humor_word(humor: Humor) -> &'static str {
    match humor {
        Humor::Off => "off",
        Humor::Light => "light",
        Humor::Playful => "playful",
    }
}

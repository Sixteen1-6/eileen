//! The audio ring (BUILD.md section 6.6, decision D81), as the simulated speech process sees it.
//! The audio component keeps the captured frames the speech process has not confirmed, from the
//! start of the turn open there, up to 180 s, and writes them to the process once it is ready
//! again, flagged replayed. The process recognises them afresh, under turns it mints, and marks
//! each turn whose speech began in a replayed frame `SpeechStarted.replayed`, so the core hears
//! it late (section 2.8).
//!
//! Here the frames are the user's turn events (a start, partials, the end of speech and the end
//! of the turn), held while the process is not ready: loading at the start, loading again after
//! a restart, hung, or down before its restart. Each turn heard then is renamed to a turn of its
//! own, and the new name sticks to the rest of the turn heard after the process is back, so one
//! utterance stays one turn. A turn the process was hearing when it went down is renamed then:
//! the core drops the open turn, and its audio from its start is in the ring, so it comes back
//! as a turn heard late even when nothing more of it was held. A mute empties the ring, and
//! nothing is held while capture is off.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use eileen_contracts::ids::TurnId;
use eileen_contracts::speech::VoiceEvent;

/// How long the ring keeps audio, ms (section 6.6): the 120 s load deadline and the longest
/// backoff step fit in it.
pub(crate) const RING_MS: u64 = 180_000;

/// The most turn events the ring holds; the oldest go first. A turn's events come a few a
/// second at most, so this is far more than 180 s of speech.
const HELD_MAX: usize = 4_096;

/// The most turns renamed at once: the user speaks one turn at a time.
const RENAMED_MAX: usize = 8;

/// The source number of the microphone, for a start the ring makes up.
const MICROPHONE: u16 = 0;

/// The tag in the random part of every turn the process mints for the ring, above the count:
/// the turns are the same in every run, and never equal one the generator mints from its random
/// stream or a test mints with a small number.
const MINTED_TAG: u128 = 0xA0D1 << 64;

/// The turn a user turn event belongs to; `None` for every other voice event.
pub(crate) const fn turn_of(event: &VoiceEvent) -> Option<TurnId> {
    match event {
        VoiceEvent::SpeechStarted { turn, .. }
        | VoiceEvent::Partial { turn, .. }
        | VoiceEvent::SpeechEnded { turn }
        | VoiceEvent::EndOfTurn { turn, .. } => Some(*turn),
        VoiceEvent::Calibration(_)
        | VoiceEvent::BargeInWindow { .. }
        | VoiceEvent::ClauseReady { .. }
        | VoiceEvent::SpeakDone { .. }
        | VoiceEvent::Embedding { .. }
        | VoiceEvent::CheckResult { .. }
        | VoiceEvent::SpeakerMatch { .. }
        | VoiceEvent::Health { .. } => None,
    }
}

/// `event` moved to `turn`; any other voice event as it is.
fn renamed(event: VoiceEvent, turn: TurnId) -> VoiceEvent {
    match event {
        VoiceEvent::SpeechStarted { source, replayed, .. } => {
            VoiceEvent::SpeechStarted { turn, source, replayed }
        }
        VoiceEvent::Partial { text, stable, .. } => VoiceEvent::Partial { turn, text, stable },
        VoiceEvent::SpeechEnded { .. } => VoiceEvent::SpeechEnded { turn },
        VoiceEvent::EndOfTurn {
            text,
            smart_turn_pct,
            silence_ms,
            asr_confidence_pct,
            reason,
            source,
            level_db,
            ..
        } => VoiceEvent::EndOfTurn {
            turn,
            text,
            smart_turn_pct,
            silence_ms,
            asr_confidence_pct,
            reason,
            source,
            level_db,
        },
        other @ (VoiceEvent::Calibration(_)
        | VoiceEvent::BargeInWindow { .. }
        | VoiceEvent::ClauseReady { .. }
        | VoiceEvent::SpeakDone { .. }
        | VoiceEvent::Embedding { .. }
        | VoiceEvent::CheckResult { .. }
        | VoiceEvent::SpeakerMatch { .. }
        | VoiceEvent::Health { .. }) => other,
    }
}

/// The user's speech the process has not heard yet, and the turns it renamed.
#[derive(Debug, Default)]
pub(crate) struct Ring {
    /// Turn events held while the process was not ready, already renamed, with when each was
    /// spoken, oldest first.
    held: VecDeque<(u64, VoiceEvent)>,
    /// Each turn heard while the process was not ready, or open when it went down, and the turn
    /// the process recognises it as, until the turn ends.
    names: BTreeMap<TurnId, TurnId>,
    /// The turn the user is speaking, live or held, under the name they began it with.
    open: Option<TurnId>,
    /// Turns minted so far.
    minted: u64,
}

impl Ring {
    /// The process is ready and hears `event` live: a turn the ring renamed keeps its new name
    /// until it ends.
    pub(crate) fn live(&mut self, event: VoiceEvent) -> VoiceEvent {
        let Some(turn) = turn_of(&event) else { return event };
        let event = match self.names.get(&turn) {
            Some(name) => renamed(event, *name),
            None => event,
        };
        self.heard(turn, &event);
        event
    }

    /// The process is not ready: `event`, spoken at `now_ms`, is held under its turn's new name.
    pub(crate) fn hold(&mut self, now_ms: u64, event: VoiceEvent) {
        let Some(turn) = turn_of(&event) else { return };
        let name = match self.names.get(&turn) {
            Some(name) => *name,
            None => self.rename(turn, now_ms),
        };
        let event = renamed(event, name);
        self.heard(turn, &event);
        self.held.push_back((now_ms, event));
        self.trim(now_ms);
    }

    /// The process went down at `now_ms` after it was ready. The core drops the turn it was
    /// hearing, whose audio from its start is in the ring, so that turn is renamed now; every
    /// turn renamed before was heard under its new name, so its rest, if any, is a new turn too.
    pub(crate) fn process_lost(&mut self, now_ms: u64) {
        self.names.clear();
        if let Some(turn) = self.open {
            self.rename(turn, now_ms);
        }
    }

    /// A mute: the ring is emptied, and no turn stays renamed or open.
    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.names.clear();
        self.open = None;
    }

    /// The process is ready again at `now_ms`: what the ring held, as the process recognises it,
    /// in the order it was spoken. Every turn begins with a start flagged replayed, made up when
    /// its own start was heard live or has left the ring, and a renamed turn with nothing held
    /// gets one too, since its rest will be heard live under the new name.
    pub(crate) fn replay(&mut self, now_ms: u64) -> Vec<VoiceEvent> {
        self.trim(now_ms);
        let mut started = BTreeSet::new();
        let mut out = Vec::new();
        for (_, event) in self.held.drain(..) {
            let Some(turn) = turn_of(&event) else { continue };
            if started.insert(turn) {
                let source = if let VoiceEvent::SpeechStarted { source, .. }
                | VoiceEvent::EndOfTurn { source, .. } = &event
                {
                    *source
                } else {
                    MICROPHONE
                };
                out.push(VoiceEvent::SpeechStarted { turn, source, replayed: true });
            }
            if !matches!(event, VoiceEvent::SpeechStarted { .. }) {
                out.push(event);
            }
        }
        for name in self.names.values() {
            if started.insert(*name) {
                out.push(VoiceEvent::SpeechStarted {
                    turn: *name,
                    source: MICROPHONE,
                    replayed: true,
                });
            }
        }
        out
    }

    /// Mints a new name for `turn` at `now_ms`, forgetting the oldest renamed turn when full.
    fn rename(&mut self, turn: TurnId, now_ms: u64) -> TurnId {
        if self.names.len() >= RENAMED_MAX
            && let Some(oldest) = self.names.keys().next().copied()
        {
            self.names.remove(&oldest);
        }
        self.minted = self.minted.saturating_add(1);
        let name = TurnId::from_parts(now_ms, MINTED_TAG | u128::from(self.minted));
        self.names.insert(turn, name);
        name
    }

    /// The user went on with `turn` to `event`: its end lets its new name go and leaves no turn
    /// open.
    fn heard(&mut self, turn: TurnId, event: &VoiceEvent) {
        if matches!(event, VoiceEvent::EndOfTurn { .. }) {
            self.names.remove(&turn);
            self.open = None;
        } else {
            self.open = Some(turn);
        }
    }

    /// Drops what the ring no longer keeps at `now_ms`: audio older than [`RING_MS`], and the
    /// oldest past [`HELD_MAX`].
    fn trim(&mut self, now_ms: u64) {
        while self.held.front().is_some_and(|(at, _)| now_ms.saturating_sub(*at) > RING_MS) {
            self.held.pop_front();
        }
        while self.held.len() > HELD_MAX {
            self.held.pop_front();
        }
    }
}

#[cfg(test)]
mod tests;

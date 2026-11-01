//! Turn-taking (BUILD.md sections 2.2 and 2.8): who holds the floor, Eileen's utterance and
//! its playback accounting, barge-in with pause-then-decide, the heard prefix and mute.
//!
//! [`Turns`] is pure and cloneable: `Core::step` delegates every turn-taking input to it, and an
//! exhaustive test can clone a state to share each prefix of the sequences it enumerates. It
//! decides nothing about meaning. Whether an utterance was addressed, a backchannel or "stop" is
//! the core's call, handed back through [`Turns::resolve_barge_in`] and [`Turns::stop`]; what
//! the machine wants done comes out as [`Out`] values the core turns into effects.
//!
//! One utterance holds the floor at a time. Its clauses may arrive in several
//! `SpeechCmd::Speak` commands that name the same `SpeakId` (a brain reply is cut into clauses
//! as its text streams in), numbered on across them; [`Turns::close`] says no more will come.

use eileen_contracts::bus::SpeakCause;
use eileen_contracts::core_io::{AudioCmd, Earcon};
use eileen_contracts::ids::{MonoTime, SpeakId, TurnId};
use eileen_contracts::speech::SpeechCmd;

/// How far barge-in ducks playback at the first speech window, dB (section 2.8).
pub const DUCK_DB: i8 = -20;

/// Consecutive speech windows that pause playback: three 32 ms windows, 96 ms.
pub const PAUSE_WINDOWS: u8 = 3;

/// How long the core has to decide after a pause, and how long a duck that never paused lasts,
/// in nanoseconds of virtual time (600 ms, section 2.8 and invariant 2).
pub const DECIDE_NS: u64 = 600_000_000;

/// Who holds the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nobody: Eileen is silent, nothing is pending and the user is not speaking.
    Idle,
    /// The user is speaking a turn.
    Hearing,
    /// A brain turn is in flight and Eileen has not started its answer.
    Thinking,
    /// Eileen is speaking.
    Speaking,
}

/// The core's decision after a barge-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// A backchannel, or speech that was not addressed: resume from the paused sample.
    Resume,
    /// Stop playback and cancel synthesis.
    Stop,
}

/// What the machine asks the core to do, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// An audio command.
    Audio(AudioCmd),
    /// A speech command.
    Speech(SpeechCmd),
    /// An utterance began (the core emits `Speaking`).
    Started {
        /// The utterance.
        speak: SpeakId,
        /// Why Eileen speaks.
        cause: SpeakCause,
        /// Its first clauses.
        text: String,
    },
    /// An utterance ended (the core emits `SpeakingStopped`).
    Stopped {
        /// The utterance.
        speak: SpeakId,
        /// Its whole text so far.
        text: String,
        /// Characters the user heard.
        heard_chars: u32,
        /// Whether the user or a command cut it short.
        interrupted: bool,
        /// Whether it resumed after a barge-in.
        resumed: bool,
    },
    /// Text of the utterance that will not be heard, because the speech process made no
    /// clause of it or went down first: the core shows it on the HUD instead.
    Unspoken {
        /// The utterance.
        speak: SpeakId,
        /// The text.
        text: String,
    },
    /// A barge-in began: playback is ducked.
    BargeIn {
        /// The utterance.
        speak: SpeakId,
    },
    /// The barge-in ended, resumed or stopped.
    BargeInEnded {
        /// The utterance.
        speak: SpeakId,
    },
    /// Set the barge-in decision timer for this instant, replacing any earlier one.
    DecideBy(MonoTime),
    /// The microphone was muted or unmuted.
    Muted(bool),
}

/// One clause of an utterance.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Clause {
    text: String,
    /// Its samples, once the speech process reported it ready.
    samples: Option<u32>,
}

/// Where a barge-in stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Barge {
    None,
    /// Ducked; `run` consecutive windows so far, the last numbered `window`.
    Ducked {
        window: u8,
        run: u8,
    },
    /// Paused, waiting for the core's verdict.
    Paused,
}

/// The utterance holding the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Utterance {
    speak: SpeakId,
    clauses: Vec<Clause>,
    /// No more clauses will come.
    closed: bool,
    /// Every sample of the clauses ready so far was played.
    drained: bool,
    /// The clause playing and the samples of it handed to the device.
    clause: usize,
    samples: u32,
    resumed: bool,
    barge: Barge,
}

impl Utterance {
    fn text(&self) -> String {
        join(self.clauses.iter().map(|clause| clause.text.as_str()))
    }

    /// Whether every clause was reported ready.
    fn all_ready(&self) -> bool {
        self.clauses.iter().all(|clause| clause.samples.is_some())
    }

    /// The heard prefix (the clauses played in full, then the playing clause's characters in
    /// proportion to its samples played) and the rest, from the start of the cut word.
    fn split_heard(&self) -> (String, String) {
        let mut heard: Vec<String> = Vec::new();
        let mut rest: Vec<String> = Vec::new();
        for (index, clause) in self.clauses.iter().enumerate() {
            match index.cmp(&self.clause) {
                std::cmp::Ordering::Less => heard.push(clause.text.clone()),
                std::cmp::Ordering::Greater => rest.push(clause.text.clone()),
                std::cmp::Ordering::Equal => {
                    let chars: Vec<char> = clause.text.chars().collect();
                    let cut = heard_chars_of(chars.len(), self.samples, clause.samples);
                    heard.push(chars.iter().take(cut).collect());
                    let mut start = cut.min(chars.len());
                    while start > 0 && chars.get(start - 1).is_some_and(|c| !c.is_whitespace()) {
                        start -= 1;
                    }
                    rest.push(chars.iter().skip(start).collect());
                }
            }
        }
        (join(heard), join(rest))
    }
}

/// The characters of a clause of `chars` characters heard after `played` of its `total`
/// samples, in proportion to time.
fn heard_chars_of(chars: usize, played: u32, total: Option<u32>) -> usize {
    let Some(total) = total.filter(|total| *total > 0) else { return 0 };
    let chars = u64::try_from(chars).unwrap_or(u64::MAX);
    let heard = chars.saturating_mul(u64::from(played.min(total))) / u64::from(total);
    usize::try_from(heard).unwrap_or(usize::MAX)
}

/// Joins the non-empty pieces, trimmed, with single spaces.
fn join<S: AsRef<str>>(pieces: impl IntoIterator<Item = S>) -> String {
    let mut out = String::new();
    for piece in pieces {
        let piece = piece.as_ref().trim();
        if piece.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(piece);
    }
    out
}

/// Turn-taking state: pure, with no IO and no clock of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Turns {
    muted: bool,
    hearing: Option<TurnId>,
    thinking: Option<TurnId>,
    utterance: Option<Utterance>,
    /// What the user heard of the last interrupted utterance, for the next brain turn.
    heard: Option<String>,
    /// What they did not hear of it, for "go on".
    rest: Option<String>,
}

impl Turns {
    /// A machine with nobody speaking, muted or not.
    #[must_use]
    pub fn new(muted: bool) -> Self {
        Self { muted, ..Self::default() }
    }

    /// Who holds the floor.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        if self.utterance.is_some() {
            Phase::Speaking
        } else if self.thinking.is_some() {
            Phase::Thinking
        } else if self.hearing.is_some() {
            Phase::Hearing
        } else {
            Phase::Idle
        }
    }

    /// Whether the microphone is muted.
    #[must_use]
    pub const fn muted(&self) -> bool {
        self.muted
    }

    /// Whether the user and Eileen are both quiet and no brain turn is pending.
    #[must_use]
    pub const fn quiet(&self) -> bool {
        matches!(self.phase(), Phase::Idle)
    }

    /// The utterance holding the floor.
    #[must_use]
    pub fn speaking(&self) -> Option<SpeakId> {
        self.utterance.as_ref().map(|utterance| utterance.speak)
    }

    /// Whether a barge-in is under way (ducked or paused).
    #[must_use]
    pub fn barge_in(&self) -> bool {
        self.utterance.as_ref().is_some_and(|utterance| utterance.barge != Barge::None)
    }

    /// Whether playback is paused for a barge-in.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.utterance.as_ref().is_some_and(|utterance| utterance.barge == Barge::Paused)
    }

    /// The brain turn in flight, if any.
    #[must_use]
    pub const fn thinking(&self) -> Option<TurnId> {
        self.thinking
    }

    /// The user began speaking `turn`.
    pub const fn speech_started(&mut self, turn: TurnId) {
        if !self.muted {
            self.hearing = Some(turn);
        }
    }

    /// The user's turn ended; returns the turn that was being heard.
    pub const fn end_of_turn(&mut self) -> Option<TurnId> {
        self.hearing.take()
    }

    /// What the user did not hear of the last interrupted utterance, left for "go on".
    #[must_use]
    pub fn rest(&self) -> Option<&str> {
        self.rest.as_deref()
    }

    /// A brain turn is in flight.
    pub const fn think(&mut self, turn: TurnId) {
        self.thinking = Some(turn);
    }

    /// The brain turn `turn` ended.
    pub fn thought(&mut self, turn: TurnId) {
        if self.thinking == Some(turn) {
            self.thinking = None;
        }
    }

    /// Mutes or unmutes; muting stops Eileen mid-sentence. Returns whether anything changed.
    pub fn set_mute(&mut self, muted: bool, out: &mut Vec<Out>) -> bool {
        if self.muted == muted {
            return false;
        }
        if muted {
            self.stop(true, out);
            out.push(Out::Audio(AudioCmd::Earcon(Earcon::Muted)));
            out.push(Out::Audio(AudioCmd::SetCapture(false)));
            self.hearing = None;
        } else {
            out.push(Out::Audio(AudioCmd::SetCapture(true)));
            out.push(Out::Audio(AudioCmd::Earcon(Earcon::Unmuted)));
        }
        self.muted = muted;
        out.push(Out::Muted(muted));
        true
    }

    /// Plays an earcon, unless muted: nothing sounds while muted (invariant 9).
    pub fn earcon(&self, earcon: Earcon, out: &mut Vec<Out>) {
        if !self.muted {
            out.push(Out::Audio(AudioCmd::Earcon(earcon)));
        }
    }

    /// Starts an utterance. Refused (false) while muted, with no clauses, or while another
    /// utterance holds the floor: the core stops that one first.
    pub fn speak(
        &mut self,
        speak: SpeakId,
        cause: SpeakCause,
        voice: &str,
        clauses: Vec<String>,
        out: &mut Vec<Out>,
    ) -> bool {
        let clauses: Vec<String> = clauses.into_iter().filter(|c| !c.trim().is_empty()).collect();
        if self.muted || self.utterance.is_some() || clauses.is_empty() {
            return false;
        }
        let text = join(&clauses);
        self.heard = None;
        self.rest = None;
        self.utterance = Some(Utterance {
            speak,
            clauses: clauses
                .iter()
                .map(|clause| Clause { text: clause.clone(), samples: None })
                .collect(),
            closed: false,
            drained: false,
            clause: 0,
            samples: 0,
            resumed: false,
            barge: Barge::None,
        });
        out.push(Out::Started { speak, cause, text: text.clone() });
        out.push(Out::Speech(SpeechCmd::Speak {
            speak,
            voice: voice.to_owned(),
            clauses,
            whole_line: text,
        }));
        true
    }

    /// Adds clauses to the utterance `speak`, numbered on from its last.
    pub fn append(
        &mut self,
        speak: SpeakId,
        voice: &str,
        clauses: Vec<String>,
        out: &mut Vec<Out>,
    ) {
        let clauses: Vec<String> = clauses.into_iter().filter(|c| !c.trim().is_empty()).collect();
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak && !u.closed)
        else {
            return;
        };
        if clauses.is_empty() {
            return;
        }
        utterance
            .clauses
            .extend(clauses.iter().map(|clause| Clause { text: clause.clone(), samples: None }));
        utterance.drained = false;
        let whole_line = join(&clauses);
        out.push(Out::Speech(SpeechCmd::Speak {
            speak,
            voice: voice.to_owned(),
            clauses,
            whole_line,
        }));
    }

    /// No more clauses will come for `speak`; it ends once its last sample is played.
    pub fn close(&mut self, speak: SpeakId, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak) else {
            return;
        };
        utterance.closed = true;
        if utterance.drained && utterance.all_ready() {
            self.finish(out);
        }
    }

    /// The speech process synthesized clause `clause` of `speak`.
    pub fn clause_ready(&mut self, speak: SpeakId, clause: u16, samples: u32) {
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak) else {
            return;
        };
        if let Some(entry) = utterance.clauses.get_mut(usize::from(clause)) {
            entry.samples = Some(samples);
            utterance.drained = false;
        }
    }

    /// The device has been handed `samples` of clause `clause` of `speak` (a `Played`,
    /// `Paused` or `Stopped` report); the position never moves backwards.
    pub fn played(&mut self, speak: SpeakId, clause: u16, samples: u32) {
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak) else {
            return;
        };
        let clause = usize::from(clause);
        if clause >= utterance.clauses.len() {
            return;
        }
        let ahead = clause > utterance.clause
            || (clause == utterance.clause && samples > utterance.samples);
        if ahead {
            utterance.clause = clause;
            utterance.samples = samples;
        }
    }

    /// The device stopped `speak` on its own (the core's own stops have already ended it).
    pub fn device_stopped(
        &mut self,
        speak: SpeakId,
        clause: u16,
        samples: u32,
        out: &mut Vec<Out>,
    ) {
        if self.speaking() != Some(speak) {
            return;
        }
        self.played(speak, clause, samples);
        self.end(false, out);
    }

    /// Every sample of `speak` ready so far was played. It ends here when it is closed and
    /// every clause was ready; otherwise synthesis is behind and playback waits for more.
    pub fn drained(&mut self, speak: SpeakId, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak) else {
            return;
        };
        utterance.drained = true;
        if utterance.closed && utterance.all_ready() {
            self.finish(out);
        }
    }

    /// The speech process finished what `speak` was sent. An utterance it made no clause for
    /// (speech is down, section 6.6) ends here, unheard, rather than holding the floor, and its
    /// text goes to the HUD.
    pub fn speak_done(&mut self, speak: SpeakId, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.as_ref().filter(|utterance| {
            utterance.speak == speak && utterance.clauses.iter().all(|c| c.samples.is_none())
        }) else {
            return;
        };
        out.push(Out::Unspoken { speak, text: utterance.text() });
        self.end(false, out);
    }

    /// The speech process went down and will synthesize nothing more. The utterance keeps the
    /// clauses it had synthesized in order from the first, whose samples the audio driver holds
    /// and plays, and is closed; the text of the rest goes to the HUD. With no clause
    /// synthesized it ends here, unheard.
    pub fn speech_lost(&mut self, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.as_mut() else { return };
        let ready = utterance.clauses.iter().take_while(|c| c.samples.is_some()).count();
        let lost = utterance.clauses.split_off(ready);
        utterance.closed = true;
        let (speak, drained) = (utterance.speak, utterance.drained);
        if !lost.is_empty() {
            let text = join(lost.iter().map(|clause| clause.text.as_str()));
            out.push(Out::Unspoken { speak, text });
        }
        if ready == 0 {
            self.end(false, out);
        } else if drained {
            self.finish(out);
        }
    }

    /// The device stopped taking samples: the utterance is stopped rather than left holding
    /// the floor.
    pub fn stalled(&mut self, out: &mut Vec<Out>) {
        self.stop(false, out);
    }

    /// One barge-in window while `speak` plays: duck at the first, pause at the third
    /// consecutive one.
    pub fn barge_in_window(
        &mut self,
        now: MonoTime,
        speak: SpeakId,
        window: u8,
        out: &mut Vec<Out>,
    ) {
        let Some(utterance) = self.utterance.as_mut().filter(|u| u.speak == speak) else {
            return;
        };
        let deadline = MonoTime(now.0.saturating_add(DECIDE_NS));
        match utterance.barge {
            Barge::None => {
                utterance.barge = Barge::Ducked { window, run: 1 };
                out.push(Out::BargeIn { speak });
                out.push(Out::Audio(AudioCmd::Duck { db: DUCK_DB }));
                out.push(Out::DecideBy(deadline));
            }
            Barge::Ducked { window: last, run } => {
                let run = if window == last.wrapping_add(1) {
                    run.saturating_add(1)
                } else if window == last {
                    run
                } else {
                    1
                };
                if run >= PAUSE_WINDOWS {
                    utterance.barge = Barge::Paused;
                    out.push(Out::Audio(AudioCmd::Pause { speak }));
                    out.push(Out::DecideBy(deadline));
                } else {
                    utterance.barge = Barge::Ducked { window, run };
                }
            }
            Barge::Paused => {}
        }
    }

    /// The decision timer fired. A duck that never paused is undone here (false); a pause
    /// needs the core's verdict now (true).
    pub fn barge_in_deadline(&mut self, out: &mut Vec<Out>) -> bool {
        let Some(utterance) = self.utterance.as_mut() else { return false };
        match utterance.barge {
            Barge::None => false,
            Barge::Ducked { .. } => {
                utterance.barge = Barge::None;
                out.push(Out::Audio(AudioCmd::Unduck));
                out.push(Out::BargeInEnded { speak: utterance.speak });
                false
            }
            Barge::Paused => true,
        }
    }

    /// Applies the core's verdict on a barge-in.
    pub fn resolve_barge_in(&mut self, verdict: Verdict, out: &mut Vec<Out>) {
        match verdict {
            Verdict::Stop => self.stop(true, out),
            Verdict::Resume => {
                let Some(utterance) = self.utterance.as_mut() else { return };
                let speak = utterance.speak;
                match utterance.barge {
                    Barge::None => return,
                    Barge::Paused => {
                        utterance.resumed = true;
                        out.push(Out::Audio(AudioCmd::Resume { speak }));
                        out.push(Out::Speech(SpeechCmd::Resume { speak }));
                    }
                    Barge::Ducked { .. } => {}
                }
                utterance.barge = Barge::None;
                out.push(Out::Audio(AudioCmd::Unduck));
                out.push(Out::BargeInEnded { speak });
            }
        }
    }

    /// Stops the utterance holding the floor, if any: playback stops and synthesis is
    /// cancelled. An interrupted utterance leaves its heard prefix for the next brain turn and
    /// its rest for "go on".
    pub fn stop(&mut self, interrupted: bool, out: &mut Vec<Out>) {
        let Some(speak) = self.speaking() else { return };
        out.push(Out::Audio(AudioCmd::Stop { speak }));
        out.push(Out::Speech(SpeechCmd::Cancel { speak }));
        self.end(interrupted, out);
    }

    /// Ends the utterance at its current position.
    fn end(&mut self, interrupted: bool, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.take() else { return };
        if utterance.barge != Barge::None {
            out.push(Out::Audio(AudioCmd::Unduck));
            out.push(Out::BargeInEnded { speak: utterance.speak });
        }
        let (heard, rest) = utterance.split_heard();
        out.push(Out::Stopped {
            speak: utterance.speak,
            text: utterance.text(),
            heard_chars: u32::try_from(heard.chars().count()).unwrap_or(u32::MAX),
            interrupted,
            resumed: utterance.resumed,
        });
        if interrupted {
            self.heard = Some(heard).filter(|heard| !heard.is_empty());
            self.rest = Some(rest).filter(|rest| !rest.is_empty());
        } else {
            self.heard = None;
            self.rest = None;
        }
    }

    /// Ends the utterance with every sample played.
    fn finish(&mut self, out: &mut Vec<Out>) {
        let Some(utterance) = self.utterance.take() else { return };
        if utterance.barge != Barge::None {
            out.push(Out::Audio(AudioCmd::Unduck));
            out.push(Out::BargeInEnded { speak: utterance.speak });
        }
        let text = utterance.text();
        out.push(Out::Stopped {
            speak: utterance.speak,
            heard_chars: u32::try_from(text.chars().count()).unwrap_or(u32::MAX),
            text,
            interrupted: false,
            resumed: utterance.resumed,
        });
        self.heard = None;
        self.rest = None;
    }

    /// What the user heard of the last interrupted utterance, once: the next brain turn is
    /// told it.
    pub const fn take_heard(&mut self) -> Option<String> {
        self.heard.take()
    }

    /// The part of the last interrupted utterance the user did not hear, once, for "go on".
    pub const fn take_rest(&mut self) -> Option<String> {
        self.rest.take()
    }
}

#[cfg(test)]
mod tests;

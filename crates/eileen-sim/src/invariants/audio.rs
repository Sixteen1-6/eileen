//! Invariants 2, 3, 9 and 10 over the audio path (BUILD.md section 4.7): what reached the core,
//! what it emitted, and each playback callback, in time order ([`AudioRecord`]).
//!
//! Each invariant is a watch that follows the records one at a time, so `turn::exhaustive`
//! ([`crate::exhaustive`]) can carry it in every state it enumerates and prune by state, and a
//! named function that runs the watch over a whole trace. A watch's `aged` copy replaces every
//! time it holds by its age, so two states that differ only in when they happened hash alike.

use std::cmp::Ordering;

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{ActivityEvent, AudioCmd, Earcon, Effect, Input};
use eileen_contracts::harness::BrainCmd;
use eileen_contracts::ids::SpeakId;
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};

use super::Violation;

/// A barge-in window's length, ms: the speech process judges speech over Eileen in 32 ms
/// windows (section 2.8).
pub const WINDOW_MS: u64 = 32;
/// The consecutive window by which playback is paused.
pub const PAUSE_BY_WINDOW: u8 = 3;
/// The longest from the onset of speech over Eileen to silence, ms, when the windows come at
/// the speech process's pace.
pub const ONSET_TO_SILENCE_MS: u64 = 150;
/// The longest a pause waits for its resume or its stop, ms.
pub const PAUSE_RESOLVES_MS: u64 = 600;
/// What comes before the heard prefix in a brain turn's message, up to the closing quote at
/// the end of its line. `eileen-core`'s brain module writes it; the simulator's barge-in tests
/// read it from the real core's message, so a change of wording fails there first.
pub const HEARD_MARKER: &str = "[interrupted] they heard: \"";

/// Samples of one clause handed to the device in one callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handed {
    /// The utterance.
    pub speak: SpeakId,
    /// Its clause.
    pub clause: u16,
    /// Samples of that clause the callback took.
    pub samples: u32,
}

/// One thing on the audio path.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    /// An input delivered to the core.
    Input(Input),
    /// An effect the core emitted.
    Effect(Effect),
    /// One playback callback and the samples it handed to the device, which may span a clause
    /// boundary.
    Callback(Vec<Handed>),
}

/// An [`AudioEvent`] at a virtual time.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioRecord {
    /// When, ms of virtual time.
    pub at_ms: u64,
    /// What.
    pub event: AudioEvent,
}

/// Invariant 2 over a trace that ends at `now_ms`: after a speech window while Eileen speaks,
/// playback is ducked by the next callback and paused by the callback after the third
/// consecutive window, and plays nothing while paused; onset to silence stays within [`ONSET_TO_SILENCE_MS`]; and a pause is
/// resumed or stopped within [`PAUSE_RESOLVES_MS`].
pub fn barge_in_pauses_in_time(records: &[AudioRecord], now_ms: u64) -> Result<(), Violation> {
    let mut watch = BargeInWatch::default();
    for record in records {
        watch.see(record.at_ms, &record.event)?;
    }
    watch.by(now_ms)
}

/// Invariant 3 over a trace: what the brain is told the user heard of an interrupted
/// utterance, and the `heard_chars` of its `SpeakingStopped`, equal the text mapped from the
/// samples handed to the device: the clauses played in full, then the playing clause's
/// characters in proportion to its samples played (section 2.8, "What was heard").
pub fn heard_prefix_matches_playback(records: &[AudioRecord]) -> Result<(), Violation> {
    let mut watch = HeardWatch::default();
    for record in records {
        watch.see(&record.event)?;
    }
    Ok(())
}

/// Invariant 9's mute and microphone parts over a trace: capture is off by the first callback
/// after a mute, and while muted or while another app uses the microphone no utterance starts,
/// nothing is sent to synthesis, no phrase or earcon plays (the mute and unmute earcons, which
/// confirm the user's own change, aside) and no sample reaches the device. Its call, cancel and
/// input-lease parts join once the core has calls, worker interrupts and leases.
pub fn nothing_sounds_while_muted(records: &[AudioRecord]) -> Result<(), Violation> {
    let mut watch = MuteWatch::default();
    for record in records {
        watch.see(record.at_ms, &record.event)?;
    }
    Ok(())
}

/// Where a barge-in stands, as invariant 2 follows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
enum Run {
    /// None under way.
    #[default]
    Quiet,
    /// Ducked after `count` consecutive windows, the last numbered `last` at `last_ms`; the
    /// run's speech began at `onset_ms`, and `paced` says each window came within
    /// [`WINDOW_MS`] of the one before.
    Ducked { last: u8, last_ms: u64, count: u8, onset_ms: u64, paced: bool },
    /// Paused at `at_ms`, waiting for the core's verdict.
    Paused { at_ms: u64 },
}

/// Invariant 2, record by record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BargeInWatch {
    /// The utterance holding the floor.
    floor: Option<SpeakId>,
    run: Run,
    /// When the window that needs a duck by the next callback came.
    duck_owed: Option<u64>,
    /// When the window that needs a pause by the next callback came.
    pause_owed: Option<u64>,
    /// After a pause or a stop owed to a run that came at pace: the run's onset, judged at the
    /// next callback.
    silence_due: Option<u64>,
}

impl BargeInWatch {
    const N: u8 = 2;
    const NAME: &'static str = "barge_in_pauses_in_time";

    fn fail(detail: String) -> Result<(), Violation> {
        Err(Violation::new(Self::N, Self::NAME, detail))
    }

    /// Follows one record at `at_ms`.
    pub fn see(&mut self, at_ms: u64, event: &AudioEvent) -> Result<(), Violation> {
        match event {
            AudioEvent::Effect(Effect::Emit(BusEvent::Speaking { speak, .. })) => {
                *self = Self { floor: Some(*speak), ..Self::default() };
            }
            AudioEvent::Effect(Effect::Emit(BusEvent::SpeakingStopped { speak, .. }))
                if self.floor == Some(*speak) =>
            {
                *self = Self::default();
            }
            AudioEvent::Input(Input::Voice(VoiceEvent::BargeInWindow {
                speak, window, ..
            })) if self.floor == Some(*speak) => {
                self.window(at_ms, *window);
            }
            AudioEvent::Effect(Effect::Audio(cmd)) => self.audio(at_ms, cmd),
            AudioEvent::Callback(handed) => return self.callback(at_ms, handed),
            AudioEvent::Input(_) | AudioEvent::Effect(_) => {}
        }
        Ok(())
    }

    /// The trace ends at `now_ms`: a pause older than [`PAUSE_RESOLVES_MS`] is a violation.
    pub fn by(&self, now_ms: u64) -> Result<(), Violation> {
        if let Run::Paused { at_ms } = self.run
            && now_ms.saturating_sub(at_ms) > PAUSE_RESOLVES_MS
        {
            return Self::fail(format!(
                "playback paused at {at_ms} ms was neither resumed nor stopped by {now_ms} ms"
            ));
        }
        Ok(())
    }

    /// A copy with every time replaced by its age at `now_ms`, for hashing states.
    #[must_use]
    pub fn aged(&self, now_ms: u64) -> Self {
        let age = |at: u64| now_ms.saturating_sub(at);
        let run = match self.run {
            Run::Quiet => Run::Quiet,
            Run::Ducked { last, last_ms, count, onset_ms, paced } => {
                Run::Ducked { last, last_ms: age(last_ms), count, onset_ms: age(onset_ms), paced }
            }
            Run::Paused { at_ms } => Run::Paused { at_ms: age(at_ms) },
        };
        Self {
            floor: self.floor,
            run,
            duck_owed: self.duck_owed.map(age),
            pause_owed: self.pause_owed.map(age),
            silence_due: self.silence_due.map(age),
        }
    }

    fn window(&mut self, at_ms: u64, window: u8) {
        let onset = at_ms.saturating_sub(WINDOW_MS);
        match self.run {
            Run::Quiet => {
                self.run = Run::Ducked {
                    last: window,
                    last_ms: at_ms,
                    count: 1,
                    onset_ms: onset,
                    paced: true,
                };
                self.duck_owed.get_or_insert(at_ms);
            }
            Run::Ducked { last, last_ms, count, onset_ms, paced } => {
                let (count, onset_ms, paced) = if window == last.wrapping_add(1) {
                    let on_time = at_ms.saturating_sub(last_ms) <= WINDOW_MS;
                    (count.saturating_add(1), onset_ms, paced && on_time)
                } else if window == last {
                    (count, onset_ms, paced)
                } else {
                    (1, onset, true)
                };
                self.run = Run::Ducked { last: window, last_ms: at_ms, count, onset_ms, paced };
                if count >= PAUSE_BY_WINDOW {
                    self.pause_owed.get_or_insert(at_ms);
                }
            }
            Run::Paused { .. } => {}
        }
    }

    /// The onset to judge at the next callback when a pause or stop ends a run that came at
    /// pace.
    const fn onset_at_pace(&self) -> Option<u64> {
        match self.run {
            Run::Ducked { onset_ms, paced: true, .. } => Some(onset_ms),
            Run::Ducked { paced: false, .. } | Run::Quiet | Run::Paused { .. } => None,
        }
    }

    fn audio(&mut self, at_ms: u64, cmd: &AudioCmd) {
        let floor = |speak: &SpeakId| self.floor == Some(*speak);
        match cmd {
            AudioCmd::Duck { .. } => self.duck_owed = None,
            AudioCmd::Pause { speak } if floor(speak) => {
                self.silence_due = self.onset_at_pace();
                self.pause_owed = None;
                self.run = Run::Paused { at_ms };
            }
            AudioCmd::Stop { speak } if floor(speak) => {
                if self.pause_owed.is_some() {
                    self.silence_due = self.onset_at_pace();
                }
                self.pause_owed = None;
                self.duck_owed = None;
                self.run = Run::Quiet;
            }
            AudioCmd::Resume { speak } if floor(speak) => {
                self.silence_due = None;
                self.run = Run::Quiet;
            }
            AudioCmd::Unduck => {
                if matches!(self.run, Run::Ducked { .. }) {
                    self.run = Run::Quiet;
                }
            }
            AudioCmd::Pause { .. }
            | AudioCmd::Stop { .. }
            | AudioCmd::Resume { .. }
            | AudioCmd::SetCapture(_)
            | AudioCmd::PlayPhrase { .. }
            | AudioCmd::Earcon(_)
            | AudioCmd::Record { .. } => {}
        }
    }

    fn callback(&mut self, at_ms: u64, handed: &[Handed]) -> Result<(), Violation> {
        if let Some(window) = self.duck_owed {
            return Self::fail(format!(
                "the speech window at {window} ms was not ducked by the callback at {at_ms} ms"
            ));
        }
        if let Some(window) = self.pause_owed {
            return Self::fail(format!(
                "the window at {window} ms was the {PAUSE_BY_WINDOW}rd in a row and playback \
                 was not paused by the callback at {at_ms} ms"
            ));
        }
        let playing = self
            .floor
            .filter(|floor| handed.iter().any(|part| part.speak == *floor && part.samples > 0));
        if let (Some(floor), Run::Paused { at_ms: paused }) = (playing, self.run) {
            return Self::fail(format!(
                "{floor} was paused at {paused} ms and still played at the callback at {at_ms} ms"
            ));
        }
        if let Some(onset) = self.silence_due.take() {
            let took = at_ms.saturating_sub(onset);
            if took > ONSET_TO_SILENCE_MS {
                return Self::fail(format!(
                    "speech began over Eileen at {onset} ms and she fell silent at {at_ms} ms, \
                     {took} ms later"
                ));
            }
        }
        Ok(())
    }
}

/// One clause of the utterance on the floor, as invariant 3 follows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Clause {
    text: String,
    /// Its samples, once the speech process reported it ready.
    samples: Option<u32>,
    /// Its samples handed to the device so far.
    handed: u32,
}

/// Invariant 3, record by record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct HeardWatch {
    floor: Option<SpeakId>,
    clauses: Vec<Clause>,
    /// What the next brain turn must be told the user heard, after an interruption.
    owed: Option<String>,
}

impl HeardWatch {
    const N: u8 = 3;
    const NAME: &'static str = "heard_prefix_matches_playback";

    fn fail(detail: String) -> Result<(), Violation> {
        Err(Violation::new(Self::N, Self::NAME, detail))
    }

    /// Follows one record.
    pub fn see(&mut self, event: &AudioEvent) -> Result<(), Violation> {
        match event {
            AudioEvent::Effect(Effect::Emit(BusEvent::Speaking { speak, .. })) => {
                *self = Self { floor: Some(*speak), ..Self::default() };
            }
            AudioEvent::Effect(Effect::Speech(SpeechCmd::Speak { speak, clauses, .. }))
                if self.floor == Some(*speak) =>
            {
                self.clauses.extend(clauses.iter().map(|text| Clause {
                    text: text.clone(),
                    samples: None,
                    handed: 0,
                }));
            }
            AudioEvent::Input(Input::Voice(VoiceEvent::ClauseReady {
                speak,
                clause,
                samples,
                ..
            })) if self.floor == Some(*speak) => {
                if let Some(entry) = self.clauses.get_mut(usize::from(*clause)) {
                    entry.samples = Some(*samples);
                }
            }
            AudioEvent::Callback(handed) => {
                for part in handed.iter().filter(|part| self.floor == Some(part.speak)) {
                    if let Some(entry) = self.clauses.get_mut(usize::from(part.clause)) {
                        entry.handed = entry.handed.saturating_add(part.samples);
                    }
                }
            }
            AudioEvent::Effect(Effect::Emit(BusEvent::SpeakingStopped {
                speak,
                heard_chars,
                interrupted,
                ..
            })) if self.floor == Some(*speak) => {
                let heard = self.heard();
                *self = Self::default();
                if *interrupted {
                    let count = heard.chars().count();
                    if usize::try_from(*heard_chars).ok() != Some(count) {
                        return Self::fail(format!(
                            "{speak} stopped with {heard_chars} characters heard, and the \
                             samples handed to the device carry {count}"
                        ));
                    }
                    self.owed = Some(heard).filter(|heard| !heard.is_empty());
                }
            }
            AudioEvent::Effect(Effect::Brain(BrainCmd::Turn { message, .. })) => {
                let told = told_heard(message);
                let owed = self.owed.take();
                if told != owed.as_deref() {
                    return Self::fail(format!(
                        "the brain was told the user heard {told:?}, and the samples handed to \
                         the device carry {owed:?}"
                    ));
                }
            }
            AudioEvent::Input(_) | AudioEvent::Effect(_) => {}
        }
        Ok(())
    }

    /// The text mapped from the samples handed so far: the clauses before the one playing in
    /// full, then the playing clause's characters in proportion to its samples.
    fn heard(&self) -> String {
        let playing = self.clauses.iter().rposition(|clause| clause.handed > 0).unwrap_or(0);
        let mut pieces: Vec<String> = Vec::new();
        for (index, clause) in self.clauses.iter().enumerate() {
            match index.cmp(&playing) {
                Ordering::Less => pieces.push(clause.text.clone()),
                Ordering::Equal => {
                    let chars = clause.text.chars().count();
                    let cut = in_proportion(chars, clause.handed, clause.samples);
                    pieces.push(clause.text.chars().take(cut).collect());
                }
                Ordering::Greater => break,
            }
        }
        join(&pieces)
    }
}

/// The heard prefix a brain turn's message carries, if any.
fn told_heard(message: &str) -> Option<&str> {
    let start = message.find(HEARD_MARKER)?.saturating_add(HEARD_MARKER.len());
    let line = message.get(start..)?.lines().next()?;
    let end = line.rfind('"')?;
    line.get(..end)
}

/// The characters of a clause of `chars` characters heard after `played` of its `total`
/// samples, in proportion to time.
fn in_proportion(chars: usize, played: u32, total: Option<u32>) -> usize {
    let Some(total) = total.filter(|total| *total > 0) else { return 0 };
    let chars = u64::try_from(chars).unwrap_or(u64::MAX);
    let heard = chars.saturating_mul(u64::from(played.min(total))) / u64::from(total);
    usize::try_from(heard).unwrap_or(usize::MAX)
}

/// The non-empty pieces, trimmed, joined with single spaces.
fn join(pieces: &[String]) -> String {
    let kept: Vec<&str> =
        pieces.iter().map(|piece| piece.trim()).filter(|piece| !piece.is_empty()).collect();
    kept.join(" ")
}

/// Invariant 9's mute and microphone parts, record by record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MuteWatch {
    muted: bool,
    /// Whether the microphone is captured; on until the core turns it off.
    capture: bool,
    /// Whether another app uses the microphone.
    elsewhere: bool,
}

impl Default for MuteWatch {
    fn default() -> Self {
        Self { muted: false, capture: true, elsewhere: false }
    }
}

impl MuteWatch {
    const N: u8 = 9;
    const NAME: &'static str = "nothing_sounds_while_muted";

    fn fail(&self, at_ms: u64, what: String) -> Result<(), Violation> {
        let why = if self.muted { "muted" } else { "another app used the microphone" };
        Err(Violation::new(Self::N, Self::NAME, format!("{what} at {at_ms} ms while {why}")))
    }

    /// Follows one record at `at_ms`.
    pub fn see(&mut self, at_ms: u64, event: &AudioEvent) -> Result<(), Violation> {
        let silent = self.muted || self.elsewhere;
        match event {
            AudioEvent::Effect(Effect::Emit(BusEvent::MuteChanged { muted })) => {
                self.muted = *muted;
            }
            AudioEvent::Effect(Effect::Audio(AudioCmd::SetCapture(on))) => self.capture = *on,
            AudioEvent::Input(Input::Activity(ActivityEvent::MicInUseElsewhere(elsewhere))) => {
                self.elsewhere = *elsewhere;
            }
            AudioEvent::Effect(Effect::Emit(BusEvent::Speaking { speak, .. })) if silent => {
                return self.fail(at_ms, format!("{speak} began"));
            }
            AudioEvent::Effect(Effect::Speech(SpeechCmd::Speak { speak, .. })) if silent => {
                return self.fail(at_ms, format!("{speak} was sent to synthesis"));
            }
            AudioEvent::Effect(Effect::Audio(AudioCmd::PlayPhrase { .. })) if silent => {
                return self.fail(at_ms, "a phrase played".to_owned());
            }
            AudioEvent::Effect(Effect::Audio(AudioCmd::Earcon(earcon)))
                if silent && !matches!(earcon, Earcon::Muted | Earcon::Unmuted) =>
            {
                return self.fail(at_ms, format!("the {earcon:?} earcon played"));
            }
            AudioEvent::Callback(handed) => {
                if self.muted && self.capture {
                    return self.fail(at_ms, "the microphone was still captured".to_owned());
                }
                if silent && let Some(part) = handed.iter().find(|part| part.samples > 0) {
                    return self.fail(
                        at_ms,
                        format!("{} samples of {} reached the device", part.samples, part.speak),
                    );
                }
            }
            AudioEvent::Input(_) | AudioEvent::Effect(_) => {}
        }
        Ok(())
    }
}

/// Invariant 10's floor, event by event: two utterances never overlap.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct OverlapWatch {
    floor: Option<SpeakId>,
}

impl OverlapWatch {
    /// Follows one bus event; when it starts an utterance while another holds the floor, the
    /// one that started and the other.
    pub(super) fn follow(&mut self, event: &BusEvent) -> Option<(SpeakId, SpeakId)> {
        if let BusEvent::Speaking { speak, .. } = event {
            let other = self.floor.filter(|other| other != speak);
            self.floor = Some(*speak);
            return other.map(|other| (*speak, other));
        }
        if let BusEvent::SpeakingStopped { speak, .. } = event
            && self.floor == Some(*speak)
        {
            self.floor = None;
        }
        None
    }

    /// Follows one record.
    pub fn see(&mut self, event: &AudioEvent) -> Result<(), Violation> {
        if let AudioEvent::Effect(Effect::Emit(bus)) = event
            && let Some((started, other)) = self.follow(bus)
        {
            return Err(Violation::new(
                10,
                "utterances_do_not_overlap",
                format!("{started} started while {other} was speaking"),
            ));
        }
        Ok(())
    }
}

//! `turn::exhaustive` (BUILD.md section 4.7, "Exhaustive tests", and section 6.3 acceptance 2):
//! every sequence of the turn-taking inputs up to a depth, through the real
//! [`eileen_core::turn::Turns`], with invariants 2, 3, 9 and 10 checked after every input.
//!
//! A [`Bench`] holds the machine, the parts that answer it and the four invariants' watches. It
//! plays the core's part around `Turns` for these inputs the way `Core::step` does in
//! `eileen-core`'s `route`, `brain` and `speech` modules: an addressed end of turn stops Eileen
//! and sends a brain turn told the heard prefix, a backchannel during a barge-in resumes, the
//! brain's text starts or extends one reply, muting stops her, and a pause with no partial
//! transcript resumes at its deadline. The parts: a device that takes 32 ms of samples at each
//! playback tick and reports its position after it, a speech process that readies one clause
//! at a time, and the user, whose speech over Eileen reaches the core as one barge-in window a
//! tick while the microphone is captured. Other audio and physical input reach the core, which
//! does not act on them yet (section 2.8's media pause and the input leases come with the full
//! core), so they leave the state as it was and the search folds them at once.
//!
//! The search is depth first from each [`Start`], cloning the bench to share each prefix, and
//! skips a state it already explored with at least as many inputs left. A state is keyed by a
//! 64-bit FNV-1a hash of everything that can change what follows, every time in it taken as an
//! age, so states that differ only in when they happened are explored once. Every tick is the
//! same 32 ms, so no phase of a clock is lost by that.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};

use eileen_contracts::bus::{BusEvent, SpeakCause};
use eileen_contracts::core_io::{
    ActivityEvent, AudioCmd, Earcon, Effect, Input, PlaybackEvent, UserInput,
};
use eileen_contracts::harness::{BrainCmd, BrainSlot};
use eileen_contracts::ids::{MonoTime, Personal, SpeakId, TurnId};
use eileen_contracts::jobs::TurnAuthority;
use eileen_contracts::speech::{SpeechCmd, TurnEndReason, VoiceEvent};
use eileen_core::turn::{Out, Turns, Verdict};

use crate::hash::TraceHash;
use crate::invariants::{
    AudioEvent, BargeInWatch, HEARD_MARKER, Handed, HeardWatch, MuteWatch, OverlapWatch, Violation,
    WINDOW_MS,
};

/// The depth pull requests run (section 4.7).
pub const PR_DEPTH: u8 = 6;
/// The depth the nightly lane runs.
pub const NIGHTLY_DEPTH: u8 = 8;
/// The environment variable that sets the depth of `turn::exhaustive`; the nightly lane sets it
/// to [`NIGHTLY_DEPTH`].
pub const DEPTH_ENV: &str = "EILEEN_EXHAUSTIVE_DEPTH";

/// One playback tick: a barge-in window's length.
const TICK_MS: u64 = WINDOW_MS;
/// Samples the device takes in a tick: 32 ms at 24 kHz.
const TICK_SAMPLES: u32 = 768;
/// Nanoseconds in a millisecond.
const NS_PER_MS: u64 = 1_000_000;
/// The brain's reply, one clause a delta; the last delta ends the turn.
const REPLY: [&str; 2] = ["One two three four.", "Five six seven."];
/// Each clause's samples once synthesized: the first ends mid-tick after three and a bit
/// ticks, so pauses, stops and clause boundaries fall inside a clause as well as between.
const CLAUSE_SAMPLES: [u32; 2] = [2_600, 1_900];
/// What the user says to Eileen at an end of turn, and as a backchannel.
const REQUEST: &str = "Eileen, tell me more";
const BACKCHANNEL: &str = "mm-hm";

/// One input of the enumeration: section 4.7's list of turn-taking inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    /// The user begins to speak.
    SpeechStart,
    /// The user stops speaking (the speech process's `SpeechEnded`).
    SpeechEnd,
    /// The user's turn ends with a request addressed to Eileen.
    EndOfTurn,
    /// The speech process readies the next clause of the utterance it holds.
    ClauseReady,
    /// 32 ms pass: one playback callback, a barge-in window if the user speaks over Eileen,
    /// and any timer due.
    PlaybackTick,
    /// The user mutes.
    Mute,
    /// The user unmutes.
    Unmute,
    /// The brain sends the next clause of its reply; the last ends its turn.
    BrainText,
    /// The user's turn ends with a backchannel ("mm-hm").
    Backchannel,
    /// Other audio starts playing.
    OtherAudio,
    /// Physical keyboard or mouse input.
    PhysicalInput,
}

/// Every [`Step`], in the order the search tries them.
pub const STEPS: [Step; 11] = [
    Step::SpeechStart,
    Step::SpeechEnd,
    Step::EndOfTurn,
    Step::ClauseReady,
    Step::PlaybackTick,
    Step::Mute,
    Step::Unmute,
    Step::BrainText,
    Step::Backchannel,
    Step::OtherAudio,
    Step::PhysicalInput,
];

/// Where an enumeration starts: idle, or partway into a reply, so that the whole depth is spent
/// on what follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Nobody speaks, nothing is pending, the microphone is on.
    Idle,
    /// Eileen speaks the first clause of a reply that is still streaming.
    Speaking,
    /// The user spoke over that clause for three windows, and playback is paused.
    Paused,
}

impl Start {
    /// Every start.
    pub const ALL: [Self; 3] = [Self::Idle, Self::Speaking, Self::Paused];

    /// The inputs that lead from idle to this start.
    #[must_use]
    pub const fn prefix(self) -> &'static [Step] {
        match self {
            Self::Idle => &[],
            Self::Speaking => {
                &[Step::SpeechStart, Step::EndOfTurn, Step::BrainText, Step::ClauseReady]
            }
            Self::Paused => &[
                Step::SpeechStart,
                Step::EndOfTurn,
                Step::BrainText,
                Step::ClauseReady,
                Step::SpeechStart,
                Step::PlaybackTick,
                Step::PlaybackTick,
                Step::PlaybackTick,
            ],
        }
    }
}

/// A broken core, for proving that the enumeration catches each invariant: the bench alters
/// what the machine asks for before the parts and the watches see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// None: the machine as it is.
    Off,
    /// A barge-in never ducks (invariant 2).
    NoDuck,
    /// A barge-in never pauses (invariant 2).
    NoPause,
    /// The brain is told the whole reply after an interruption (invariant 3).
    ToldWholeReply,
    /// Muting leaves the microphone captured (invariant 9).
    CaptureLeftOn,
    /// Every utterance is announced after another that never stops (invariant 10).
    PhantomUtterance,
}

impl Fault {
    /// Whether this fault drops `cmd`.
    const fn drops(self, cmd: &AudioCmd) -> bool {
        matches!(
            (self, cmd),
            (Self::NoDuck, AudioCmd::Duck { .. })
                | (Self::NoPause, AudioCmd::Pause { .. })
                | (Self::CaptureLeftOn, AudioCmd::SetCapture(false))
        )
    }
}

/// What an enumeration covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Explored {
    /// Where it started.
    pub start: Start,
    /// Its depth.
    pub depth: u8,
    /// Distinct states reached.
    pub states: usize,
    /// Inputs applied.
    pub transitions: u64,
    /// Inputs that led to a state already explored deeply enough.
    pub pruned: u64,
}

/// A sequence that broke an invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counterexample {
    /// Where it started.
    pub start: Start,
    /// The inputs from idle, the start's prefix included.
    pub steps: Vec<Step>,
    /// The invariant it broke.
    pub violation: Violation,
}

impl fmt::Display for Counterexample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "from {:?}, after {:?}: {}", self.start, self.steps, self.violation)
    }
}

impl std::error::Error for Counterexample {}

/// Enumerates every sequence of [`STEPS`] up to `depth` inputs after `start`'s prefix, with
/// the machine altered by `fault`, and returns what it covered or a shortest sequence that
/// broke an invariant (found by searching again at each depth from 1 once one has).
pub fn explore(start: Start, depth: u8, fault: Fault) -> Result<Explored, Counterexample> {
    let mut bench = Bench::new(fault);
    let mut path = Vec::new();
    for step in start.prefix() {
        path.push(*step);
        if let Err(violation) = bench.apply(*step) {
            return Err(Counterexample { start, steps: path, violation });
        }
    }
    let mut search = Search::new(start, depth, &bench, path.clone());
    let Err(violation) = search.below(&bench, depth) else {
        search.explored.states = search.visited.len();
        return Ok(search.explored);
    };
    let first = Counterexample { start, steps: search.path, violation };
    for shorter in 1..depth {
        let mut again = Search::new(start, shorter, &bench, path.clone());
        if let Err(violation) = again.below(&bench, shorter) {
            return Err(Counterexample { start, steps: again.path, violation });
        }
    }
    Err(first)
}

/// The depth-first search.
struct Search {
    /// The states explored, by key, with the inputs that were left below each.
    visited: BTreeMap<u64, u8>,
    /// The inputs from idle to the state being explored.
    path: Vec<Step>,
    explored: Explored,
}

impl Search {
    fn new(start: Start, depth: u8, bench: &Bench, path: Vec<Step>) -> Self {
        let visited = BTreeMap::from([(bench.key(), depth)]);
        let explored = Explored { start, depth, states: 0, transitions: 0, pruned: 0 };
        Self { visited, path, explored }
    }

    /// Explores every sequence of up to `left` inputs from `bench`.
    fn below(&mut self, bench: &Bench, left: u8) -> Result<(), Violation> {
        let Some(remaining) = left.checked_sub(1) else { return Ok(()) };
        for step in STEPS {
            self.explored.transitions = self.explored.transitions.saturating_add(1);
            self.path.push(step);
            let mut next = bench.clone();
            next.apply(step)?;
            let key = next.key();
            if self.visited.get(&key).is_some_and(|had| *had >= remaining) {
                self.explored.pruned = self.explored.pruned.saturating_add(1);
            } else {
                self.visited.insert(key, remaining);
                self.below(&next, remaining)?;
            }
            self.path.pop();
        }
        Ok(())
    }
}

/// A [`Hasher`] over [`TraceHash`], so state keys are the same on every OS and in every build.
struct Fnv(TraceHash);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0.value()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// The user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
struct User {
    speaking: bool,
    /// The turn the speech process opened for their speech, until it ends.
    turn: Option<TurnId>,
}

/// The playback device: one utterance at a time, held until it is stopped or the next one's
/// samples arrive, so a pause holds over samples that arrive after it ran dry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Device {
    speak: Option<SpeakId>,
    /// Clauses given and not started: number and samples.
    queued: VecDeque<(u16, u32)>,
    /// The clause playing: number, samples, samples played.
    current: Option<(u16, u32, u32)>,
    /// The last position reached.
    last: (u16, u32),
    paused: bool,
    /// Whether the microphone is captured.
    capture: bool,
    /// It ran dry and said so; it says so again only after more samples arrive.
    dry: bool,
}

impl Device {
    const fn new() -> Self {
        Self {
            speak: None,
            queued: VecDeque::new(),
            current: None,
            last: (0, 0),
            paused: false,
            capture: true,
            dry: false,
        }
    }

    fn forget(&mut self) {
        self.speak = None;
        self.queued.clear();
        self.current = None;
        self.last = (0, 0);
        self.paused = false;
        self.dry = false;
    }

    /// A clause's samples arrive.
    fn clause_ready(&mut self, speak: SpeakId, clause: u16, samples: u32) {
        if self.speak != Some(speak) {
            self.forget();
            self.speak = Some(speak);
            self.last = (clause, 0);
        }
        self.queued.push_back((clause, samples));
        self.dry = false;
    }

    /// Carries out an audio command; the report it sends back, if any.
    fn command(&mut self, cmd: &AudioCmd) -> Option<PlaybackEvent> {
        let (clause, samples_played) = self.last;
        match cmd {
            AudioCmd::SetCapture(on) => self.capture = *on,
            AudioCmd::Pause { speak } if self.speak == Some(*speak) => {
                self.paused = true;
                return Some(PlaybackEvent::Paused { speak: *speak, clause, samples_played });
            }
            AudioCmd::Resume { speak } if self.speak == Some(*speak) => self.paused = false,
            AudioCmd::Stop { speak } if self.speak == Some(*speak) => {
                self.forget();
                return Some(PlaybackEvent::Stopped { speak: *speak, clause, samples_played });
            }
            AudioCmd::Pause { .. }
            | AudioCmd::Resume { .. }
            | AudioCmd::Stop { .. }
            | AudioCmd::Duck { .. }
            | AudioCmd::Unduck
            | AudioCmd::PlayPhrase { .. }
            | AudioCmd::Earcon(_)
            | AudioCmd::Record { .. } => {}
        }
        None
    }

    /// One tick's callback: the samples it hands to the device, clause by clause.
    fn callback(&mut self) -> Vec<Handed> {
        let mut handed = Vec::new();
        let Some(speak) = self.speak.filter(|_| !self.paused) else { return handed };
        let mut budget = TICK_SAMPLES;
        while budget > 0 {
            let next = self
                .current
                .or_else(|| self.queued.pop_front().map(|(clause, total)| (clause, total, 0)));
            let Some((clause, total, played)) = next else { break };
            let take = budget.min(total.saturating_sub(played));
            budget = budget.saturating_sub(take);
            let played = played.saturating_add(take);
            if take > 0 {
                handed.push(Handed { speak, clause, samples: take });
            }
            self.last = (clause, played);
            self.current = (played < total).then_some((clause, total, played));
        }
        handed
    }
}

/// The utterance the speech process holds: the next clause to ready and how many it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Synth {
    speak: SpeakId,
    next: u16,
    count: u16,
}

/// The brain turn in flight, as `eileen-core`'s brain module follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Reply {
    turn: TurnId,
    /// Its utterance, once started.
    speak: Option<SpeakId>,
    /// Deltas sent so far.
    sent: u8,
    /// Its utterance was stopped, so the rest is dropped.
    silenced: bool,
    /// It goes to the HUD instead of being spoken.
    on_hud: bool,
}

/// The four invariants' watches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct Watches {
    barge: BargeInWatch,
    heard: HeardWatch,
    mute: MuteWatch,
    overlap: OverlapWatch,
}

/// The turn-taking machine among the parts that answer it, with the watches.
#[derive(Debug, Clone)]
pub struct Bench {
    turns: Turns,
    now_ms: u64,
    fault: Fault,
    /// The number the next turn or utterance id is minted from.
    next_id: u64,
    user: User,
    device: Device,
    synth: Option<Synth>,
    brain: Option<Reply>,
    /// When the barge-in decision timer fires, ms.
    decide_by: Option<u64>,
    /// The number of the last barge-in window, 0 while no run is under way.
    window: u8,
    watches: Watches,
}

impl Bench {
    /// An idle, unmuted bench whose machine `fault` alters.
    #[must_use]
    pub fn new(fault: Fault) -> Self {
        Self {
            turns: Turns::new(false),
            now_ms: 0,
            fault,
            next_id: 1,
            user: User::default(),
            device: Device::new(),
            synth: None,
            brain: None,
            decide_by: None,
            window: 0,
            watches: Watches::default(),
        }
    }

    /// The machine.
    #[must_use]
    pub const fn turns(&self) -> &Turns {
        &self.turns
    }

    /// Virtual time, ms.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// The state's key: a hash of everything that can change what follows, with every time as
    /// an age. The id counter is left out: two states that differ in it alone go on alike.
    #[must_use]
    pub fn key(&self) -> u64 {
        let mut hasher = Fnv(TraceHash::default());
        self.turns.hash(&mut hasher);
        self.user.hash(&mut hasher);
        self.device.hash(&mut hasher);
        self.synth.hash(&mut hasher);
        self.brain.hash(&mut hasher);
        self.decide_by.map(|at| at.saturating_sub(self.now_ms)).hash(&mut hasher);
        self.window.hash(&mut hasher);
        self.watches.barge.aged(self.now_ms).hash(&mut hasher);
        self.watches.heard.hash(&mut hasher);
        self.watches.mute.hash(&mut hasher);
        self.watches.overlap.hash(&mut hasher);
        hasher.finish()
    }

    /// Applies one input and checks the invariants on everything it caused.
    pub fn apply(&mut self, step: Step) -> Result<(), Violation> {
        match step {
            Step::SpeechStart => self.speech_start()?,
            Step::SpeechEnd => self.user.speaking = false,
            Step::EndOfTurn => self.end_of_turn(REQUEST)?,
            Step::Backchannel => self.end_of_turn(BACKCHANNEL)?,
            Step::ClauseReady => self.clause_ready()?,
            Step::PlaybackTick => self.tick()?,
            Step::Mute => self.set_mute(true)?,
            Step::Unmute => self.set_mute(false)?,
            Step::BrainText => self.brain_text()?,
            Step::OtherAudio => {
                self.input(Input::Activity(ActivityEvent::OtherAudio { playing: true }))?;
            }
            Step::PhysicalInput => self.input(Input::Activity(ActivityEvent::PhysicalInput))?,
        }
        // The speech process numbers windows; only consecutiveness within a run matters, so
        // the count starts again once no run is under way.
        if !self.turns.barge_in() || self.turns.paused() {
            self.window = 0;
        }
        self.watches.barge.by(self.now_ms)
    }

    const fn mint(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Shows the watches one record at the current time.
    fn see(&mut self, event: &AudioEvent) -> Result<(), Violation> {
        let at = self.now_ms;
        self.watches.barge.see(at, event)?;
        self.watches.heard.see(event)?;
        self.watches.mute.see(at, event)?;
        self.watches.overlap.see(event)
    }

    fn input(&mut self, input: Input) -> Result<(), Violation> {
        self.see(&AudioEvent::Input(input))
    }

    fn effect(&mut self, effect: Effect) -> Result<(), Violation> {
        self.see(&AudioEvent::Effect(effect))
    }

    /// The user begins to speak; the core hears of it only while the microphone is captured.
    fn speech_start(&mut self) -> Result<(), Violation> {
        if self.user.speaking {
            return Ok(());
        }
        self.user.speaking = true;
        if !self.device.capture {
            return Ok(());
        }
        let turn = if let Some(turn) = self.user.turn {
            turn
        } else {
            let id = self.mint();
            TurnId::from_parts(id, u128::from(id))
        };
        self.user.turn = Some(turn);
        self.input(Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }))?;
        self.turns.speech_started(turn);
        Ok(())
    }

    /// The user's turn ends with `text`, as `route::end_of_turn` and `route::utterance` take
    /// it: nothing while muted; a backchannel resumes a barge-in and is otherwise not
    /// addressed; a request stops Eileen and goes to the brain.
    fn end_of_turn(&mut self, text: &str) -> Result<(), Violation> {
        let Some(turn) = self.user.turn.take() else { return Ok(()) };
        self.user.speaking = false;
        self.input(Input::Voice(VoiceEvent::EndOfTurn {
            turn,
            text: Personal::new(text.to_owned()),
            smart_turn_pct: 90,
            silence_ms: 250,
            asr_confidence_pct: 95,
            reason: TurnEndReason::SmartTurn,
            source: 0,
            level_db: -30,
        }))?;
        self.turns.end_of_turn();
        if self.turns.muted() {
            return Ok(());
        }
        let mut out = Vec::new();
        if text == BACKCHANNEL {
            if self.turns.barge_in() {
                self.turns.resolve_barge_in(Verdict::Resume, &mut out);
            }
            return self.apply_outs(out);
        }
        self.turns.stop(true, &mut out);
        self.apply_outs(out)?;
        if let Some(old) = self.brain.take() {
            self.effect(Effect::Brain(BrainCmd::Interrupt { slot: BrainSlot::Primary }))?;
            self.turns.thought(old.turn);
        }
        let heard = self.turns.take_heard();
        let told = if self.fault == Fault::ToldWholeReply {
            heard.map(|_| REPLY.join(" "))
        } else {
            heard
        };
        let message = match told {
            Some(heard) => format!("[heard] {text}\n        {HEARD_MARKER}{heard}\""),
            None => format!("[heard] {text}"),
        };
        self.effect(Effect::Audio(AudioCmd::Earcon(Earcon::Routed)))?;
        let authority = TurnAuthority::Addressed;
        let slot = BrainSlot::Primary;
        self.effect(Effect::Brain(BrainCmd::Turn { slot, turn, message, authority }))?;
        self.turns.think(turn);
        self.brain = Some(Reply { turn, speak: None, sent: 0, silenced: false, on_hud: false });
        Ok(())
    }

    /// The speech process readies the next clause of the utterance it holds; its samples reach
    /// the device before the core hears of it.
    fn clause_ready(&mut self) -> Result<(), Violation> {
        let Some(synth) = self.synth.filter(|synth| synth.next < synth.count) else {
            return Ok(());
        };
        let index = usize::from(synth.next);
        let samples = CLAUSE_SAMPLES.get(index).copied().unwrap_or(TICK_SAMPLES);
        let chars = REPLY.get(index).map_or(0, |text| text.chars().count());
        let (speak, clause) = (synth.speak, synth.next);
        self.input(Input::Voice(VoiceEvent::ClauseReady {
            speak,
            clause,
            chars: u32::try_from(chars).unwrap_or(u32::MAX),
            samples,
        }))?;
        self.device.clause_ready(speak, clause, samples);
        self.turns.clause_ready(speak, clause, samples);
        self.synth = Some(Synth { next: clause.saturating_add(1), ..synth });
        Ok(())
    }

    /// 32 ms pass: the device's callback and its report, then the user's window, then the
    /// decision timer if it is due.
    fn tick(&mut self) -> Result<(), Violation> {
        self.now_ms = self.now_ms.saturating_add(TICK_MS);
        let handed = self.device.callback();
        let played = !handed.is_empty();
        self.see(&AudioEvent::Callback(handed))?;
        let mut out = Vec::new();
        if let Some(speak) = self.device.speak.filter(|_| !self.device.paused) {
            if played {
                let (clause, samples_played) = self.device.last;
                self.input(Input::Playback(PlaybackEvent::Played {
                    speak,
                    clause,
                    samples_played,
                }))?;
                self.turns.played(speak, clause, samples_played);
            } else if !self.device.dry {
                self.device.dry = true;
                self.input(Input::Playback(PlaybackEvent::Drained { speak }))?;
                self.turns.drained(speak, &mut out);
            }
        }
        self.apply_outs(out)?;
        let floor = self.turns.speaking().filter(|_| self.user.speaking && self.device.capture);
        if let Some(speak) = floor {
            self.window = self.window.wrapping_add(1);
            let window = self.window;
            self.input(Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }))?;
            let mut out = Vec::new();
            let now = MonoTime(self.now_ms.saturating_mul(NS_PER_MS));
            self.turns.barge_in_window(now, speak, window, &mut out);
            self.apply_outs(out)?;
        } else if self.window != 0 {
            // A window with no speech in it: the next one is not consecutive.
            self.window = self.window.wrapping_add(1);
        }
        if self.decide_by.is_some_and(|at| at <= self.now_ms) {
            self.decide_by = None;
            let mut out = Vec::new();
            if self.turns.barge_in_deadline(&mut out) {
                // `route::verdict_from_partial` with no partial transcript resumes.
                self.turns.resolve_barge_in(Verdict::Resume, &mut out);
            }
            self.apply_outs(out)?;
        }
        Ok(())
    }

    /// The user mutes or unmutes, as `speech::set_mute` does it: a reply muting stopped goes
    /// on to the HUD.
    fn set_mute(&mut self, muted: bool) -> Result<(), Violation> {
        self.input(Input::User(UserInput::SetMute(muted)))?;
        let stopped = self.turns.speaking().filter(|_| muted);
        let mut out = Vec::new();
        let changed = self.turns.set_mute(muted, &mut out);
        self.apply_outs(out)?;
        if changed
            && let Some(reply) = self.brain.as_mut()
            && stopped.is_some()
            && reply.speak == stopped
        {
            reply.on_hud = true;
        }
        Ok(())
    }

    /// The brain sends its next clause, as `brain::brain_event` takes a `TextDelta`; the last
    /// one closes the reply and ends the turn, as `TurnDone` does.
    fn brain_text(&mut self) -> Result<(), Violation> {
        let Some(reply) = self.brain else { return Ok(()) };
        let index = usize::from(reply.sent);
        let Some(text) = REPLY.get(index) else { return Ok(()) };
        let close = index.saturating_add(1) == REPLY.len();
        self.brain = Some(Reply { sent: reply.sent.saturating_add(1), ..reply });
        self.reply(vec![(*text).to_owned()], close)?;
        if close {
            self.brain = None;
            self.turns.thought(reply.turn);
        }
        Ok(())
    }

    /// `brain::reply`: the first clause starts the reply's utterance, the rest extend it; while
    /// muted it goes to the HUD, and once its utterance was stopped the rest is dropped.
    fn reply(&mut self, clauses: Vec<String>, close: bool) -> Result<(), Violation> {
        let Some(mut reply) = self.brain else { return Ok(()) };
        if reply.silenced {
            return Ok(());
        }
        if reply.on_hud || self.turns.muted() {
            reply.on_hud = true;
            self.brain = Some(reply);
            return Ok(());
        }
        let mut out = Vec::new();
        match reply.speak {
            Some(speak) if self.turns.speaking() == Some(speak) => {
                self.turns.append(speak, "", clauses, &mut out);
                if close {
                    self.turns.close(speak, &mut out);
                }
                self.apply_outs(out)
            }
            Some(_) => {
                reply.silenced = true;
                self.brain = Some(reply);
                Ok(())
            }
            None => {
                self.turns.stop(false, &mut out);
                self.apply_outs(out)?;
                let id = self.mint();
                let speak = SpeakId::from_parts(id, u128::from(id));
                let mut out = Vec::new();
                if !self.turns.speak(speak, SpeakCause::Reply, "", clauses, &mut out) {
                    reply.silenced = true;
                    self.brain = Some(reply);
                    return Ok(());
                }
                reply.speak = Some(speak);
                self.brain = Some(reply);
                self.apply_outs(out)?;
                if close {
                    let mut out = Vec::new();
                    self.turns.close(speak, &mut out);
                    self.apply_outs(out)?;
                }
                Ok(())
            }
        }
    }

    /// What the speech process does with a command for it.
    fn synthesize(&mut self, cmd: &SpeechCmd) {
        if let SpeechCmd::Speak { speak, clauses, .. } = cmd {
            let added = u16::try_from(clauses.len()).unwrap_or(u16::MAX);
            if let Some(synth) = self.synth.as_mut()
                && synth.speak == *speak
            {
                synth.count = synth.count.saturating_add(added);
            } else {
                self.synth = Some(Synth { speak: *speak, next: 0, count: added });
            }
        }
        if let SpeechCmd::Cancel { speak } = cmd
            && self.synth.is_some_and(|synth| synth.speak == *speak)
        {
            self.synth = None;
        }
    }

    /// Carries out what the machine asked for, in order, as `speech::apply` does, then hands
    /// the machine the device's reports.
    fn apply_outs(&mut self, outs: Vec<Out>) -> Result<(), Violation> {
        let mut reports = Vec::new();
        for out in outs {
            match out {
                Out::Audio(cmd) => {
                    if self.fault.drops(&cmd) {
                        continue;
                    }
                    reports.extend(self.device.command(&cmd));
                    self.effect(Effect::Audio(cmd))?;
                }
                Out::Speech(cmd) => {
                    self.synthesize(&cmd);
                    self.effect(Effect::Speech(cmd))?;
                }
                Out::Started { speak, cause, text } => {
                    if self.fault == Fault::PhantomUtterance {
                        let phantom = SpeakId::from_parts(0, u128::from(u64::MAX));
                        let text = String::new();
                        self.effect(Effect::Emit(BusEvent::Speaking {
                            speak: phantom,
                            cause,
                            text,
                        }))?;
                    }
                    self.effect(Effect::Emit(BusEvent::Speaking { speak, cause, text }))?;
                }
                Out::Stopped { speak, heard_chars, interrupted, resumed, .. } => {
                    let event =
                        BusEvent::SpeakingStopped { speak, heard_chars, interrupted, resumed };
                    self.effect(Effect::Emit(event))?;
                }
                Out::DecideBy(at) => self.decide_by = Some(at.0 / NS_PER_MS),
                Out::Muted(muted) => self.effect(Effect::Emit(BusEvent::MuteChanged { muted }))?,
                Out::Unspoken { .. } | Out::BargeIn { .. } | Out::BargeInEnded { .. } => {}
            }
        }
        for report in reports {
            self.input(Input::Playback(report))?;
            let mut out = Vec::new();
            match report {
                PlaybackEvent::Paused { speak, clause, samples_played }
                | PlaybackEvent::Played { speak, clause, samples_played } => {
                    self.turns.played(speak, clause, samples_played);
                }
                PlaybackEvent::Stopped { speak, clause, samples_played } => {
                    self.turns.device_stopped(speak, clause, samples_played, &mut out);
                }
                PlaybackEvent::Drained { .. }
                | PlaybackEvent::Stalled
                | PlaybackEvent::DeviceChanged => {}
            }
            if !out.is_empty() {
                self.apply_outs(out)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

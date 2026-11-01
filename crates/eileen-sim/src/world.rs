//! The `World`: the real core among simulated parts, in virtual time.
//!
//! Each step takes one input, wraps it in an [`Envelope`] with the next `seq` and the virtual
//! time, calls `Core::step`, applies the effects to the simulated parts, checks every
//! invariant, writes one line of the step log and feeds the trace hash. The parts: timers the
//! core set, the speech process with its life and the playback device, a scripted brain and
//! scripted worker sessions ([`crate::parts`]), the bus with one slow subscriber, the HUD's
//! last state, the supervised children's lives (invariant 8), and the span collector
//! ([`crate::collect`]), which keeps every span and log line the core produces on the
//! simulator's thread for invariant 13.
//! What the parts answer is scheduled at virtual times, and [`World::advance`] delivers it,
//! and the timers, in time order, running the device's 10 ms callbacks as time passes.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::{self, Write as _};

use eileen_contracts::bus::BusEvent;
use eileen_contracts::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use eileen_contracts::core_io::{Effect, Input, PlaybackEvent, TimerId};
use eileen_contracts::hud::HudState;
use eileen_contracts::ids::{Envelope, MonoTime, SpeakId, TraceCtx, WallTime};
use eileen_contracts::ports::SettingsValidator;
use eileen_contracts::record::InputRecord;
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};
use eileen_core::{Core, CoreConfig, CorePorts};
use eileen_stdx::rng::CoreRng;
use serde_json::Value;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::collect::Collector;
use crate::hash::TraceHash;
use crate::invariants::{self, ChildEvent, ChildRecord, Delivery, Violation};
use crate::parts::Brain as BrainPart;
use crate::parts::{BrainReply, CALLBACK_MS, Device, Due, Speech, Workers};
use crate::profile::Profile;
use crate::resolver::SimResolver;

/// The wall clock the world starts at: 2026-01-01T00:00:00Z, in microseconds. Wall time only
/// stamps envelopes; nothing in a run depends on today's date.
const EPOCH_US: i64 = 1_767_225_600_000_000;
/// Nanoseconds in a millisecond, for `MonoTime`.
const NS_PER_MS: u64 = 1_000_000;
/// The width of the step log's input column.
const INPUT_COLUMN: usize = 32;

/// The settings validator the simulated core uses. `eileen-sim` may not depend on
/// `eileen-config` (BUILD.md section 3.1), so it carries its own; like the stub, it refuses
/// every change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SimSettings;

impl SettingsValidator for SimSettings {
    fn validate(
        &self,
        _current: &Config,
        _changes: &[SettingChange],
    ) -> Result<Proposal, ValidationError> {
        Err(ValidationError {
            key: String::new(),
            line: None,
            message: "the simulator refuses setting changes".to_owned(),
        })
    }

    fn specs(&self) -> &[SettingSpecView] {
        &[]
    }
}

/// The bus with one slow subscriber: a bounded queue it drains a few events per step. When the
/// queue is full an event is dropped and counted, and the next event that fits is preceded by
/// a `Lagged` notice with the count.
#[derive(Debug)]
struct Bus {
    queue: VecDeque<Delivery>,
    capacity: usize,
    drain: usize,
    missed: u64,
    delivered: Vec<Delivery>,
}

impl Bus {
    const fn new(capacity: usize, drain: usize) -> Self {
        Self { queue: VecDeque::new(), capacity, drain, missed: 0, delivered: Vec::new() }
    }

    fn publish(&mut self, seq: u64) {
        let room = self.capacity.saturating_sub(self.queue.len());
        match (room, self.missed) {
            (0, _) => self.missed = self.missed.saturating_add(1),
            (1, 1..) => {
                // Room for the notice only: it counts this event too.
                self.queue.push_back(Delivery::Lagged(self.missed.saturating_add(1)));
                self.missed = 0;
            }
            (_, 0) => self.queue.push_back(Delivery::Event(seq)),
            (_, missed) => {
                self.queue.push_back(Delivery::Lagged(missed));
                self.queue.push_back(Delivery::Event(seq));
                self.missed = 0;
            }
        }
    }

    fn take(&mut self) {
        for _ in 0..self.drain {
            let Some(item) = self.queue.pop_front() else { break };
            self.delivered.push(item);
        }
    }
}

/// What a step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    /// The step's line of the step log.
    pub line: String,
    /// The invariant it broke, if any.
    pub violation: Option<Violation>,
}

/// The core among simulated parts.
pub struct World {
    profile: Profile,
    now_ms: u64,
    seq: u64,
    steps: u32,
    core: Core,
    core_rng: CoreRng,
    id_rng: CoreRng,
    timers: BTreeMap<u64, MonoTime>,
    due: Due,
    speech: Speech,
    device: Device,
    brain: BrainPart,
    workers: Workers,
    /// The utterances the speech process holds, which playback events may name (invariant 10).
    speech_queue: Vec<SpeakId>,
    /// The supervised children's lives (invariant 8).
    children: Vec<ChildRecord>,
    /// The HUD state the core sent last.
    hud: Option<HudState>,
    effects: Vec<Envelope<Effect>>,
    published: Vec<Envelope<BusEvent>>,
    bus: Bus,
    inputs: Vec<InputRecord>,
    log: String,
    hash: TraceHash,
    collector: Collector,
    _trace: DefaultGuard,
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("World")
            .field("profile", &self.profile.name)
            .field("now_ms", &self.now_ms)
            .field("seq", &self.seq)
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

impl World {
    /// A world around a fresh core with the default [`SimResolver`], its random numbers seeded
    /// from `seed`. Spans and log lines on this thread go to the world's collector until it is
    /// dropped.
    #[must_use]
    pub fn new(profile: Profile, seed: u64) -> Self {
        Self::with_resolver(profile, seed, SimResolver::default())
    }

    /// A world whose core resolves intents with `resolver`.
    #[must_use]
    pub fn with_resolver(profile: Profile, seed: u64, resolver: SimResolver) -> Self {
        let ports = CorePorts {
            resolver: Box::new(resolver),
            cleaner: Box::new(eileen_dictation::Cleaner),
            settings: Box::new(SimSettings),
            skills: Box::new(eileen_skills::Engine),
        };
        let core = Core::new(CoreConfig::default(), ports, None);
        let collector = Collector::default();
        let subscriber = tracing_subscriber::registry().with(collector.layer());
        let trace = tracing::subscriber::set_default(subscriber);
        // Two streams from the seed: the core's, and the one the generator mints IDs from, so a
        // replay, which mints none, hands the core the same numbers the run did.
        let mut core_rng = CoreRng::from_seed(seed);
        let id_rng = core_rng.fork();
        let bus = Bus::new(
            usize::try_from(profile.bus_subscriber_capacity).unwrap_or(usize::MAX),
            usize::try_from(profile.bus_subscriber_drain).unwrap_or(usize::MAX),
        );
        Self {
            profile,
            now_ms: 0,
            seq: 0,
            steps: 0,
            core,
            core_rng,
            id_rng,
            timers: BTreeMap::new(),
            due: Due::default(),
            speech: Speech::default(),
            device: Device::new(),
            brain: BrainPart::default(),
            workers: Workers::default(),
            speech_queue: Vec::new(),
            children: Vec::new(),
            hud: None,
            effects: Vec::new(),
            published: Vec::new(),
            bus,
            inputs: Vec::new(),
            log: String::new(),
            hash: TraceHash::default(),
            collector,
            _trace: trace,
        }
    }

    /// The profile.
    #[must_use]
    pub const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Virtual time, ms since the run began.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Steps taken.
    #[must_use]
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// Utterances the simulated speech process holds, which playback events may name.
    #[must_use]
    pub fn speaking(&self) -> &[SpeakId] {
        &self.speech_queue
    }

    /// The bus events published so far.
    #[must_use]
    pub fn published(&self) -> &[Envelope<BusEvent>] {
        &self.published
    }

    /// Every effect the core emitted so far, in order.
    #[must_use]
    pub fn effects(&self) -> &[Envelope<Effect>] {
        &self.effects
    }

    /// The simulated playback device.
    #[must_use]
    pub const fn device(&self) -> &Device {
        &self.device
    }

    /// The simulated speech process.
    #[must_use]
    pub const fn speech(&self) -> &Speech {
        &self.speech
    }

    /// The HUD state the core sent last.
    #[must_use]
    pub const fn hud(&self) -> Option<&HudState> {
        self.hud.as_ref()
    }

    /// The supervised children's lives so far.
    #[must_use]
    pub fn children(&self) -> &[ChildRecord] {
        &self.children
    }

    /// The speech process hangs: it sends nothing more until the core has it killed.
    pub fn hang_speech(&mut self) {
        self.speech.hang(&mut self.due);
    }

    /// The speech process crashes: the supervisor reports its exit now, and the utterances it
    /// held that never reached the device are gone with it.
    pub fn crash_speech(&mut self) {
        self.speech.crash(self.now_ms, &mut self.due);
        self.speech_gone();
    }

    /// Makes every later load of the speech process never send `Ready` (`true`), or finish
    /// again (`false`).
    pub const fn stick_speech_loads(&mut self, stuck: bool) {
        self.speech.stick_loads(stuck);
    }

    /// The speech process ended: it holds no utterance now, and the device plays on only what
    /// it was given.
    fn speech_gone(&mut self) {
        let playing = self.device.speaking();
        self.speech_queue.retain(|queued| playing.contains(queued));
    }

    /// Queues the scripted brain's reply to the next turn it is sent.
    pub fn script_brain(&mut self, reply: BrainReply) {
        self.brain.script(reply);
    }

    /// The decoded inputs so far, as a `SimRegression` records them.
    #[must_use]
    pub fn inputs(&self) -> &[InputRecord] {
        &self.inputs
    }

    /// The step log so far.
    #[must_use]
    pub fn log(&self) -> &str {
        &self.log
    }

    /// The trace hash so far: the step log, every input and every effect.
    #[must_use]
    pub const fn hash(&self) -> u64 {
        self.hash.value()
    }

    /// The random numbers the generator mints IDs with (the core has its own stream).
    pub const fn id_rng(&mut self) -> &mut CoreRng {
        &mut self.id_rng
    }

    /// Moves virtual time forward by up to `ms`, running the device's 10 ms callbacks, and
    /// returns the first input due on the way: a part's answer or a timer, which the next step
    /// should deliver. Time stops where that input is due; with nothing due it moves the whole
    /// way and returns `None`.
    pub fn advance(&mut self, ms: u64) -> Option<Input> {
        if let Some(input) = self.take_due() {
            return Some(input);
        }
        let target = self.now_ms.saturating_add(ms);
        while self.now_ms < target {
            let next_tick =
                (self.now_ms / CALLBACK_MS).saturating_add(1).saturating_mul(CALLBACK_MS);
            self.now_ms = next_tick.min(target);
            if self.now_ms == next_tick {
                self.device.callback(self.now_ms, &mut self.due);
            }
            if let Some(input) = self.take_due() {
                return Some(input);
            }
        }
        None
    }

    /// How long until the earliest timer the core set, ms (0 when one is due).
    #[must_use]
    pub fn until_next_timer(&self) -> Option<u64> {
        let at = self.timers.values().min()?;
        Some((at.0 / NS_PER_MS).saturating_sub(self.now_ms))
    }

    /// Delivers everything due within `ms` of virtual time, one step each, and returns the
    /// steps' outcomes.
    pub fn settle(&mut self, ms: u64) -> Vec<StepOutcome> {
        let target = self.now_ms.saturating_add(ms);
        let mut outcomes = Vec::new();
        while let Some(input) = self.advance(target.saturating_sub(self.now_ms)) {
            outcomes.push(self.step(input));
        }
        outcomes
    }

    /// The earliest timer or part's answer due now; a timer goes first only when it is due
    /// strictly earlier.
    fn take_due(&mut self) -> Option<Input> {
        let now = self.mono();
        let timer = self
            .timers
            .iter()
            .filter(|(_, at)| **at <= now)
            .min_by_key(|(id, at)| (**at, **id))
            .map(|(id, at)| (*id, *at));
        let item = self.due.next_at().filter(|at| *at <= self.now_ms);
        let timer_first = match (timer, item) {
            (Some((_, at)), Some(item)) => at.0 < item.saturating_mul(NS_PER_MS),
            (Some(_), None) => true,
            (None, _) => false,
        };
        if timer_first && let Some((id, _)) = timer {
            self.timers.remove(&id);
            return Some(Input::Timer(TimerId(id)));
        }
        self.due.pop(self.now_ms)
    }

    /// Sets virtual time to `mono` (a replayed input's time), never backwards.
    pub fn set_time(&mut self, mono: MonoTime) {
        self.now_ms = self.now_ms.max(mono.0 / NS_PER_MS);
    }

    /// One step: `input` into the core at the current virtual time.
    pub fn step(&mut self, input: Input) -> StepOutcome {
        self.seq = self.seq.saturating_add(1);
        self.steps = self.steps.saturating_add(1);
        let mono = self.mono();
        let at =
            WallTime(EPOCH_US.saturating_add(
                i64::try_from(self.now_ms.saturating_mul(1_000)).unwrap_or(i64::MAX),
            ));
        let ctx = TraceCtx { turn: turn_of(&input), ..TraceCtx::default() };
        let label = label_of(&input);
        let record = InputRecord { seq: self.seq, mono, at, ctx, input: input.clone() };
        self.hash.record(&serde_json::to_vec(&record).unwrap_or_default());
        self.apply_input(&input);
        self.inputs.push(record);
        let envelope = Envelope { seq: self.seq, at, mono, ctx, body: input };
        let mut effects = Vec::new();
        self.core.step(mono, &mut self.core_rng, envelope, &mut effects);
        let count = effects.len();
        for effect in effects {
            self.hash.record(&serde_json::to_vec(&effect).unwrap_or_default());
            self.apply_effect(effect);
        }
        self.bus.take();
        let violation = self.check();
        let status =
            violation.as_ref().map_or_else(|| "ok".to_owned(), |v| format!("FAIL {}", v.number));
        let line = format_line(self.steps, self.now_ms, &label, count, &status);
        self.log.push_str(&line);
        self.log.push('\n');
        self.hash.record(line.as_bytes());
        StepOutcome { line, violation }
    }

    const fn mono(&self) -> MonoTime {
        MonoTime(self.now_ms.saturating_mul(NS_PER_MS))
    }

    /// What an input does to the parts before the core sees it: a clause's samples reach the
    /// device, and a drained utterance leaves the speech process and the device.
    fn apply_input(&mut self, input: &Input) {
        match input {
            Input::Playback(PlaybackEvent::Drained { speak }) => {
                self.speech_queue.retain(|queued| queued != speak);
                self.device.forget(*speak);
            }
            Input::Voice(VoiceEvent::ClauseReady { speak, clause, samples, .. }) => {
                self.device.clause_ready(*speak, *clause, *samples);
            }
            Input::Voice(VoiceEvent::SpeakDone { speak })
                if !self.device.speaking().contains(speak) =>
            {
                // Done with no clause on the device: nothing of it will ever play.
                self.speech_queue.retain(|queued| queued != speak);
            }
            Input::Sidecar(child, SidecarEvent::Exited { .. }) => {
                self.child(*child, ChildEvent::Down);
            }
            Input::Sidecar(SidecarId::Speech, _) => {
                self.speech.delivered(self.now_ms, input, &mut self.due);
            }
            _ => {}
        }
    }

    /// Notes an event in a child's life.
    fn child(&mut self, child: SidecarId, event: ChildEvent) {
        self.children.push(ChildRecord { at_ms: self.now_ms, child, event });
    }

    fn apply_effect(&mut self, effect: Envelope<Effect>) {
        let now = self.now_ms;
        match &effect.body {
            Effect::Emit(event) => {
                self.bus.publish(effect.seq);
                let published = Envelope {
                    seq: effect.seq,
                    at: effect.at,
                    mono: effect.mono,
                    ctx: effect.ctx,
                    body: event.clone(),
                };
                self.published.push(published);
            }
            Effect::Speech(cmd) => {
                if let SpeechCmd::Speak { speak, .. } = cmd
                    && !self.speech_queue.contains(speak)
                {
                    self.speech_queue.push(*speak);
                }
                if let SpeechCmd::Cancel { speak } = cmd {
                    self.speech_queue.retain(|queued| queued != speak);
                }
                self.speech.command(now, cmd, &mut self.due);
            }
            Effect::Audio(cmd) => self.device.command(now, cmd, &mut self.due),
            Effect::Brain(cmd) => self.brain.command(now, cmd, &mut self.due),
            Effect::Worker(session, cmd) => {
                self.workers.command(now, *session, cmd, &mut self.due);
            }
            Effect::Sidecar(child, cmd) => {
                let event = match cmd {
                    SidecarCmd::Start => ChildEvent::Started,
                    SidecarCmd::Restart => ChildEvent::Restarted,
                    SidecarCmd::HangDumpThenKill => ChildEvent::Down,
                };
                self.child(*child, event);
                if *child == SidecarId::Speech {
                    self.speech.sidecar(now, *cmd, &mut self.due);
                    if *cmd == SidecarCmd::HangDumpThenKill {
                        self.speech_gone();
                    }
                }
            }
            Effect::Hud(state) => self.hud = Some((**state).clone()),
            Effect::SetTimer { id, at } => {
                self.timers.insert(id.0, *at);
            }
            Effect::CancelTimer(id) => {
                self.timers.remove(&id.0);
            }
            // The parts that answer the rest (the other sidecars, the store, the learning store)
            // arrive with the full core; their effects are recorded and hashed only.
            Effect::Hook(..)
            | Effect::ToolForward { .. }
            | Effect::ToolReply { .. }
            | Effect::Learn(_)
            | Effect::Settings(_)
            | Effect::Persist(_)
            | Effect::Incident(_)
            | Effect::Notify(_) => {}
        }
        self.effects.push(effect);
    }

    /// Every invariant the world checks, in number order; the first broken one.
    fn check(&self) -> Option<Violation> {
        let queue_max = usize::try_from(self.profile.speech_queue_max).unwrap_or(usize::MAX);
        invariants::bus_events_are_attributed(&self.published, &self.bus.delivered)
            .and_then(|()| {
                invariants::children_restart_with_bounded_backoff(&self.children, self.now_ms)
            })
            .and_then(|()| {
                invariants::utterances_do_not_overlap(
                    &self.published,
                    self.speech_queue.len(),
                    queue_max,
                )
            })
            .and_then(|()| self.collector.check().map_or(Ok(()), Err))
            .err()
    }
}

/// The turn an input belongs to, for its envelope's context.
const fn turn_of(input: &Input) -> Option<eileen_contracts::ids::TurnId> {
    let Input::Voice(event) = input else { return None };
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

/// An input's name in the step log: its variant and, for an enum inside, that variant too, as
/// they are serialized (`voice.speech_started`, `user.set_mute`, `timer`).
#[must_use]
pub fn label_of(input: &Input) -> String {
    let Ok(Value::Object(outer)) = serde_json::to_value(input) else { return "?".to_owned() };
    let Some((name, inner)) = outer.into_iter().next() else { return "?".to_owned() };
    match inner {
        Value::Object(map) => match map.into_iter().next() {
            Some((variant, _)) => format!("{name}.{variant}"),
            None => name,
        },
        Value::String(variant) => format!("{name}.{variant}"),
        Value::Array(items) => match items.into_iter().find_map(|item| match item {
            Value::Object(map) => map.into_iter().next().map(|(variant, _)| variant),
            Value::String(variant) => Some(variant),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
        }) {
            Some(variant) => format!("{name}.{variant}"),
            None => name,
        },
        Value::Null | Value::Bool(_) | Value::Number(_) => name,
    }
}

/// One line of the step log: step, virtual ms, the input, the effects it caused and the
/// invariants' status, in fixed-width columns.
#[must_use]
pub fn format_line(step: u32, now_ms: u64, input: &str, effects: usize, status: &str) -> String {
    let mut column: String = input.chars().take(INPUT_COLUMN).collect();
    while column.len() < INPUT_COLUMN {
        column.push(' ');
    }
    let mut line = String::new();
    // Writing to a String cannot fail.
    let _written: fmt::Result =
        write!(line, "{step:>6} {now_ms:>10} {column} {effects:>4} {status}");
    line
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::{Input, TimerId, UserInput};
    use eileen_contracts::ids::Personal;

    use super::{Bus, World, format_line, label_of};
    use crate::invariants::Delivery;
    use crate::profile::Profile;

    #[test]
    fn the_step_log_has_fixed_columns() {
        let line = format_line(7, 1_234, "voice.speech_started", 0, "ok");
        assert_eq!(
            line, "     7       1234 voice.speech_started                0 ok",
            "step (6), virtual ms (10), input (32), effects (4), status"
        );
        let long = format_line(1, 0, &"x".repeat(40), 12, "FAIL 6");
        assert_eq!(
            long.len(),
            "     1          0 ".len() + 32 + " ".len() + 4 + " FAIL 6".len(),
            "{long}"
        );
    }

    #[test]
    fn inputs_are_labelled_by_their_variants() {
        assert_eq!(label_of(&Input::Timer(TimerId(3))), "timer", "a timer");
        assert_eq!(label_of(&Input::User(UserInput::SetMute(true))), "user.set_mute", "mute");
        assert_eq!(
            label_of(&Input::User(UserInput::Typed { text: Personal::new("hi".to_owned()) })),
            "user.typed",
            "typed text"
        );
        assert_eq!(label_of(&Input::User(UserInput::ToggleMute)), "user.toggle_mute", "toggle");
    }

    #[test]
    fn a_full_bus_drops_events_and_says_so_before_the_next() {
        let mut bus = Bus::new(2, 0);
        for seq in 1..=4 {
            bus.publish(seq);
        }
        assert_eq!(bus.missed, 2, "two events found the queue full");
        bus.drain = 2;
        bus.take();
        bus.publish(5);
        bus.drain = 2;
        bus.take();
        assert_eq!(
            bus.delivered,
            [Delivery::Event(1), Delivery::Event(2), Delivery::Lagged(2), Delivery::Event(5)],
            "the notice comes before the next event delivered"
        );
    }

    #[test]
    fn timers_fire_when_virtual_time_reaches_them() {
        let mut world = World::new(Profile::default_profile().unwrap(), 1);
        world.timers.insert(9, eileen_contracts::ids::MonoTime(50_000_000));
        assert_eq!(world.advance(10), None, "10 ms: not yet");
        assert_eq!(world.advance(40), Some(Input::Timer(TimerId(9))), "50 ms: due");
        assert_eq!(world.advance(100), None, "fired once");
    }

    #[test]
    fn a_step_keeps_every_invariant() {
        let mut world = World::new(Profile::default_profile().unwrap(), 1);
        let outcome = world.step(Input::User(UserInput::ToggleMute));
        assert_eq!(outcome.violation, None, "{outcome:?}");
        assert!(outcome.line.ends_with(" ok"), "{}", outcome.line);
        assert_eq!(world.inputs().len(), 1, "the input is recorded");
    }
}

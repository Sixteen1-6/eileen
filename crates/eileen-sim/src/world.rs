//! The `World`: the real core among simulated parts, in virtual time.
//!
//! Each step takes one input, wraps it in an [`Envelope`] with the next `seq` and the virtual
//! time, calls `Core::step`, applies the effects to the simulated parts, checks every
//! invariant, writes one line of the step log and feeds the trace hash. The parts are small
//! now and grow with WP1: timers the core set, a speech process that queues utterances, the
//! bus with one slow subscriber, and the flight recorder's ring, which captures every span and
//! log line the core produces on the simulator's thread.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::{self, Write as _};
use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::bus::BusEvent;
use eileen_contracts::config::{Config, Proposal, SettingChange, SettingSpecView, ValidationError};
use eileen_contracts::core_io::{Effect, Input, PlaybackEvent, TimerId};
use eileen_contracts::ids::{Envelope, MonoTime, SpeakId, TraceCtx, WallTime};
use eileen_contracts::ports::SettingsValidator;
use eileen_contracts::record::InputRecord;
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};
use eileen_core::{Core, CoreConfig, CorePorts};
use eileen_stdx::rng::CoreRng;
use eileen_trace::redact::Redactor;
use eileen_trace::ring::{Ring, RingLayer};
use serde_json::Value;
use tracing::subscriber::DefaultGuard;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::hash::TraceHash;
use crate::invariants::{self, Delivery, Violation};
use crate::profile::Profile;

/// The wall clock the world starts at: 2026-01-01T00:00:00Z, in microseconds. Wall time only
/// stamps envelopes; nothing in a run depends on today's date.
const EPOCH_US: i64 = 1_767_225_600_000_000;
/// Nanoseconds in a millisecond, for `MonoTime`.
const NS_PER_MS: u64 = 1_000_000;
/// The width of the step log's input column.
const INPUT_COLUMN: usize = 32;
/// The process the ring records as, the core's.
const PROCESS: &str = "eileen";
/// The ring's bounds: far above what a run of the skeleton produces.
const RING_BYTES: usize = 64 * 1024 * 1024;
const RING_WINDOW: Duration = Duration::from_hours(24);

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
    speech_queue: Vec<SpeakId>,
    published: Vec<Envelope<BusEvent>>,
    bus: Bus,
    inputs: Vec<InputRecord>,
    log: String,
    hash: TraceHash,
    ring: Arc<Ring>,
    redactor: Redactor,
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
    /// A world around a fresh stub core, its random numbers seeded from `seed`. Spans and log
    /// lines on this thread go to the world's ring until it is dropped.
    #[must_use]
    pub fn new(profile: Profile, seed: u64) -> Self {
        let ports = CorePorts {
            resolver: Box::new(eileen_intent::Resolver),
            cleaner: Box::new(eileen_dictation::Cleaner),
            settings: Box::new(SimSettings),
            skills: Box::new(eileen_skills::Engine),
        };
        let core = Core::new(CoreConfig::default(), ports, None);
        let ring = Arc::new(Ring::new(PROCESS, RING_BYTES, RING_WINDOW));
        let subscriber = tracing_subscriber::registry().with(RingLayer::new(Arc::clone(&ring)));
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
            speech_queue: Vec::new(),
            published: Vec::new(),
            bus,
            inputs: Vec::new(),
            log: String::new(),
            hash: TraceHash::default(),
            ring,
            redactor: Redactor::new(),
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

    /// Moves virtual time forward by `ms` and returns the first timer that is now due, which
    /// the next step should deliver.
    pub fn advance(&mut self, ms: u64) -> Option<TimerId> {
        self.now_ms = self.now_ms.saturating_add(ms);
        let now = self.mono();
        let due = self.timers.iter().find(|(_, at)| **at <= now).map(|(id, _)| *id)?;
        self.timers.remove(&due);
        Some(TimerId(due))
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

    fn apply_input(&mut self, input: &Input) {
        if let Input::Playback(PlaybackEvent::Drained { speak }) = input {
            self.speech_queue.retain(|queued| queued != speak);
        }
    }

    fn apply_effect(&mut self, effect: Envelope<Effect>) {
        let Envelope { seq, at, mono, ctx, body } = effect;
        match body {
            Effect::Emit(event) => {
                self.bus.publish(seq);
                self.published.push(Envelope { seq, at, mono, ctx, body: event });
            }
            Effect::Speech(SpeechCmd::Speak { speak, .. }) => self.speech_queue.push(speak),
            Effect::Speech(SpeechCmd::Cancel { speak }) => {
                self.speech_queue.retain(|queued| *queued != speak);
            }
            Effect::SetTimer { id, at } => {
                self.timers.insert(id.0, at);
            }
            Effect::CancelTimer(id) => {
                self.timers.remove(&id.0);
            }
            // The parts that answer the rest (audio, the brain, workers, sidecars, the store,
            // the HUD) arrive with WP1; until then their effects are counted and hashed only.
            Effect::Speech(_)
            | Effect::Audio(_)
            | Effect::Brain(_)
            | Effect::Worker(..)
            | Effect::Hook(..)
            | Effect::ToolForward { .. }
            | Effect::ToolReply { .. }
            | Effect::Sidecar(..)
            | Effect::Hud(_)
            | Effect::Learn(_)
            | Effect::Settings(_)
            | Effect::Persist(_)
            | Effect::Incident(_)
            | Effect::Notify(_) => {}
        }
    }

    /// Every invariant the skeleton checks, in number order; the first broken one.
    fn check(&self) -> Option<Violation> {
        let queue_max = usize::try_from(self.profile.speech_queue_max).unwrap_or(usize::MAX);
        let records = self.ring.records(&self.redactor);
        invariants::bus_events_are_attributed(&self.published, &self.bus.delivered)
            .and_then(|()| {
                invariants::utterances_do_not_overlap(
                    &self.published,
                    self.speech_queue.len(),
                    queue_max,
                )
            })
            .and_then(|()| invariants::spans_follow_the_table(&records.spans, &records.events))
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
        assert_eq!(world.advance(40), Some(TimerId(9)), "50 ms: due");
        assert_eq!(world.advance(100), None, "fired once");
    }

    #[test]
    fn a_step_against_the_stub_core_keeps_every_invariant() {
        let mut world = World::new(Profile::default_profile().unwrap(), 1);
        let outcome = world.step(Input::User(UserInput::ToggleMute));
        assert_eq!(outcome.violation, None, "{outcome:?}");
        assert!(outcome.line.ends_with(" ok"), "{}", outcome.line);
        assert_eq!(world.inputs().len(), 1, "the input is recorded");
    }
}

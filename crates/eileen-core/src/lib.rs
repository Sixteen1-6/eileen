//! Eileen's decisions (BUILD.md sections 2.2 to 2.12 and 3.3.3): one state machine with one entry
//! point, [`Core::step`], that turns every [`Input`] into [`Effect`]s and performs no IO. This is
//! the minimal core that slice S1 wires (section 6.3, "First pull request"); the full
//! `Core::step` grows from it.
//!
//! # Owns
//!
//! - [`Core`]: turn-taking and barge-in (pause, then decide, then resume or stop), the addressee
//!   gate's minimal form (the name, the conversation window, a full built-in command), fast-path
//!   dispatch of mute, unmute, stop, cancel, "go on" and "what time is it", one brain turn at a
//!   time spoken clause by clause, one job with a cold worker session, mute, and the completion
//!   announcement. Later: the rest of section 6.3's list.
//! - [`turn`]: the pure, cloneable turn-taking machine `Core::step` delegates to.
//! - [`CorePorts`] and [`CoreConfig`]: what the runtime hands the core at start, with
//!   [`LocalTime`], the PC's offsets from UTC and when they change.
//! - The `shell_command` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Any IO, clock or thread: drivers turn the world into inputs and carry out effects
//!   (`eileen-runtime`). The resolver, cleaner, settings validator and skill engine arrive through
//!   the ports of `eileen-contracts::ports`, implemented by WP7's and WP4's crates.
//!
//! # Invariants
//!
//! - No IO: the crate depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace` among
//!   Eileen's crates (check 24), plus `tracing` for spans and `serde_json` for tool arguments,
//!   and has no build dependencies (decision D84); time comes in as [`MonoTime`] and randomness as [`CoreRng`] (`disallowed-methods` bans the
//!   system clock and generated IDs). IDs are minted from `MonoTime` and the RNG.
//! - Maps that can reach an effect are ordered (`disallowed-types` in
//!   `crates/eileen-core/clippy.toml` bans `HashMap`).
//! - One utterance holds the floor at a time; the announcement queue is bounded (invariant 10).
//! - No unaddressed speech becomes a bus event, and `Heard` follows `TurnRouted` (invariant 6).
//! - Barge-in ducks at the first window, pauses at the third consecutive one and resolves within
//!   600 ms of the pause (invariant 2); the brain is told exactly the heard prefix (invariant 3).
//! - Nothing is spoken and no earcon plays while muted (invariant 9).
//! - A worker's `PreToolUse` is never allowed: no decision only for file tools whose every path
//!   is in the job's folders, a deny with a do-not-retry reason for everything else, until W2's
//!   permission gate replaces this.
//! - The span-parent table holds for every span (invariant 13).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime` (one task owns the core), `eileen-replay` and `eileen-sim`.
//! - May depend on `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Must not depend on any other Eileen crate, a native library or anything that can hang
//!   (check 24), or on rmcp, nor know vendor CLI flags (section 3.1).
//! - Wire types: none of its own; inputs, effects and the snapshot are contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-core`.
//! - Layers: unit tests per module with a scripted resolver (`tests`), exhaustive turn-taking
//!   (`turn::exhaustive`, next), the simulator's slice tests (`eileen-sim::it::slice`), seeds
//!   and regressions, and scenario replay.
//! - A bug here starts as a failing simulator seed or a scenario whose expectation broke.
//!
//! # Traps
//!
//! - A brain reply is one utterance: each clause cut from the streamed text goes to the speech
//!   process as a `SpeechCmd::Speak` naming the same `SpeakId`, numbered on from the last, and
//!   it ends once it is closed and drained. A `Drained` while the reply is still open means
//!   synthesis is behind, not that Eileen finished.
//! - The heard prefix is mapped from the samples handed to the device in proportion to time,
//!   by the core's own clause text, not by the speech process's character counts.
//! - A worker session's `SessionSpec` leaves its hook and shim endpoints empty: the runtime
//!   mints their tokens at spawn. Its working folder is `run/<session>/cwd`, relative to the
//!   data directory.
//! - The local time is the offset in force at each step's wall time, from [`LocalTime`]'s
//!   changes, which the runtime reads at start for the current year and the next: a
//!   daylight-saving change is followed with no restart, a change of time zone only from the
//!   next start.
//! - Effects carry the core's own `seq`, one counter for every effect, kept in the snapshot.
//!
//! # Logs and tracing
//!
//! - Spans `turn`, `addressee`, `router.decide`, `fastpath.run`, `brain.turn`, `job`,
//!   `worker.session`, `speak`, `audio.play` and `bargein`, from `eileen_trace::names`. No
//!   transcript, spoken text or model text goes into a span or a log line.
//!
//! # Do not change without a decision record
//!
//! - The signatures of [`Core::new`], [`Core::step`] and [`Core::snapshot`] (section 3.3.3), and
//!   the snapshot's version.

#![forbid(unsafe_code)]

mod addressee;
mod announce;
mod brain;
mod clauses;
mod clock;
#[doc(hidden)]
pub mod fuzz_entry;
mod job;
mod route;
mod spans;
mod speech;
pub mod turn;

use std::collections::BTreeMap;
use std::fmt;

use eileen_contracts::bus::{BusEvent, CancelSource};
use eileen_contracts::config::Config;
use eileen_contracts::core_io::{Effect, Input, PlaybackEvent, TimerId, UserInput};
use eileen_contracts::ids::{Envelope, JobId, MonoTime, SpeakId, TraceCtx, TurnId, WallTime};
use eileen_contracts::jobs::{JobKind, TurnSource};
use eileen_contracts::ports::{DictationCleaner, IntentResolver, SettingsValidator, SkillEngine};
use eileen_contracts::record::CoreSnapshotV1;
use eileen_contracts::speech::{VoiceCalibration, VoiceEvent};
use eileen_stdx::rng::CoreRng;

pub use crate::clock::{LocalOffset, LocalTime};

use crate::addressee::Window;
use crate::announce::Queue;
use crate::brain::Brain;
use crate::job::Jobs;
use crate::spans::Spans;
use crate::turn::Turns;

/// The ports the core calls inside [`Core::step`] (section 3.3.15).
pub struct CorePorts {
    /// The intent resolver (`eileen_intent::Resolver`).
    pub resolver: Box<dyn IntentResolver>,
    /// The dictation cleaner (`eileen_dictation::Cleaner`).
    pub cleaner: Box<dyn DictationCleaner>,
    /// The settings validator (`eileen_config::Validator`).
    pub settings: Box<dyn SettingsValidator>,
    /// The skill engine (`eileen_skills::Engine`).
    pub skills: Box<dyn SkillEngine>,
}

impl fmt::Debug for CorePorts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CorePorts").finish_non_exhaustive()
    }
}

/// What the runtime read from disk at start. WP1 adds `machine` and `speaker_bar` (section
/// 3.3.17) once their contract types exist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreConfig {
    /// The configuration (section 3.3.11).
    pub config: Config,
    /// `voices\<user>\calibration.toml`, when the first run measured one.
    pub calibration: Option<VoiceCalibration>,
    /// The measured peak working set per job kind, in MB (section 2.5).
    pub cost_estimates: BTreeMap<JobKind, u32>,
    /// The PC's offsets from UTC and when they change, which the runtime read from the time
    /// zone's rules at start: the fast path's "what time is it" speaks the offset in force at
    /// the turn's wall time.
    pub local_time: LocalTime,
}

/// The kinds of timer the core sets; each has at most one live timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimerKind {
    /// The barge-in decision.
    BargeIn,
    /// The conversation window closing.
    Window,
    /// An announcement's second of quiet.
    Announce,
}

/// The core's timers.
#[derive(Debug, Default)]
struct Timers {
    next: u64,
    live: BTreeMap<TimerKind, (TimerId, MonoTime)>,
}

impl Timers {
    /// Sets the timer of `kind` for `at`, replacing a live one; nothing when it is set already.
    fn set(&mut self, cx: &mut Cx<'_>, kind: TimerKind, at: MonoTime) {
        if let Some((id, when)) = self.live.get(&kind).copied() {
            if when == at {
                return;
            }
            cx.push(TraceCtx::default(), Effect::CancelTimer(id));
        }
        self.next = self.next.saturating_add(1);
        let id = TimerId(self.next);
        self.live.insert(kind, (id, at));
        cx.push(TraceCtx::default(), Effect::SetTimer { id, at });
    }

    /// The kind of the timer `id`, which has fired.
    fn fired(&mut self, id: TimerId) -> Option<TimerKind> {
        let kind = self.live.iter().find(|(_, (live, _))| *live == id).map(|(kind, _)| *kind)?;
        self.live.remove(&kind);
        Some(kind)
    }
}

/// One step's context: the time, the random numbers and where effects go.
struct Cx<'a> {
    now: MonoTime,
    at: WallTime,
    rng: &'a mut CoreRng,
    effects: &'a mut Vec<Envelope<Effect>>,
    seq: u64,
}

impl Cx<'_> {
    /// Appends an effect with the next `seq`.
    fn push(&mut self, ctx: TraceCtx, effect: Effect) {
        self.seq = self.seq.saturating_add(1);
        self.effects.push(Envelope {
            seq: self.seq,
            at: self.at,
            mono: self.now,
            ctx,
            body: effect,
        });
    }

    /// Publishes a bus event.
    fn emit(&mut self, ctx: TraceCtx, event: BusEvent) {
        self.push(ctx, Effect::Emit(event));
    }

    /// Virtual milliseconds, the timestamp part of the IDs the core mints.
    const fn ms(&self) -> u64 {
        self.now.0 / 1_000_000
    }

    /// 80 random bits for an ID.
    fn random(&mut self) -> u128 {
        self.rng.ulid_random()
    }
}

/// A turn's correlation.
const fn turn_ctx(turn: TurnId) -> TraceCtx {
    TraceCtx { turn: Some(turn), job: None, call: None }
}

/// A job's correlation.
const fn job_ctx(job: JobId) -> TraceCtx {
    TraceCtx { turn: None, job: Some(job), call: None }
}

/// The utterance holding the floor and what it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Current {
    speak: SpeakId,
    turn: Option<TurnId>,
    job: Option<JobId>,
}

/// What Eileen last said, for the follow-up feature.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LastSpoken {
    at: MonoTime,
    words: Vec<String>,
    job: Option<JobId>,
}

/// The core: every decision Eileen makes, as one state machine.
pub struct Core {
    config: CoreConfig,
    ports: CorePorts,
    turns: Turns,
    window: Window,
    /// The latest partial transcript of the turn being heard.
    partial: Option<(TurnId, String)>,
    current: Option<Current>,
    last_spoken: Option<LastSpoken>,
    brain: Brain,
    jobs: Jobs,
    announcements: Queue,
    timers: Timers,
    spans: Spans,
    seq: u64,
    /// The snapshot the core started from, for the fields it does not run yet.
    resumed: CoreSnapshotV1,
}

impl fmt::Debug for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Core")
            .field("phase", &self.turns.phase())
            .field("muted", &self.turns.muted())
            .field("seq", &self.seq)
            .finish_non_exhaustive()
    }
}

impl Core {
    /// A core with this configuration and these ports, resumed from `snapshot` when given.
    #[must_use]
    pub fn new(config: CoreConfig, ports: CorePorts, snapshot: Option<CoreSnapshotV1>) -> Self {
        let resumed = snapshot.unwrap_or_else(fresh_snapshot);
        let counter = |name: &str| resumed.counters.get(name).copied().unwrap_or(0);
        let timers = Timers { next: counter(COUNTER_TIMERS), live: BTreeMap::new() };
        Self {
            config,
            ports,
            turns: Turns::new(resumed.muted),
            window: Window::resumed(resumed.conversation_until),
            partial: None,
            current: None,
            last_spoken: None,
            brain: Brain::default(),
            jobs: Jobs::resumed(resumed.jobs.clone()),
            announcements: Queue::default(),
            timers,
            spans: Spans::default(),
            seq: counter(COUNTER_SEQ),
            resumed,
        }
    }

    /// Takes one input at `now` and appends the effects it causes.
    pub fn step(
        &mut self,
        now: MonoTime,
        rng: &mut CoreRng,
        input: Envelope<Input>,
        effects: &mut Vec<Envelope<Effect>>,
    ) {
        let Envelope { at, ctx, body, .. } = input;
        let mut cx = Cx { now, at, rng, effects, seq: self.seq };
        match body {
            Input::Voice(event) => self.voice(&mut cx, event),
            Input::Playback(event) => self.playback(&mut cx, event),
            Input::Brain(event) => self.brain_event(&mut cx, event),
            Input::Worker(session, event) => self.worker_event(&mut cx, session, event),
            Input::Hook(session, request) => self.hook(&mut cx, session, request),
            Input::ToolCall(request) => self.tool_call(&mut cx, ctx, request),
            Input::User(input) => self.user(&mut cx, input),
            Input::Timer(id) => self.timer(&mut cx, id),
            Input::ToolResult(..)
            | Input::Sidecar(..)
            | Input::Learn(_)
            | Input::Activity(_)
            | Input::Memory(_)
            | Input::Settings(_)
            | Input::Store(_) => {}
        }
        self.announce(&mut cx);
        self.seq = cx.seq;
    }

    /// The state to checkpoint and replay from (section 3.3.16), built from the live state.
    #[must_use]
    pub fn snapshot(&self) -> CoreSnapshotV1 {
        let mut snapshot = self.resumed.clone();
        snapshot.v = 1;
        snapshot.jobs = self.jobs.all().to_vec();
        snapshot.muted = self.turns.muted();
        snapshot.conversation_until = self.window.closes_at();
        snapshot.counters.insert(COUNTER_SEQ.to_owned(), self.seq);
        snapshot.counters.insert(COUNTER_TIMERS.to_owned(), self.timers.next);
        snapshot
    }

    /// The turn-taking state, for tests and the exhaustive enumeration.
    #[must_use]
    pub const fn turns(&self) -> &Turns {
        &self.turns
    }

    fn voice(&mut self, cx: &mut Cx<'_>, event: VoiceEvent) {
        match event {
            VoiceEvent::SpeechStarted { turn, .. } => self.turns.speech_started(turn),
            VoiceEvent::Partial { turn, text, .. } => {
                if !self.turns.muted() {
                    self.partial = Some((turn, text.into_inner()));
                }
            }
            VoiceEvent::EndOfTurn { turn, text, .. } => self.end_of_turn(cx, turn, text.get()),
            VoiceEvent::Calibration(calibration) => self.config.calibration = calibration,
            VoiceEvent::BargeInWindow { speak, window, .. } => {
                let mut out = Vec::new();
                self.turns.barge_in_window(cx.now, speak, window, &mut out);
                self.apply(cx, out);
            }
            VoiceEvent::ClauseReady { speak, clause, samples, .. } => {
                self.turns.clause_ready(speak, clause, samples);
            }
            VoiceEvent::SpeakDone { speak } => {
                let mut out = Vec::new();
                self.turns.speak_done(speak, &mut out);
                self.apply(cx, out);
            }
            VoiceEvent::SpeechEnded { .. }
            | VoiceEvent::Embedding { .. }
            | VoiceEvent::CheckResult { .. }
            | VoiceEvent::SpeakerMatch { .. }
            | VoiceEvent::Health { .. } => {}
        }
    }

    fn playback(&mut self, cx: &mut Cx<'_>, event: PlaybackEvent) {
        let mut out = Vec::new();
        match event {
            PlaybackEvent::Played { speak, clause, samples_played }
            | PlaybackEvent::Paused { speak, clause, samples_played } => {
                self.turns.played(speak, clause, samples_played);
            }
            PlaybackEvent::Stopped { speak, clause, samples_played } => {
                self.turns.device_stopped(speak, clause, samples_played, &mut out);
            }
            PlaybackEvent::Drained { speak } => self.turns.drained(speak, &mut out),
            PlaybackEvent::Stalled => self.turns.stalled(&mut out),
            PlaybackEvent::DeviceChanged => {}
        }
        self.apply(cx, out);
    }

    fn user(&mut self, cx: &mut Cx<'_>, input: UserInput) {
        match input {
            UserInput::SetMute(muted) => self.set_mute(cx, muted),
            UserInput::ToggleMute => {
                let muted = !self.turns.muted();
                self.set_mute(cx, muted);
            }
            UserInput::Typed { text } => self.typed(cx, text.get(), TurnSource::Typed),
            UserInput::Cli { text } => self.typed(cx, text.get(), TurnSource::Cli),
            UserInput::CancelJob(id) => {
                self.cancel_job(cx, Some(&job::by_id(id)), CancelSource::Hud);
            }
            UserInput::Approve(_)
            | UserInput::Deny(_)
            | UserInput::PauseDesktopJobs
            | UserInput::StartAnyway(_)
            | UserInput::Hud { .. }
            | UserInput::DictationKey(_)
            | UserInput::SaveIncident
            | UserInput::SaveScenario => {}
        }
    }

    fn timer(&mut self, cx: &mut Cx<'_>, id: TimerId) {
        match self.timers.fired(id) {
            Some(TimerKind::BargeIn) => self.barge_in_deadline(cx),
            Some(TimerKind::Window) => self.window_deadline(cx),
            Some(TimerKind::Announce) | None => {}
        }
    }
}

/// The snapshot counter of effect sequence numbers.
const COUNTER_SEQ: &str = "effect_seq";
/// The snapshot counter of timer IDs.
const COUNTER_TIMERS: &str = "timer_ids";

/// The snapshot of a core that has never run.
fn fresh_snapshot() -> CoreSnapshotV1 {
    CoreSnapshotV1 { v: 1, ..CoreSnapshotV1::default() }
}

#[cfg(test)]
mod tests;

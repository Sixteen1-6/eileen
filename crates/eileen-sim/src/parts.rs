//! The simulated parts that answer the core's effects (BUILD.md section 4.7): the speech
//! process, the playback device, a scripted brain and scripted workers. Each part turns an
//! effect into inputs scheduled on [`Due`] at virtual times; the world delivers them in time
//! order.
//!
//! - The speech process answers `Speak` with one `ClauseReady` per clause, a synthesis time
//!   apart, then `SpeakDone`; `Cancel` drops what it has not delivered. It answers the core's
//!   `Start` and `Restart` with `Ready` after a load, and the audio component's pings with a
//!   `Heartbeat` every second while it is ready; `HangDumpThenKill` ends it with `Exited`. A
//!   test or a fault can hang it (no more heartbeats or clauses), crash it (`Exited` at once),
//!   slow its next load or make its loads never finish. While it is not ready the audio
//!   component answers `Speak` with `SpeakDone` and no clause.
//! - The device plays the clauses whose `ClauseReady` was delivered, in 10 ms callbacks of
//!   240 samples (24 kHz), reports `Played` at most every 50 ms, and honours `Duck`, `Pause`
//!   (with a `Paused` report), `Resume` and `Stop` (with a `Stopped` report). It reports
//!   `Drained` when every sample it was given has played.
//! - The brain runs one process per slot, each with its own instance number, which it stamps on
//!   everything it sends: the primary from the start as instance 1, and each `Start`,
//!   `Recycle` and `Promote` a new one with the next number and its `Ready`; `Stop` ends one
//!   with `Exited`. A slot answers each `Turn` sent to it with the next scripted reply (text in
//!   deltas, an optional `jobs_dispatch` call, `TurnDone`), after a delay the profile draws
//!   ([`BrainTurn`]); `Interrupt` drops the rest of the slot's turn. A stalled turn sends
//!   nothing for the stall, then its whole answer; its process hears no interrupt until then,
//!   and a later turn to the same slot waits for it, as a stuck process would, while the other
//!   slots answer at once. A failed turn ends in an error with no text.
//! - A worker session answers `Start` with `Init`, `Bind` with one `PreToolUse` for a write in
//!   its job folder and then `TurnDone` (later when slow, or `Exited` part way when it
//!   crashes), and `Cancel` with `Exited`.

use std::collections::{BTreeMap, VecDeque};

use eileen_contracts::connectors::{
    ConnectorManifest, Platform, RunsIn, SidecarCmd, SidecarEvent, SidecarId,
};
use eileen_contracts::core_io::{
    AudioCmd, Caller, Input, PathAccess, PathClass, PathFact, PlaybackEvent, ShimRole,
    ToolCallRequest,
};
use eileen_contracts::harness::{
    BrainCmd, BrainEvent, BrainSlot, HookRequest, WorkerCmd, WorkerEvent,
};
use eileen_contracts::ids::{
    ConnectorId, Personal, SpeakId, ToolName, TurnId, Untrusted, WorkerSessionId,
};
use eileen_contracts::jobs::{CallOrigin, Cost, Route, TurnAuthority};
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};
use serde_json::{Value, json};

use crate::invariants::Handed;

/// The device's callback period, ms.
pub const CALLBACK_MS: u64 = 10;
/// Samples the device takes per callback: 10 ms at 24 kHz.
const SAMPLES_PER_CALLBACK: u32 = 240;
/// How often the device reports `Played`, ms.
const REPORT_MS: u64 = 50;
/// Samples synthesized per character of a clause: about 15 characters a second at 24 kHz.
const SAMPLES_PER_CHAR: u32 = 1_600;
/// How long the speech process takes to synthesize one clause, ms.
const SYNTH_MS: u64 = 60;
/// How long the speech process takes from `Start` or `Restart` to `Ready`, ms.
pub const LOAD_MS: u64 = 2_000;
/// How often the audio component pings the speech process, ms.
pub const HEARTBEAT_MS: u64 = 1_000;
/// How long the supervisor takes to report the exit of a process the guard killed, ms,
/// unless a test makes the hang dump slower.
const EXIT_MS: u64 = 50;
/// The exit code of a crashed speech process.
const CRASH_CODE: i32 = 1;
/// How long the brain takes to its first text, and between deltas, ms, unless the profile
/// says otherwise.
const BRAIN_FIRST_MS: u64 = 800;
const BRAIN_DELTA_MS: u64 = 150;
/// How long after an interrupt the brain reports the turn done, and after a stop or a kill
/// the process's exit, ms.
const BRAIN_INTERRUPT_MS: u64 = 50;
/// How long a brain process the core starts takes to load and send `Ready`, ms.
pub const BRAIN_START_MS: u64 = 2_000;
/// The model every simulated brain process reports.
const BRAIN_MODEL: &str = "sim-brain";
/// How long a worker session takes to start, to ask its first hook and to finish, ms.
const WORKER_START_MS: u64 = 500;
const WORKER_HOOK_MS: u64 = 300;
const WORKER_DONE_MS: u64 = 1_200;
/// When a crashing worker session exits after its `Bind`, ms: after its hook, before its end.
const WORKER_CRASH_MS: u64 = 600;
/// The exit code of a crashed worker session.
const WORKER_CRASH_CODE: i32 = 1;
/// How much longer a slow load of the speech process takes, ms (the fault
/// [`crate::faults::SPEECH_SLOW_LOAD`]): well inside the core's 120 s.
pub const SLOW_LOAD_MS: u64 = 10_000;
/// How much longer a slow worker session takes over its job, ms (the fault
/// [`crate::faults::WORKER_SLOW`]).
pub const SLOW_WORKER_MS: u64 = 5_000;

/// Inputs the parts scheduled, in time order (ties in the order scheduled).
#[derive(Debug, Default)]
pub struct Due {
    items: BTreeMap<(u64, u64), Input>,
    order: u64,
}

impl Due {
    /// Schedules `input` for virtual time `at_ms`.
    pub fn schedule(&mut self, at_ms: u64, input: Input) {
        self.order = self.order.saturating_add(1);
        self.items.insert((at_ms, self.order), input);
    }

    /// When the next item is due.
    #[must_use]
    pub fn next_at(&self) -> Option<u64> {
        self.items.keys().next().map(|(at, _)| *at)
    }

    /// The next item due at or before `now_ms`.
    pub fn pop(&mut self, now_ms: u64) -> Option<Input> {
        let key = *self.items.keys().next().filter(|(at, _)| *at <= now_ms)?;
        self.items.remove(&key)
    }

    /// Drops every scheduled item `drop` picks.
    pub fn drop_where(&mut self, drop: impl Fn(&Input) -> bool) {
        self.items.retain(|_, input| !drop(input));
    }

    /// How many items wait.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether nothing waits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Where the simulated speech process stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Process {
    /// Not started.
    #[default]
    Off,
    /// Loading its models after a `Start` or `Restart`.
    Loading,
    /// Ready: it synthesizes, and the audio component's pings come back as heartbeats.
    Up,
    /// Hung: it answers nothing until it is killed.
    Hung,
    /// Ended, by a crash or a kill.
    Dead,
}

/// The simulated speech process: synthesis and its life (recognition comes from the
/// generator).
#[derive(Debug, Default)]
pub struct Speech {
    /// The next clause number of each utterance, and when its synthesis is free.
    utterances: BTreeMap<SpeakId, (u16, u64)>,
    process: Process,
    /// Its loads never finish: no `Ready` comes (a fault a test injects).
    stuck_loads: bool,
    /// How long a kill's `Exited` takes instead of [`EXIT_MS`] (a slow hang dump a test
    /// injects).
    exit_ms: Option<u64>,
    /// The next load takes [`SLOW_LOAD_MS`] longer (a fault).
    slow_load: bool,
}

/// What the speech process sends that stops when it ends or hangs: clauses, `SpeakDone`,
/// `Ready` and heartbeats.
const fn from_the_process(input: &Input) -> bool {
    matches!(
        input,
        Input::Voice(VoiceEvent::ClauseReady { .. } | VoiceEvent::SpeakDone { .. })
            | Input::Sidecar(
                SidecarId::Speech,
                SidecarEvent::Ready { .. } | SidecarEvent::Heartbeat
            )
    )
}

/// The speech process's `Ready`. The speech process serves no MCP, so the transport fills its
/// manifest, with no tools.
fn ready() -> Option<Input> {
    let manifest = ConnectorManifest {
        id: ConnectorId::new("pc.speech").ok()?,
        version: "0.1.0".to_owned(),
        runs: RunsIn::Sidecar { binary: "eileen-speech".to_owned() },
        platforms: vec![Platform::Windows, Platform::Linux],
        tools: Vec::new(),
        events: Vec::new(),
        checks: Vec::new(),
        senses_room: false,
        requires_opt_in: false,
    };
    Some(Input::Sidecar(SidecarId::Speech, SidecarEvent::Ready { manifest: Box::new(manifest) }))
}

impl Speech {
    /// Where the process stands.
    #[must_use]
    pub const fn process(&self) -> Process {
        self.process
    }

    /// Makes every later load never send `Ready` (`true`), or finish again (`false`).
    pub const fn stick_loads(&mut self, stuck: bool) {
        self.stuck_loads = stuck;
    }

    /// Whether its loads never finish.
    #[must_use]
    pub const fn loads_stuck(&self) -> bool {
        self.stuck_loads
    }

    /// Makes the supervisor report each later kill's `Exited` `ms` after the kill.
    pub const fn slow_exits(&mut self, ms: u64) {
        self.exit_ms = Some(ms);
    }

    /// Makes the next load take [`SLOW_LOAD_MS`] longer than [`LOAD_MS`].
    pub const fn slow_next_load(&mut self) {
        self.slow_load = true;
    }

    /// Answers the core's command for the process: `Start` and `Restart` load it, and
    /// `HangDumpThenKill` ends it.
    pub fn sidecar(&mut self, now_ms: u64, cmd: SidecarCmd, due: &mut Due) {
        match cmd {
            SidecarCmd::Start | SidecarCmd::Restart => {
                due.drop_where(from_the_process);
                self.utterances.clear();
                self.process = Process::Loading;
                let load =
                    if self.slow_load { LOAD_MS.saturating_add(SLOW_LOAD_MS) } else { LOAD_MS };
                self.slow_load = false;
                if !self.stuck_loads
                    && let Some(ready) = ready()
                {
                    due.schedule(now_ms.saturating_add(load), ready);
                }
            }
            SidecarCmd::HangDumpThenKill => {
                if matches!(self.process, Process::Off | Process::Dead) {
                    return;
                }
                self.end(due);
                let exited = Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { code: None });
                due.schedule(now_ms.saturating_add(self.exit_ms.unwrap_or(EXIT_MS)), exited);
            }
        }
    }

    /// The world delivered `input`: a `Ready` makes a loading process ready, and while it is,
    /// each heartbeat brings the next a second later.
    pub fn delivered(&mut self, now_ms: u64, input: &Input, due: &mut Due) {
        let Input::Sidecar(SidecarId::Speech, event) = input else { return };
        let beats = match event {
            SidecarEvent::Ready { .. } if self.process == Process::Loading => {
                self.process = Process::Up;
                true
            }
            SidecarEvent::Heartbeat => self.process == Process::Up,
            SidecarEvent::Ready { .. } | SidecarEvent::Event(_) | SidecarEvent::Exited { .. } => {
                false
            }
        };
        if beats {
            let beat = Input::Sidecar(SidecarId::Speech, SidecarEvent::Heartbeat);
            due.schedule(now_ms.saturating_add(HEARTBEAT_MS), beat);
        }
    }

    /// The process hangs: no heartbeat, clause or `Ready` comes until it is killed.
    pub fn hang(&mut self, due: &mut Due) {
        if matches!(self.process, Process::Loading | Process::Up) {
            due.drop_where(from_the_process);
            self.process = Process::Hung;
        }
    }

    /// The process crashes: the supervisor reports `Exited` at once.
    pub fn crash(&mut self, now_ms: u64, due: &mut Due) {
        if matches!(self.process, Process::Off | Process::Dead) {
            return;
        }
        self.end(due);
        let exited = SidecarEvent::Exited { code: Some(CRASH_CODE) };
        due.schedule(now_ms, Input::Sidecar(SidecarId::Speech, exited));
    }

    /// The process is gone, with everything it had not sent.
    fn end(&mut self, due: &mut Due) {
        due.drop_where(from_the_process);
        self.utterances.clear();
        self.process = Process::Dead;
    }

    /// Answers a speech command. Only a ready process synthesizes; while none is, the audio
    /// component answers `Speak` with `SpeakDone` and no clause, and a hung one answers
    /// nothing.
    pub fn command(&mut self, now_ms: u64, cmd: &SpeechCmd, due: &mut Due) {
        match self.process {
            Process::Up => {}
            Process::Hung => return,
            Process::Off | Process::Loading | Process::Dead => {
                if let SpeechCmd::Speak { speak, .. } = cmd {
                    due.schedule(now_ms, Input::Voice(VoiceEvent::SpeakDone { speak: *speak }));
                }
                return;
            }
        }
        match cmd {
            SpeechCmd::Speak { speak, clauses, .. } => {
                let (next, free) = self.utterances.entry(*speak).or_insert((0, now_ms));
                let mut at = (*free).max(now_ms);
                for clause in clauses {
                    at = at.saturating_add(SYNTH_MS);
                    let chars = u32::try_from(clause.chars().count()).unwrap_or(u32::MAX);
                    let event = VoiceEvent::ClauseReady {
                        speak: *speak,
                        clause: *next,
                        chars,
                        samples: chars.saturating_mul(SAMPLES_PER_CHAR).max(1),
                    };
                    due.schedule(at, Input::Voice(event));
                    *next = next.saturating_add(1);
                }
                *free = at;
                due.schedule(at, Input::Voice(VoiceEvent::SpeakDone { speak: *speak }));
            }
            SpeechCmd::Cancel { speak } => {
                let speak = *speak;
                due.drop_where(|input| {
                    matches!(
                        input,
                        Input::Voice(
                            VoiceEvent::ClauseReady { speak: s, .. } | VoiceEvent::SpeakDone { speak: s }
                        ) if *s == speak
                    )
                });
            }
            SpeechCmd::Resume { .. }
            | SpeechCmd::SetMode(_)
            | SpeechCmd::SetHotwords { .. }
            | SpeechCmd::Embed { .. }
            | SpeechCmd::Check { .. }
            | SpeechCmd::LoadModel { .. }
            | SpeechCmd::UnloadModel { .. }
            | SpeechCmd::Enroll { .. }
            | SpeechCmd::CloneFromRecording { .. }
            | SpeechCmd::DeleteVoice { .. } => {}
        }
    }
}

/// One utterance on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Playing {
    speak: SpeakId,
    /// Clauses given and not started: number and samples.
    queued: VecDeque<(u16, u32)>,
    /// The clause playing: number, samples, samples played.
    current: Option<(u16, u32, u32)>,
    /// The last position reported or reached, for reports between clauses.
    last: (u16, u32),
    paused: bool,
    since_report: u64,
}

/// The simulated playback device.
#[derive(Debug, Default)]
pub struct Device {
    playing: VecDeque<Playing>,
    ducked: bool,
    capture: bool,
}

impl Device {
    /// A device with capture on.
    #[must_use]
    pub const fn new() -> Self {
        Self { playing: VecDeque::new(), ducked: false, capture: true }
    }

    /// Whether other audio is ducked.
    #[must_use]
    pub const fn ducked(&self) -> bool {
        self.ducked
    }

    /// Whether the microphone is captured.
    #[must_use]
    pub const fn capture(&self) -> bool {
        self.capture
    }

    /// The utterances it holds, the one playing first.
    #[must_use]
    pub fn speaking(&self) -> Vec<SpeakId> {
        self.playing.iter().map(|playing| playing.speak).collect()
    }

    /// A clause's samples arrived (its `ClauseReady` was delivered).
    pub fn clause_ready(&mut self, speak: SpeakId, clause: u16, samples: u32) {
        if let Some(playing) = self.playing.iter_mut().find(|p| p.speak == speak) {
            playing.queued.push_back((clause, samples));
            return;
        }
        self.playing.push_back(Playing {
            speak,
            queued: VecDeque::from([(clause, samples)]),
            current: None,
            last: (clause, 0),
            paused: false,
            since_report: 0,
        });
    }

    /// The device forgets an utterance (a `Drained` was delivered).
    pub fn forget(&mut self, speak: SpeakId) {
        self.playing.retain(|playing| playing.speak != speak);
    }

    /// Answers an audio command.
    pub fn command(&mut self, now_ms: u64, cmd: &AudioCmd, due: &mut Due) {
        match cmd {
            AudioCmd::SetCapture(on) => self.capture = *on,
            AudioCmd::Duck { .. } => self.ducked = true,
            AudioCmd::Unduck => self.ducked = false,
            AudioCmd::Pause { speak } => {
                if let Some(playing) = self.playing.iter_mut().find(|p| p.speak == *speak) {
                    playing.paused = true;
                    let (clause, samples_played) = playing.position();
                    let event = PlaybackEvent::Paused { speak: *speak, clause, samples_played };
                    due.schedule(now_ms, Input::Playback(event));
                }
            }
            AudioCmd::Resume { speak } => {
                if let Some(playing) = self.playing.iter_mut().find(|p| p.speak == *speak) {
                    playing.paused = false;
                }
            }
            AudioCmd::Stop { speak } => {
                if let Some(playing) = self.playing.iter().find(|p| p.speak == *speak) {
                    let (clause, samples_played) = playing.position();
                    let event = PlaybackEvent::Stopped { speak: *speak, clause, samples_played };
                    due.schedule(now_ms, Input::Playback(event));
                }
                self.forget(*speak);
            }
            AudioCmd::PlayPhrase { .. } | AudioCmd::Earcon(_) | AudioCmd::Record { .. } => {}
        }
    }

    /// The clause playing and its samples played, or the last position reached, of `speak`
    /// while the device holds it.
    #[must_use]
    pub fn position(&self, speak: SpeakId) -> Option<(u16, u32)> {
        self.playing.iter().find(|playing| playing.speak == speak).map(Playing::position)
    }

    /// One 10 ms callback at `now_ms`: the first utterance takes its samples. Returns the
    /// samples it handed to the device, clause by clause.
    pub fn callback(&mut self, now_ms: u64, due: &mut Due) -> Vec<Handed> {
        let mut handed = Vec::new();
        let Some(playing) = self.playing.front_mut() else { return handed };
        if playing.paused {
            return handed;
        }
        if !playing.take(SAMPLES_PER_CALLBACK, &mut handed) {
            let speak = playing.speak;
            self.playing.pop_front();
            due.schedule(now_ms, Input::Playback(PlaybackEvent::Drained { speak }));
            return handed;
        }
        playing.since_report = playing.since_report.saturating_add(CALLBACK_MS);
        if playing.since_report >= REPORT_MS {
            playing.since_report = 0;
            let (clause, samples_played) = playing.position();
            let speak = playing.speak;
            due.schedule(
                now_ms,
                Input::Playback(PlaybackEvent::Played { speak, clause, samples_played }),
            );
        }
        handed
    }
}

impl Playing {
    /// The clause playing and its samples played, or the last position reached.
    fn position(&self) -> (u16, u32) {
        self.current.map_or(self.last, |(clause, _, played)| (clause, played))
    }

    /// Takes up to `budget` samples, noting them in `handed`; false when nothing was left to
    /// play.
    fn take(&mut self, mut budget: u32, handed: &mut Vec<Handed>) -> bool {
        let mut took = false;
        while budget > 0 {
            let next = self
                .current
                .or_else(|| self.queued.pop_front().map(|(clause, total)| (clause, total, 0)));
            let Some((clause, total, played)) = next else { break };
            let step = budget.min(total.saturating_sub(played));
            budget -= step;
            let played = played.saturating_add(step);
            took = took || step > 0;
            if step > 0 {
                handed.push(Handed { speak: self.speak, clause, samples: step });
            }
            self.last = (clause, played);
            self.current = (played < total).then_some((clause, total, played));
        }
        took
    }
}

/// One scripted brain reply.
#[derive(Debug, Clone, PartialEq)]
pub struct BrainReply {
    /// The text, in the deltas it streams in.
    pub deltas: Vec<String>,
    /// A `jobs_dispatch` call's arguments, sent after the first delta.
    pub dispatch: Option<Value>,
    /// Whether the turn ends in an error.
    pub fail: bool,
    /// How long the brain sends nothing before this answer, ms: a stalled instance, which
    /// hears no interrupt until the stall ends and then sends its whole answer.
    pub stall_ms: Option<u64>,
}

impl BrainReply {
    /// A reply of these deltas.
    #[must_use]
    pub fn text(deltas: &[&str]) -> Self {
        Self {
            deltas: deltas.iter().map(|d| (*d).to_owned()).collect(),
            dispatch: None,
            fail: false,
            stall_ms: None,
        }
    }

    /// The same reply, dispatching a general job named `title` that does `task`.
    #[must_use]
    pub fn dispatching(mut self, title: &str, task: &str) -> Self {
        self.dispatch = Some(json!({ "kind": "general", "task": task, "title_hint": title }));
        self
    }

    /// The same reply after the brain stalls for `ms`.
    #[must_use]
    pub const fn stalling(mut self, ms: u64) -> Self {
        self.stall_ms = Some(ms);
        self
    }
}

/// How the brain answers one turn: its pace, which the profile draws, and the faults that
/// fired for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrainTurn {
    /// Time to the first text, ms.
    pub first_ms: u64,
    /// Time between two deltas, ms.
    pub delta_ms: u64,
    /// A reply nobody scripted dispatches a job.
    pub dispatch: bool,
    /// The turn stalls this long first, unless its scripted reply says otherwise.
    pub stall_ms: Option<u64>,
    /// The turn ends in an error with no text.
    pub fail: bool,
}

impl Default for BrainTurn {
    /// The fixed pace a world a test drives by hand uses, with no fault.
    fn default() -> Self {
        Self {
            first_ms: BRAIN_FIRST_MS,
            delta_ms: BRAIN_DELTA_MS,
            dispatch: false,
            stall_ms: None,
            fail: false,
        }
    }
}

/// One brain process: the instance a slot runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Instance {
    /// Its number, never reused, which it stamps on everything it sends.
    number: u32,
    /// When it has loaded and can answer, ms.
    ready_at: u64,
    /// The turn it answers now, which an interrupt ends.
    live: Option<TurnId>,
    /// Until when it is stuck in a stalled answer, ms: it hears no interrupt before then, and a
    /// later turn's answer starts no sooner.
    busy_until: u64,
}

/// The scripted brain: one process per slot, each with its own instance number. The primary
/// runs from the start as instance 1 (the runtime started it before the core's first step);
/// `Start`, `Recycle` and `Promote` start a process with the next number and send its `Ready`
/// after a load, and `Stop` ends one. A turn is answered by the slot it was sent to, so a stall
/// in one slot holds up only that slot.
#[derive(Debug)]
pub struct Brain {
    script: VecDeque<BrainReply>,
    slots: BTreeMap<BrainSlot, Instance>,
    /// The last instance number given out.
    last: u32,
}

impl Default for Brain {
    fn default() -> Self {
        let primary = Instance { number: 1, ready_at: 0, live: None, busy_until: 0 };
        Self {
            script: VecDeque::new(),
            slots: BTreeMap::from([(BrainSlot::Primary, primary)]),
            last: 1,
        }
    }
}

impl Brain {
    /// Queues the reply to the next turn.
    pub fn script(&mut self, reply: BrainReply) {
        self.script.push_back(reply);
    }

    /// How many brain processes run, each of which holds memory.
    #[must_use]
    pub fn running(&self) -> u32 {
        u32::try_from(self.slots.len()).unwrap_or(u32::MAX)
    }

    /// The instance `slot` runs, if it runs one.
    #[must_use]
    pub fn instance(&self, slot: BrainSlot) -> Option<u32> {
        self.slots.get(&slot).map(|instance| instance.number)
    }

    /// The reply to a turn nobody scripted: a short answer, and a job when the user asked
    /// for a file or `dispatch` says so.
    fn default_reply(message: &str, dispatch: bool) -> BrainReply {
        if dispatch || message.contains("file") {
            BrainReply::text(&["Sure, I will start that. ", "It takes a moment."])
                .dispatching("the hello file", "create hello.txt containing hi")
        } else {
            BrainReply::text(&[
                "Here is a short answer, ",
                "in two clauses. ",
                "And a last sentence.",
            ])
        }
    }

    /// Starts a fresh process in `slot`: the next instance number, its `Ready` after a load.
    fn start(&mut self, now_ms: u64, slot: BrainSlot, due: &mut Due) -> Instance {
        self.last = self.last.saturating_add(1);
        let ready_at = now_ms.saturating_add(BRAIN_START_MS);
        let instance = Instance { number: self.last, ready_at, live: None, busy_until: 0 };
        self.slots.insert(slot, instance);
        let ready = BrainEvent::Ready {
            which: slot,
            instance: instance.number,
            route: Route::Claude,
            model: BRAIN_MODEL.to_owned(),
        };
        due.schedule(ready_at, Input::Brain(ready));
        instance
    }

    /// Ends `instance`, which ran in `slot`: nothing more of it comes, then its `Exited`.
    fn end(now_ms: u64, slot: BrainSlot, instance: Instance, code: Option<i32>, due: &mut Due) {
        due.drop_where(|input| of_instance(input, instance.number, None));
        let exited = BrainEvent::Exited { which: slot, instance: instance.number, code };
        due.schedule(now_ms.saturating_add(BRAIN_INTERRUPT_MS), Input::Brain(exited));
    }

    /// Answers a brain command; `pace` says how a `Turn` is answered.
    pub fn command(&mut self, now_ms: u64, cmd: &BrainCmd, pace: BrainTurn, due: &mut Due) {
        match cmd {
            BrainCmd::Turn { slot, turn, message, authority } => {
                let mut reply = self
                    .script
                    .pop_front()
                    .unwrap_or_else(|| Self::default_reply(message, pace.dispatch));
                if pace.fail {
                    reply.deltas.clear();
                    reply.dispatch = None;
                    reply.fail = true;
                }
                reply.stall_ms = reply.stall_ms.or(pace.stall_ms);
                // A turn sent to a slot with no process starts one first, as the driver would.
                let mut instance = match self.slots.get(slot) {
                    Some(instance) => *instance,
                    None => self.start(now_ms, *slot, due),
                };
                let start = now_ms.max(instance.ready_at).max(instance.busy_until);
                instance.live = Some(*turn);
                let sent = Sent { number: instance.number, turn: *turn, authority: *authority };
                let end = answer(start, sent, &reply, pace, due);
                if reply.stall_ms.is_some() {
                    instance.busy_until = instance.busy_until.max(end);
                }
                self.slots.insert(*slot, instance);
            }
            BrainCmd::Interrupt { slot } => {
                // A stuck process hears nothing: its whole answer still comes, late.
                if let Some(instance) = self.slots.get_mut(slot)
                    && now_ms >= instance.busy_until
                    && let Some(turn) = instance.live.take()
                {
                    let number = instance.number;
                    due.drop_where(|input| of_instance(input, number, Some(turn)));
                    let done = Input::Brain(turn_done(number, turn, false));
                    due.schedule(now_ms.saturating_add(BRAIN_INTERRUPT_MS), done);
                }
            }
            BrainCmd::Recycle { slot, .. } => {
                if let Some(old) = self.slots.remove(slot) {
                    Self::end(now_ms, *slot, old, Some(0), due);
                    self.start(now_ms, *slot, due);
                }
            }
            BrainCmd::Promote { slot } => {
                if *slot == BrainSlot::Primary {
                    return;
                }
                let Some(promoted) = self.slots.remove(slot) else { return };
                if let Some(old) = self.slots.insert(BrainSlot::Primary, promoted) {
                    Self::end(now_ms, BrainSlot::Primary, old, None, due);
                }
                let ready = BrainEvent::Ready {
                    which: BrainSlot::Primary,
                    instance: promoted.number,
                    route: Route::Claude,
                    model: BRAIN_MODEL.to_owned(),
                };
                due.schedule(now_ms.max(promoted.ready_at), Input::Brain(ready));
                self.start(now_ms, *slot, due);
            }
            BrainCmd::Start { slot } => {
                if !self.slots.contains_key(slot) {
                    self.start(now_ms, *slot, due);
                }
            }
            BrainCmd::Stop { slot } => {
                if let Some(old) = self.slots.remove(slot) {
                    Self::end(now_ms, *slot, old, Some(0), due);
                }
            }
        }
    }
}

/// A turn as one brain instance was sent it.
#[derive(Debug, Clone, Copy)]
struct Sent {
    /// The instance that answers it.
    number: u32,
    turn: TurnId,
    authority: TurnAuthority,
}

/// Schedules a reply's deltas, its dispatch and its end from `start_ms` at `pace`, after its
/// stall; returns when it ends.
fn answer(start_ms: u64, sent: Sent, reply: &BrainReply, pace: BrainTurn, due: &mut Due) -> u64 {
    let Sent { number, turn, authority } = sent;
    let stall = reply.stall_ms.unwrap_or(0);
    let mut at = start_ms.saturating_add(stall).saturating_add(pace.first_ms);
    for (index, text) in reply.deltas.iter().enumerate() {
        let text = Untrusted::new(text.clone());
        let delta = BrainEvent::TextDelta { instance: number, turn, text };
        due.schedule(at, Input::Brain(delta));
        if index == 0
            && let Some(call) =
                reply.dispatch.as_ref().and_then(|args| dispatch(number, turn, authority, args))
        {
            due.schedule(at.saturating_add(20), Input::ToolCall(call));
        }
        at = at.saturating_add(pace.delta_ms);
    }
    due.schedule(at, Input::Brain(turn_done(number, turn, reply.fail)));
    at
}

fn turn_done(instance: u32, turn: TurnId, is_error: bool) -> BrainEvent {
    BrainEvent::TurnDone { instance, turn, cost: Cost::default(), context_tokens: 1_000, is_error }
}

/// The brain's `jobs_dispatch` call, over the instance's own shim connection (the tool name
/// always parses).
fn dispatch(
    number: u32,
    turn: TurnId,
    authority: TurnAuthority,
    args: &Value,
) -> Option<ToolCallRequest> {
    Some(ToolCallRequest {
        caller: Caller::Shim { connection: u64::from(number), role: ShimRole::Brain },
        origin: CallOrigin::Brain { turn, authority },
        tool: ToolName::new("jobs_dispatch").ok()?,
        args: Untrusted::new(args.clone()),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    })
}

/// Whether `input` comes from the brain instance `number`, for `turn` when one is named.
fn of_instance(input: &Input, number: u32, turn: Option<TurnId>) -> bool {
    let ours = |t: TurnId| turn.is_none_or(|turn| turn == t);
    let text = matches!(
        input,
        Input::Brain(
            BrainEvent::TextDelta { instance, turn: t, .. }
                | BrainEvent::TurnDone { instance, turn: t, .. }
        ) if *instance == number && ours(*t)
    );
    let ready = turn.is_none()
        && matches!(input, Input::Brain(BrainEvent::Ready { instance, .. }) if *instance == number);
    let shim = Caller::Shim { connection: u64::from(number), role: ShimRole::Brain };
    let call = matches!(
        input,
        Input::ToolCall(request)
            if request.caller == shim
                && matches!(request.origin, CallOrigin::Brain { turn: t, .. } if ours(t))
    );
    text || ready || call
}

/// How a worker session's job goes wrong (the faults [`crate::faults::WORKER_CRASH`] and
/// [`crate::faults::WORKER_SLOW`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkerFault {
    /// It does not.
    #[default]
    None,
    /// The session exits after its hook, before it finishes.
    Crash,
    /// Everything after the `Bind` comes [`SLOW_WORKER_MS`] later.
    Slow,
}

/// The scripted worker sessions.
#[derive(Debug, Default)]
pub struct Workers {
    hooks: u64,
}

impl Workers {
    /// Answers a worker command; `fault` says how a `Bind` goes wrong, if it does.
    pub fn command(
        &mut self,
        now_ms: u64,
        session: WorkerSessionId,
        cmd: &WorkerCmd,
        fault: WorkerFault,
        due: &mut Due,
    ) {
        match cmd {
            WorkerCmd::Start(_) => {
                let init = WorkerEvent::Init {
                    session_id: format!("sim-{session}"),
                    pid: 4_000,
                    model: "sim-model".to_owned(),
                    tools: vec!["Write".to_owned()],
                    mcp_servers: vec!["eileen".to_owned()],
                    plugins: Vec::new(),
                    capabilities: Vec::new(),
                };
                due.schedule(now_ms.saturating_add(WORKER_START_MS), Input::Worker(session, init));
            }
            WorkerCmd::Bind(_) => {
                self.hooks = self.hooks.saturating_add(1);
                let hook = write_hook(self.hooks);
                let late = if fault == WorkerFault::Slow { SLOW_WORKER_MS } else { 0 };
                let at = |ms: u64| now_ms.saturating_add(ms).saturating_add(late);
                due.schedule(at(WORKER_HOOK_MS), Input::Hook(session, hook));
                if fault == WorkerFault::Crash {
                    let exited = WorkerEvent::Exited { code: Some(WORKER_CRASH_CODE) };
                    due.schedule(at(WORKER_CRASH_MS), Input::Worker(session, exited));
                    return;
                }
                let done = WorkerEvent::TurnDone {
                    cost: Cost::default(),
                    is_error: false,
                    summary: Untrusted::new("Created hello.txt with hi.".to_owned()),
                };
                due.schedule(at(WORKER_DONE_MS), Input::Worker(session, done));
            }
            WorkerCmd::Cancel => {
                due.drop_where(|input| of_session(input, session));
                let exited = WorkerEvent::Exited { code: None };
                due.schedule(now_ms.saturating_add(20), Input::Worker(session, exited));
            }
            WorkerCmd::Prompt { .. }
            | WorkerCmd::Interrupt
            | WorkerCmd::Recycle
            | WorkerCmd::RemoveWorktree
            | WorkerCmd::RestoreSnapshot { .. } => {}
        }
    }
}

/// A `PreToolUse` for a write of `hello.txt` in the job's folder.
fn write_hook(request: u64) -> HookRequest {
    HookRequest::PreToolUse {
        request,
        tool_name: "Write".to_owned(),
        tool_input: Untrusted::new(json!({ "file_path": "hello.txt", "content": "hi" })),
        tool_use_id: format!("toolu_sim_{request}"),
        paths: vec![PathFact {
            arg: "file_path".to_owned(),
            class: PathClass::JobFolder,
            access: PathAccess::Write,
            canonical: Personal::new("hello.txt".to_owned()),
            runs_code: false,
            opens_web: false,
            git_component: false,
        }],
        git: Vec::new(),
        urls: Vec::new(),
    }
}

/// Whether `input` comes from the worker session `session`.
fn of_session(input: &Input, session: WorkerSessionId) -> bool {
    matches!(input, Input::Worker(s, _) | Input::Hook(s, _) if *s == session)
}

#[cfg(test)]
mod tests {
    use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
    use eileen_contracts::core_io::{AudioCmd, Input, PlaybackEvent};
    use eileen_contracts::harness::{BrainCmd, BrainEvent, BrainSlot};
    use eileen_contracts::ids::{SpeakId, TurnId};
    use eileen_contracts::jobs::{Route, TurnAuthority};
    use eileen_contracts::speech::{SpeechCmd, VoiceEvent};

    use super::{
        BRAIN_MODEL, BRAIN_START_MS, Brain, BrainReply, BrainTurn, CALLBACK_MS, Device, Due,
        HEARTBEAT_MS, LOAD_MS, Process, SLOW_LOAD_MS, Speech,
    };

    const fn speak() -> SpeakId {
        SpeakId::from_parts(1, 1)
    }

    fn drain(due: &mut Due, now_ms: u64) -> Vec<Input> {
        std::iter::from_fn(|| due.pop(now_ms)).collect()
    }

    /// Delivers what is due by `now_ms` back to the speech process, as the world does.
    fn deliver(speech: &mut Speech, due: &mut Due, now_ms: u64) -> Vec<Input> {
        let inputs = drain(due, now_ms);
        for input in &inputs {
            speech.delivered(now_ms, input, due);
        }
        inputs
    }

    /// A speech process that loaded and is ready, with nothing scheduled.
    fn ready(due: &mut Due) -> Speech {
        let mut speech = Speech::default();
        speech.sidecar(0, SidecarCmd::Start, due);
        deliver(&mut speech, due, LOAD_MS);
        due.drop_where(|_| true);
        speech
    }

    const fn beat() -> Input {
        Input::Sidecar(SidecarId::Speech, SidecarEvent::Heartbeat)
    }

    #[test]
    fn the_speech_process_loads_beats_hangs_and_is_killed() {
        let mut speech = Speech::default();
        let mut due = Due::default();
        let say = SpeechCmd::Speak {
            speak: speak(),
            voice: String::new(),
            clauses: vec!["One.".to_owned()],
            whole_line: String::new(),
        };
        speech.command(0, &say, &mut due);
        assert_eq!(
            drain(&mut due, 0),
            [Input::Voice(VoiceEvent::SpeakDone { speak: speak() })],
            "before it is started, a `Speak` is done with no clause"
        );
        speech.sidecar(0, SidecarCmd::Start, &mut due);
        assert!(deliver(&mut speech, &mut due, LOAD_MS - 1).is_empty(), "still loading");
        let ready = deliver(&mut speech, &mut due, LOAD_MS);
        assert!(
            matches!(
                ready.as_slice(),
                [Input::Sidecar(SidecarId::Speech, SidecarEvent::Ready { .. })]
            ),
            "{ready:?}"
        );
        assert_eq!(speech.process(), Process::Up);
        let first = LOAD_MS + HEARTBEAT_MS;
        assert_eq!(deliver(&mut speech, &mut due, first), [beat()], "a second later");
        assert_eq!(deliver(&mut speech, &mut due, first + HEARTBEAT_MS), [beat()], "and on");
        speech.hang(&mut due);
        assert!(due.is_empty(), "a hung process sends nothing");
        speech.command(5_000, &say, &mut due);
        assert!(due.is_empty(), "not even `SpeakDone`");
        speech.sidecar(9_000, SidecarCmd::HangDumpThenKill, &mut due);
        assert_eq!(
            drain(&mut due, 9_050),
            [Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { code: None })],
            "the supervisor reports the kill's exit"
        );
        speech.stick_loads(true);
        speech.sidecar(10_000, SidecarCmd::Restart, &mut due);
        assert!(due.is_empty(), "a stuck load never sends `Ready`");
        speech.crash(11_000, &mut due);
        assert!(
            matches!(
                drain(&mut due, 11_000).as_slice(),
                [Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { code: Some(_) })]
            ),
            "a crash exits at once"
        );
    }

    #[test]
    fn the_speech_process_reports_each_clause_then_done() {
        let mut due = Due::default();
        let mut speech = ready(&mut due);
        let cmd = SpeechCmd::Speak {
            speak: speak(),
            voice: String::new(),
            clauses: vec!["One.".to_owned(), "Two two.".to_owned()],
            whole_line: String::new(),
        };
        speech.command(0, &cmd, &mut due);
        let inputs = drain(&mut due, 1_000);
        assert!(
            matches!(
                inputs.as_slice(),
                [
                    Input::Voice(VoiceEvent::ClauseReady { clause: 0, chars: 4, .. }),
                    Input::Voice(VoiceEvent::ClauseReady { clause: 1, chars: 8, .. }),
                    Input::Voice(VoiceEvent::SpeakDone { .. }),
                ]
            ),
            "{inputs:?}"
        );
        speech.command(0, &cmd, &mut due);
        assert!(
            matches!(
                drain(&mut due, 1_000).first(),
                Some(Input::Voice(VoiceEvent::ClauseReady { clause: 2, .. }))
            ),
            "clauses of the same utterance are numbered on"
        );
        speech.command(2_000, &cmd, &mut due);
        speech.command(2_000, &SpeechCmd::Cancel { speak: speak() }, &mut due);
        assert!(due.is_empty(), "a cancel drops what was not delivered");
    }

    #[test]
    fn the_device_plays_in_callbacks_reports_and_drains() {
        let mut device = Device::new();
        let mut due = Due::default();
        device.clause_ready(speak(), 0, 1_000);
        let mut now = 0;
        let mut inputs = Vec::new();
        for _ in 0..10 {
            now += CALLBACK_MS;
            device.callback(now, &mut due);
            inputs.extend(drain(&mut due, now));
        }
        assert_eq!(
            inputs,
            [
                Input::Playback(PlaybackEvent::Played {
                    speak: speak(),
                    clause: 0,
                    samples_played: 1_000
                }),
                Input::Playback(PlaybackEvent::Drained { speak: speak() }),
            ],
            "five callbacks play 1,000 samples and report at 50 ms; the sixth drains"
        );
        device.clause_ready(speak(), 1, 24_000);
        device.command(now, &AudioCmd::Pause { speak: speak() }, &mut due);
        assert!(
            matches!(
                drain(&mut due, now).as_slice(),
                [Input::Playback(PlaybackEvent::Paused { .. })]
            ),
            "a pause is reported"
        );
        for _ in 0..10 {
            now += CALLBACK_MS;
            device.callback(now, &mut due);
        }
        assert!(due.is_empty(), "nothing plays while paused");
        device.command(now, &AudioCmd::Resume { speak: speak() }, &mut due);
        for _ in 0..5 {
            now += CALLBACK_MS;
            device.callback(now, &mut due);
        }
        assert_eq!(
            drain(&mut due, now),
            [Input::Playback(PlaybackEvent::Played {
                speak: speak(),
                clause: 1,
                samples_played: 1_200
            })],
            "resumed from the paused sample, reported after 50 ms"
        );
        device.command(now, &AudioCmd::Stop { speak: speak() }, &mut due);
        assert!(
            matches!(
                drain(&mut due, now).as_slice(),
                [Input::Playback(PlaybackEvent::Stopped { samples_played: 1_200, .. })]
            ),
            "a stop is reported where it stopped"
        );
        assert!(device.speaking().is_empty(), "and forgotten");
    }

    fn turn(n: u128) -> (TurnId, BrainCmd) {
        turn_to(BrainSlot::Primary, n)
    }

    fn turn_to(slot: BrainSlot, n: u128) -> (TurnId, BrainCmd) {
        let turn = TurnId::from_parts(0, n);
        let cmd = BrainCmd::Turn {
            slot,
            turn,
            message: "[heard] tell me about rivers".to_owned(),
            authority: TurnAuthority::Addressed,
        };
        (turn, cmd)
    }

    /// Everything scheduled, in time order, with when it comes.
    fn all(due: &mut Due) -> Vec<(u64, Input)> {
        std::iter::from_fn(|| due.next_at().and_then(|at| due.pop(at).map(|input| (at, input))))
            .collect()
    }

    /// When each of `turn`'s text deltas and its end come, and whether it ended in an error.
    fn timeline(items: &[(u64, Input)], turn: TurnId) -> (Vec<u64>, Option<(u64, bool)>) {
        let mut deltas = Vec::new();
        let mut done = None;
        for (at, input) in items {
            if let Input::Brain(BrainEvent::TextDelta { turn: t, .. }) = input
                && *t == turn
            {
                deltas.push(*at);
            }
            if let Input::Brain(BrainEvent::TurnDone { turn: t, is_error, .. }) = input
                && *t == turn
            {
                done = Some((*at, *is_error));
            }
        }
        (deltas, done)
    }

    #[test]
    fn the_brain_answers_at_its_pace_and_an_interrupt_ends_the_turn() {
        let mut due = Due::default();
        let mut brain = Brain::default();
        let pace = BrainTurn { first_ms: 300, delta_ms: 100, ..BrainTurn::default() };
        let (one, cmd) = turn(1);
        brain.script(BrainReply::text(&["One. ", "Two. ", "Three."]));
        brain.command(1_000, &cmd, pace, &mut due);
        let (deltas, done) = timeline(&all(&mut due), one);
        assert_eq!(deltas, [1_300, 1_400, 1_500], "first text after 300 ms, then every 100");
        assert_eq!(done, Some((1_600, false)), "done after the last delta");
        let (two, cmd) = turn(2);
        brain.command(2_000, &cmd, pace, &mut due);
        brain.command(2_100, &BrainCmd::Interrupt { slot: BrainSlot::Primary }, pace, &mut due);
        let (deltas, done) = timeline(&all(&mut due), two);
        assert!(deltas.is_empty(), "an interrupt drops what was not sent: {deltas:?}");
        assert_eq!(done, Some((2_150, false)), "and ends the turn 50 ms later");
    }

    #[test]
    fn a_stalled_brain_sends_its_whole_answer_late_and_hears_no_interrupt() {
        let mut due = Due::default();
        let mut brain = Brain::default();
        let pace = BrainTurn { first_ms: 300, delta_ms: 100, ..BrainTurn::default() };
        let (one, cmd) = turn(1);
        brain.script(BrainReply::text(&["One. ", "Two."]).stalling(20_000));
        brain.command(0, &cmd, pace, &mut due);
        brain.command(5_000, &BrainCmd::Interrupt { slot: BrainSlot::Primary }, pace, &mut due);
        let (two, cmd) = turn(2);
        brain.command(5_000, &cmd, pace, &mut due);
        // Turn 2 is the live one now, but the process is still stuck in turn 1's stall.
        brain.command(6_000, &BrainCmd::Interrupt { slot: BrainSlot::Primary }, pace, &mut due);
        let items = all(&mut due);
        let (deltas, done) = timeline(&items, one);
        assert_eq!(deltas, [20_300, 20_400], "the whole answer, after the stall");
        assert_eq!(done, Some((20_500, false)), "and its end, though it was interrupted");
        let (deltas, done) = timeline(&items, two);
        assert_eq!(deltas, [20_800, 20_900, 21_000], "the next turn waits for the stuck process");
        assert_eq!(done, Some((21_100, false)), "and no interrupt reached it while stuck");
        let (four, cmd) = turn(4);
        brain.command(30_000, &cmd, pace, &mut due);
        brain.command(30_100, &BrainCmd::Interrupt { slot: BrainSlot::Primary }, pace, &mut due);
        let (deltas, done) = timeline(&all(&mut due), four);
        assert!(deltas.is_empty(), "once the stall is over it hears again: {deltas:?}");
        assert_eq!(done, Some((30_150, false)), "and ends the turn");
        let failed = BrainTurn { fail: true, ..pace };
        let (three, cmd) = turn(3);
        Brain::default().command(0, &cmd, failed, &mut due);
        assert_eq!(timeline(&all(&mut due), three), (vec![], Some((300, true))), "a failed turn");
    }

    /// Which instance sent each of `turn`'s text deltas, in time order.
    fn senders(items: &[(u64, Input)], turn: TurnId) -> Vec<(u64, u32)> {
        items
            .iter()
            .filter_map(|(at, input)| match input {
                Input::Brain(BrainEvent::TextDelta { instance, turn: t, .. }) if *t == turn => {
                    Some((*at, *instance))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_stall_in_one_slot_does_not_hold_up_another() {
        let mut due = Due::default();
        let mut brain = Brain::default();
        let pace = BrainTurn { first_ms: 300, delta_ms: 100, ..BrainTurn::default() };
        assert_eq!((brain.instance(BrainSlot::Primary), brain.running()), (Some(1), 1), "primary");
        let (one, cmd) = turn(1);
        brain.script(BrainReply::text(&["Stuck. "]).stalling(20_000));
        brain.command(0, &cmd, pace, &mut due);
        brain.command(5_000, &BrainCmd::Start { slot: BrainSlot::Fallback }, pace, &mut due);
        assert_eq!(brain.instance(BrainSlot::Fallback), Some(2), "the next instance number");
        assert_eq!(brain.running(), 2, "two processes hold memory");
        brain.script(BrainReply::text(&["Fresh. "]));
        let (_, cmd) = turn_to(BrainSlot::Fallback, 1);
        brain.command(5_000, &cmd, pace, &mut due);
        let items = all(&mut due);
        let ready = BrainEvent::Ready {
            which: BrainSlot::Fallback,
            instance: 2,
            route: Route::Claude,
            model: BRAIN_MODEL.to_owned(),
        };
        assert!(items.contains(&(5_000 + BRAIN_START_MS, Input::Brain(ready))), "{items:?}");
        assert_eq!(
            senders(&items, one),
            [(5_000 + BRAIN_START_MS + 300, 2), (20_300, 1)],
            "the fallback answers the same turn first, as another instance"
        );
        brain.command(30_000, &BrainCmd::Stop { slot: BrainSlot::Fallback }, pace, &mut due);
        assert_eq!((brain.instance(BrainSlot::Fallback), brain.running()), (None, 1), "stopped");
        let exited = BrainEvent::Exited { which: BrainSlot::Fallback, instance: 2, code: Some(0) };
        assert_eq!(all(&mut due), [(30_050, Input::Brain(exited))], "and its exit reported");
    }

    #[test]
    fn a_promoted_spare_answers_as_the_primary_and_a_new_spare_starts() {
        let mut due = Due::default();
        let mut brain = Brain::default();
        let pace = BrainTurn::default();
        brain.command(0, &BrainCmd::Start { slot: BrainSlot::Spare }, pace, &mut due);
        brain.command(
            0,
            &BrainCmd::Recycle { slot: BrainSlot::Primary, summary: String::new() },
            pace,
            &mut due,
        );
        assert_eq!(brain.instance(BrainSlot::Primary), Some(3), "a recycled primary is new");
        let (one, cmd) = turn(1);
        brain.command(10_000, &cmd, pace, &mut due);
        brain.command(10_100, &BrainCmd::Promote { slot: BrainSlot::Spare }, pace, &mut due);
        assert_eq!(brain.instance(BrainSlot::Primary), Some(2), "the spare is the primary");
        assert_eq!(brain.instance(BrainSlot::Spare), Some(4), "and a new spare started");
        assert_eq!(brain.running(), 2, "the killed primary holds nothing");
        let items = all(&mut due);
        assert!(senders(&items, one).is_empty(), "the killed primary sends nothing more");
        let killed = BrainEvent::Exited { which: BrainSlot::Primary, instance: 3, code: None };
        assert!(items.contains(&(10_150, Input::Brain(killed))), "{items:?}");
        assert!(
            items.iter().any(|(_, input)| matches!(
                input,
                Input::Brain(BrainEvent::Ready { which: BrainSlot::Primary, instance: 2, .. })
            )),
            "the core hears which instance is the primary now: {items:?}"
        );
    }

    #[test]
    fn a_slow_load_takes_longer_once() {
        let mut due = Due::default();
        let mut speech = Speech::default();
        speech.slow_next_load();
        speech.sidecar(0, SidecarCmd::Start, &mut due);
        assert_eq!(due.next_at(), Some(LOAD_MS + SLOW_LOAD_MS), "the slow load");
        speech.sidecar(0, SidecarCmd::Restart, &mut due);
        assert_eq!(due.next_at(), Some(LOAD_MS), "the next load is not slow");
    }
}

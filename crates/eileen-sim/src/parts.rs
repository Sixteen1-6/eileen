//! The simulated parts that answer the core's effects (BUILD.md section 4.7): the speech
//! process, the playback device, a scripted brain and scripted workers. Each part turns an
//! effect into inputs scheduled on [`Due`] at virtual times; the world delivers them in time
//! order.
//!
//! - The speech process answers `Speak` with one `ClauseReady` per clause, a synthesis time
//!   apart, then `SpeakDone`; `Cancel` drops what it has not delivered. It answers the core's
//!   `Start` and `Restart` with `Ready` after a load, and the audio component's pings with a
//!   `Heartbeat` every second while it is ready; `HangDumpThenKill` ends it with `Exited`. A
//!   test can hang it (no more heartbeats or clauses), crash it (`Exited` at once) or make its
//!   loads never finish. While it is not ready the audio component answers `Speak` with
//!   `SpeakDone` and no clause.
//! - The device plays the clauses whose `ClauseReady` was delivered, in 10 ms callbacks of
//!   240 samples (24 kHz), reports `Played` at most every 50 ms, and honours `Duck`, `Pause`
//!   (with a `Paused` report), `Resume` and `Stop` (with a `Stopped` report). It reports
//!   `Drained` when every sample it was given has played.
//! - The brain answers each `Turn` with the next scripted reply (text in deltas, an optional
//!   `jobs_dispatch` call, `TurnDone`); `Interrupt` drops the rest of the turn.
//! - A worker session answers `Start` with `Init`, `Bind` with one `PreToolUse` for a write in
//!   its job folder and then `TurnDone`, and `Cancel` with `Exited`.

use std::collections::{BTreeMap, VecDeque};

use eileen_contracts::connectors::{
    ConnectorManifest, Platform, RunsIn, SidecarCmd, SidecarEvent, SidecarId,
};
use eileen_contracts::core_io::{
    AudioCmd, Caller, Input, PathAccess, PathClass, PathFact, PlaybackEvent, ShimRole,
    ToolCallRequest,
};
use eileen_contracts::harness::{BrainCmd, BrainEvent, HookRequest, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::{
    ConnectorId, Personal, SpeakId, ToolName, TurnId, Untrusted, WorkerSessionId,
};
use eileen_contracts::jobs::{CallOrigin, Cost, TurnAuthority};
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};
use serde_json::{Value, json};

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
/// How long the supervisor takes to report the exit of a process the guard killed, ms.
const EXIT_MS: u64 = 50;
/// The exit code of a crashed speech process.
const CRASH_CODE: i32 = 1;
/// How long the brain takes to its first text, and between deltas, ms.
const BRAIN_FIRST_MS: u64 = 800;
const BRAIN_DELTA_MS: u64 = 150;
/// How long a worker session takes to start, to ask its first hook and to finish, ms.
const WORKER_START_MS: u64 = 500;
const WORKER_HOOK_MS: u64 = 300;
const WORKER_DONE_MS: u64 = 1_200;

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

    /// Answers the core's command for the process: `Start` and `Restart` load it, and
    /// `HangDumpThenKill` ends it.
    pub fn sidecar(&mut self, now_ms: u64, cmd: SidecarCmd, due: &mut Due) {
        match cmd {
            SidecarCmd::Start | SidecarCmd::Restart => {
                due.drop_where(from_the_process);
                self.utterances.clear();
                self.process = Process::Loading;
                if !self.stuck_loads
                    && let Some(ready) = ready()
                {
                    due.schedule(now_ms.saturating_add(LOAD_MS), ready);
                }
            }
            SidecarCmd::HangDumpThenKill => {
                if matches!(self.process, Process::Off | Process::Dead) {
                    return;
                }
                self.end(due);
                let exited = Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { code: None });
                due.schedule(now_ms.saturating_add(EXIT_MS), exited);
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

    /// One 10 ms callback at `now_ms`: the first utterance takes its samples.
    pub fn callback(&mut self, now_ms: u64, due: &mut Due) {
        let Some(playing) = self.playing.front_mut() else { return };
        if playing.paused {
            return;
        }
        if !playing.take(SAMPLES_PER_CALLBACK) {
            let speak = playing.speak;
            self.playing.pop_front();
            due.schedule(now_ms, Input::Playback(PlaybackEvent::Drained { speak }));
            return;
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
    }
}

impl Playing {
    /// The clause playing and its samples played, or the last position reached.
    fn position(&self) -> (u16, u32) {
        self.current.map_or(self.last, |(clause, _, played)| (clause, played))
    }

    /// Takes up to `budget` samples; false when nothing was left to play.
    fn take(&mut self, mut budget: u32) -> bool {
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
}

impl BrainReply {
    /// A reply of these deltas.
    #[must_use]
    pub fn text(deltas: &[&str]) -> Self {
        Self {
            deltas: deltas.iter().map(|d| (*d).to_owned()).collect(),
            dispatch: None,
            fail: false,
        }
    }

    /// The same reply, dispatching a general job named `title` that does `task`.
    #[must_use]
    pub fn dispatching(mut self, title: &str, task: &str) -> Self {
        self.dispatch = Some(json!({ "kind": "general", "task": task, "title_hint": title }));
        self
    }
}

/// The scripted brain.
#[derive(Debug, Default)]
pub struct Brain {
    script: VecDeque<BrainReply>,
    live: Option<TurnId>,
}

impl Brain {
    /// Queues the reply to the next turn.
    pub fn script(&mut self, reply: BrainReply) {
        self.script.push_back(reply);
    }

    /// The reply to a turn nobody scripted: a short answer, and a job when the user asked
    /// for a file.
    fn default_reply(message: &str) -> BrainReply {
        if message.contains("file") {
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

    /// Answers a brain command.
    pub fn command(&mut self, now_ms: u64, cmd: &BrainCmd, due: &mut Due) {
        match cmd {
            BrainCmd::Turn { turn, message, authority, .. } => {
                let reply = self.script.pop_front().unwrap_or_else(|| Self::default_reply(message));
                self.live = Some(*turn);
                answer(now_ms, *turn, *authority, &reply, due);
            }
            BrainCmd::Interrupt { .. } => {
                if let Some(turn) = self.live.take() {
                    due.drop_where(|input| of_brain_turn(input, turn));
                    due.schedule(now_ms.saturating_add(50), Input::Brain(turn_done(turn, false)));
                }
            }
            BrainCmd::Recycle { .. }
            | BrainCmd::Promote { .. }
            | BrainCmd::Start { .. }
            | BrainCmd::Stop { .. } => {}
        }
    }
}

/// Schedules a reply's deltas, its dispatch and its end.
fn answer(now_ms: u64, turn: TurnId, authority: TurnAuthority, reply: &BrainReply, due: &mut Due) {
    let mut at = now_ms.saturating_add(BRAIN_FIRST_MS);
    for (index, text) in reply.deltas.iter().enumerate() {
        let delta = BrainEvent::TextDelta { instance: 1, turn, text: Untrusted::new(text.clone()) };
        due.schedule(at, Input::Brain(delta));
        if index == 0
            && let Some(call) =
                reply.dispatch.as_ref().and_then(|args| dispatch(turn, authority, args))
        {
            due.schedule(at.saturating_add(20), Input::ToolCall(call));
        }
        at = at.saturating_add(BRAIN_DELTA_MS);
    }
    due.schedule(at, Input::Brain(turn_done(turn, reply.fail)));
}

fn turn_done(turn: TurnId, is_error: bool) -> BrainEvent {
    BrainEvent::TurnDone {
        instance: 1,
        turn,
        cost: Cost::default(),
        context_tokens: 1_000,
        is_error,
    }
}

/// The brain's `jobs_dispatch` call (the tool name always parses).
fn dispatch(turn: TurnId, authority: TurnAuthority, args: &Value) -> Option<ToolCallRequest> {
    Some(ToolCallRequest {
        caller: Caller::Shim { connection: 1, role: ShimRole::Brain },
        origin: CallOrigin::Brain { turn, authority },
        tool: ToolName::new("jobs_dispatch").ok()?,
        args: Untrusted::new(args.clone()),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    })
}

/// Whether `input` belongs to the brain turn `turn`.
fn of_brain_turn(input: &Input, turn: TurnId) -> bool {
    let text = matches!(
        input,
        Input::Brain(BrainEvent::TextDelta { turn: t, .. } | BrainEvent::TurnDone { turn: t, .. })
            if *t == turn
    );
    let call = matches!(
        input,
        Input::ToolCall(request)
            if matches!(request.origin, CallOrigin::Brain { turn: t, .. } if t == turn)
    );
    text || call
}

/// The scripted worker sessions.
#[derive(Debug, Default)]
pub struct Workers {
    hooks: u64,
}

impl Workers {
    /// Answers a worker command.
    pub fn command(
        &mut self,
        now_ms: u64,
        session: WorkerSessionId,
        cmd: &WorkerCmd,
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
                due.schedule(now_ms.saturating_add(WORKER_HOOK_MS), Input::Hook(session, hook));
                let done = WorkerEvent::TurnDone {
                    cost: Cost::default(),
                    is_error: false,
                    summary: Untrusted::new("Created hello.txt with hi.".to_owned()),
                };
                due.schedule(now_ms.saturating_add(WORKER_DONE_MS), Input::Worker(session, done));
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
    use eileen_contracts::ids::SpeakId;
    use eileen_contracts::speech::{SpeechCmd, VoiceEvent};

    use super::{CALLBACK_MS, Device, Due, HEARTBEAT_MS, LOAD_MS, Process, Speech};

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
}

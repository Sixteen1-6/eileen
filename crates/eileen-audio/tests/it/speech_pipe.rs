//! WP4 acceptance 15, the core end of the speech pipe: the audio component, built through
//! `eileen_audio::component`, captures a 16 kHz test clip from a `WavSource` and serves each
//! connection of the in-memory transport, where a scripted stand-in plays the speech process. All
//! of it runs on paused tokio time, with no models and no audio device.
//!
//! The stand-in recognises speech the simplest way that still exercises the ring: a frame with
//! any sample above zero is speech, the first such frame opens a turn (a `SpeechStarted`, marked
//! replayed when the frame carried the replayed flag), and the next silent frame ends it (an
//! `EndOfTurn` whose text names the level it heard, so each spoken command is told apart by its
//! level). A frame with the break flag drops an open turn, as voice activity starts fresh. It
//! answers each `Ping` with a `Pong` that carries the newest frame it read and the open turn, as
//! decision D81 asks of the speech process.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eileen_audio::fake::{MemoryLimits, StandIns, memory_transport};
use eileen_audio::{
    AudioConfig, ConnectionError, ConnectionId, Pace, SharedSource, SpeechConnection, StreamFormat,
    WavClip, WavSource, component,
};
use eileen_contracts::connectors::{SidecarEvent, SidecarId};
use eileen_contracts::core_io::{AudioCmd, Effect, Input};
use eileen_contracts::ids::{
    Envelope, MonoTime, Personal, SessionId, SpeakId, TraceCtx, TurnId, WallTime,
};
use eileen_contracts::ports::{
    Backpressure, ComponentCtx, ComponentError, EffectSource, InputSink, ShutdownSignal,
};
use eileen_contracts::speech::{
    AudioFlags, Frame, FrameBody, FrameReader, ListenMode, SpeechCheck, SpeechCheckRequest,
    SpeechCmd, SpeechEventFrame, TurnEndReason, VoiceEvent,
};
use eileen_rt::channel::{self, Receiver, Sender};
use eileen_trace::names;
use eileen_trace::test_support::{Captured, capture};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep};

/// Every wait in these tests that is not the subject of the test.
const WAIT: Duration = Duration::from_secs(5);

/// The first command's level, as an `i16` sample.
const FIRST: i16 = 8_192;
/// The second command's level.
const SECOND: i16 = 16_384;

fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

const fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A command spoken into the test clip: from `at`, for `len`, every sample at `level`.
struct Say {
    at: Duration,
    len: Duration,
    level: i16,
}

/// A 16 kHz mono clip of silence `total` long with these commands in it.
fn clip(total: Duration, says: &[Say]) -> WavClip {
    let samples_in = |d: Duration| usize::try_from(d.as_millis()).unwrap() * 16;
    let mut samples = vec![0.0_f32; samples_in(total)];
    for say in says {
        let start = samples_in(say.at);
        samples[start..start + samples_in(say.len)].fill(f32::from(say.level) / 32_767.0);
    }
    WavClip::new(StreamFormat { sample_rate_hz: 16_000, channels: 1 }, samples).unwrap()
}

/// Runs `body` on paused tokio time, as `#[tokio::test(start_paused = true)]` does, under a
/// subscriber that captures every event, so the test can read what the component logged.
fn logged<T>(body: impl Future<Output = T>) -> (T, Captured) {
    capture(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(body)
    })
}

fn envelope<T>(body: T) -> Envelope<T> {
    Envelope { seq: 0, at: WallTime(0), mono: MonoTime(0), ctx: TraceCtx::default(), body }
}

struct Inputs(Sender<Envelope<Input>>);

impl InputSink for Inputs {
    fn send(&self, input: Envelope<Input>) -> Result<(), Backpressure> {
        self.0.try_send(input).map_err(|_| Backpressure { capacity: self.0.capacity() })
    }
}

struct Effects(Receiver<Envelope<Effect>>);

impl EffectSource for Effects {
    fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
        Box::pin(self.0.recv())
    }
}

struct Stop(watch::Receiver<bool>);

impl ShutdownSignal for Stop {
    fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let mut stop = self.0.clone();
        Box::pin(async move {
            let _stopped = stop.wait_for(|stop| *stop).await;
        })
    }
}

/// The audio component running on a test clip, with the test's side of its transport.
struct Rig {
    stand_ins: StandIns,
    effects: Sender<Envelope<Effect>>,
    inputs: Receiver<Envelope<Input>>,
    stop: watch::Sender<bool>,
    task: JoinHandle<Result<(), ComponentError>>,
    /// Turn numbers already minted, so each stand-in mints new ones as a new process would.
    turns: u128,
}

impl Rig {
    fn start(clip: WavClip) -> Self {
        let (transport, stand_ins) = memory_transport(MemoryLimits::default());
        let source: SharedSource =
            Arc::new(Mutex::new(WavSource::new(clip, Pace::RealTime).unwrap()));
        let config = AudioConfig { speech: Some(Arc::new(transport)), capture: Some(source) };
        let audio = component(&config);
        let (effects, effects_rx) = channel::bounded("test.effects", nz(64));
        let (inputs_tx, inputs) = channel::bounded("test.inputs", nz(4_096));
        let (stop, stop_rx) = watch::channel(false);
        let ctx = ComponentCtx {
            inputs: Box::new(Inputs(inputs_tx)),
            effects: Box::new(Effects(effects_rx)),
            data_dir: PathBuf::from("data"),
            session: SessionId::from_parts(1, 1),
            shutdown: Box::new(Stop(stop_rx)),
        };
        let task = eileen_rt::task::spawn("test.audio", audio.start(ctx));
        Self { stand_ins, effects, inputs, stop, task, turns: 0 }
    }

    /// Routes an effect to the component and gives it a millisecond to take it.
    async fn effect(&self, body: Effect) {
        self.effects.send(envelope(body)).await.unwrap();
        sleep(ms(1)).await;
    }

    async fn speech(&self, cmd: SpeechCmd) {
        self.effect(Effect::Speech(cmd)).await;
    }

    /// Connects a new stand-in, as a speech process that has loaded its models.
    fn connect(&mut self) -> StandIn {
        let connection = self.stand_ins.connect().unwrap();
        self.turns += 1_000;
        StandIn::new(connection, self.turns)
    }

    /// Waits for the component to hand a connection back.
    async fn handed_back(&mut self) -> ConnectionId {
        self.stand_ins.handed_back(WAIT).await.unwrap()
    }

    /// Every input the component has sent so far.
    fn inputs(&mut self) -> Vec<Input> {
        std::iter::from_fn(|| self.inputs.try_recv().ok()).map(|input| input.body).collect()
    }

    /// Stops the component and checks that it ended cleanly.
    async fn stop(self) {
        self.stop.send(true).unwrap();
        // Within a write's deadline and a read's, which a component stopping mid-write waits out,
        // with room to spare.
        let within = Duration::from_secs(10);
        let ended = tokio::time::timeout(within, self.task).await.unwrap().unwrap();
        assert_eq!(ended, Ok(()), "the component stops cleanly");
    }
}

/// The voice events among `inputs`.
fn voice(inputs: &[Input]) -> Vec<VoiceEvent> {
    inputs
        .iter()
        .filter_map(
            |input| if let Input::Voice(event) = input { Some(event.clone()) } else { None },
        )
        .collect()
}

/// The final transcripts among `inputs`, in order.
fn ends(inputs: &[Input]) -> Vec<String> {
    voice(inputs)
        .into_iter()
        .filter_map(|event| {
            if let VoiceEvent::EndOfTurn { text, .. } = event {
                Some(text.into_inner())
            } else {
                None
            }
        })
        .collect()
}

fn text_of(level: i16) -> String {
    format!("level {level}")
}

/// What a stand-in received, in order.
#[derive(Debug, Clone, PartialEq)]
enum Got {
    Command(SpeechCmd),
    Audio { seq: u64, flags: AudioFlags, level: i16 },
    Ping,
    Pong,
}

/// A frame of audio a stand-in received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Heard {
    seq: u64,
    flags: AudioFlags,
    level: i16,
}

/// A turn a stand-in recognised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Turn {
    id: TurnId,
    level: i16,
    replayed: bool,
    /// The frame whose silence ended it.
    ended_at: u64,
}

/// A turn still open.
#[derive(Debug, Clone, Copy)]
struct Open {
    turn: TurnId,
    level: i16,
    replayed: bool,
}

/// The scripted speech process.
struct StandIn {
    connection: SpeechConnection,
    frames: FrameReader,
    got: Vec<Got>,
    newest: u64,
    open: Option<Open>,
    turns: Vec<Turn>,
    next_turn: u128,
    pongs_sent: usize,
    last_pong_seq: u64,
    /// Paced time it waits after each read, as a speech process slower than the pipe.
    read_pause: Duration,
}

impl StandIn {
    fn new(connection: SpeechConnection, turns_from: u128) -> Self {
        Self {
            connection,
            frames: FrameReader::for_speech(),
            got: Vec::new(),
            newest: 0,
            open: None,
            turns: Vec::new(),
            next_turn: turns_from,
            pongs_sent: 0,
            last_pong_seq: 0,
            read_pause: Duration::ZERO,
        }
    }

    /// Reads and answers frames for `span` of paced time, or until the component hands the
    /// connection back.
    async fn run_for(&mut self, span: Duration) -> Result<(), ConnectionError> {
        let until = Instant::now() + span;
        let mut buf = vec![0_u8; 8_192];
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(());
            }
            let n = match self.connection.reader().read(&mut buf, left).await {
                Ok(n) => n,
                Err(ConnectionError::TimedOut { .. }) => return Ok(()),
                Err(error) => return Err(error),
            };
            let mut rest = &buf[..n];
            while !rest.is_empty() {
                let (taken, frame) = self.frames.feed(rest).unwrap();
                rest = &rest[taken..];
                if let Some(frame) = frame {
                    self.on_frame(frame).await;
                }
            }
            if !self.read_pause.is_zero() {
                sleep(self.read_pause).await;
            }
        }
    }

    async fn on_frame(&mut self, frame: Frame) {
        match frame.body {
            FrameBody::Command(command) => self.got.push(Got::Command(command.cmd)),
            FrameBody::Audio(samples) => {
                let level = samples.iter().map(|s| s.saturating_abs()).max().unwrap();
                self.on_audio(Heard { seq: frame.seq, flags: frame.flags, level }).await;
            }
            FrameBody::Ping => {
                self.got.push(Got::Ping);
                let turn = self.open.map_or([0; 16], |open| open.turn.ulid().to_bytes());
                self.send(Frame { seq: self.newest, turn, ..Frame::new(FrameBody::Pong) }).await;
                self.pongs_sent += 1;
                self.last_pong_seq = self.newest;
            }
            FrameBody::Pong => self.got.push(Got::Pong),
            other @ (FrameBody::Event(_) | FrameBody::Pcm { .. } | FrameBody::Hello(_)) => {
                panic!("the core may not send {other:?}")
            }
        }
    }

    async fn on_audio(&mut self, heard: Heard) {
        self.newest = heard.seq;
        self.got.push(Got::Audio { seq: heard.seq, flags: heard.flags, level: heard.level });
        if heard.flags.contains(AudioFlags::BREAK) {
            self.open = None;
        }
        match self.open {
            None if heard.level > 0 => {
                self.next_turn += 1;
                let open = Open {
                    turn: TurnId::from_parts(1, self.next_turn),
                    level: heard.level,
                    replayed: heard.flags.contains(AudioFlags::REPLAYED),
                };
                self.open = Some(open);
                let started = VoiceEvent::SpeechStarted {
                    turn: open.turn,
                    source: 0,
                    replayed: open.replayed,
                };
                self.event(started).await;
            }
            Some(open) if heard.level == 0 => {
                self.open = None;
                self.turns.push(Turn {
                    id: open.turn,
                    level: open.level,
                    replayed: open.replayed,
                    ended_at: heard.seq,
                });
                self.event(VoiceEvent::EndOfTurn {
                    turn: open.turn,
                    text: Personal::new(text_of(open.level)),
                    smart_turn_pct: 90,
                    silence_ms: 32,
                    asr_confidence_pct: 90,
                    reason: TurnEndReason::SmartTurn,
                    source: 0,
                    level_db: -20,
                })
                .await;
            }
            _ => {}
        }
    }

    async fn event(&mut self, event: VoiceEvent) {
        let body = SpeechEventFrame { ctx: TraceCtx::default(), span: 0, event };
        self.send(Frame { seq: self.newest, ..Frame::new(FrameBody::Event(body)) }).await;
    }

    async fn send(&mut self, frame: Frame) {
        let bytes = frame.encode().unwrap();
        self.connection.writer().write_all(&bytes, WAIT).await.unwrap();
    }

    /// The audio frames it received, in order.
    fn audio(&self) -> Vec<Heard> {
        self.got
            .iter()
            .filter_map(|got| {
                if let Got::Audio { seq, flags, level } = got {
                    Some(Heard { seq: *seq, flags: *flags, level: *level })
                } else {
                    None
                }
            })
            .collect()
    }

    fn count(&self, wanted: &Got) -> usize {
        self.got.iter().filter(|got| *got == wanted).count()
    }
}

#[tokio::test(start_paused = true)]
async fn a_stand_in_that_connects_late_recognises_an_earlier_command_once_as_replayed() {
    let mut rig = Rig::start(clip(
        Duration::from_secs(12),
        &[Say { at: ms(1_000), len: ms(500), level: FIRST }],
    ));
    sleep(Duration::from_secs(6)).await;

    let mut stand_in = rig.connect();
    stand_in.run_for(Duration::from_secs(2)).await.unwrap();

    let audio = stand_in.audio();
    assert_eq!(audio[0].seq, 1, "the replay starts at the first frame captured");
    assert!(audio.windows(2).all(|pair| pair[1].seq == pair[0].seq + 1), "no frame missing");
    assert!(audio.iter().all(|heard| !heard.flags.contains(AudioFlags::BREAK)), "no break");
    let spoken: Vec<&Heard> = audio.iter().filter(|heard| heard.level > 0).collect();
    assert!(!spoken.is_empty(), "the command's frames were written");
    assert!(
        spoken.iter().all(|heard| heard.flags.contains(AudioFlags::REPLAYED)),
        "every frame of the command carries the replayed flag"
    );
    let last = audio.last().unwrap();
    assert!(!last.flags.contains(AudioFlags::REPLAYED), "live frames follow the replay");

    assert_eq!(stand_in.turns.len(), 1, "recognised exactly once: {:?}", stand_in.turns);
    assert_eq!((stand_in.turns[0].level, stand_in.turns[0].replayed), (FIRST, true));
    let inputs = rig.inputs();
    let started: Vec<bool> = voice(&inputs)
        .into_iter()
        .filter_map(|event| {
            if let VoiceEvent::SpeechStarted { replayed, .. } = event {
                Some(replayed)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(started, [true], "one SpeechStarted reached the core, marked replayed");
    assert_eq!(ends(&inputs), [text_of(FIRST)], "one EndOfTurn reached the core");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn the_latest_set_mode_and_set_hotwords_reach_each_new_stand_in_before_any_audio() {
    let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
    let words = |list: &[&str]| SpeechCmd::SetHotwords {
        words: list.iter().map(|word| (*word).to_owned()).collect(),
    };
    rig.speech(SpeechCmd::SetMode(ListenMode::Dictation)).await;
    rig.speech(words(&["Resolve"])).await;
    rig.speech(SpeechCmd::SetMode(ListenMode::Conversation)).await;
    rig.speech(words(&["Eileen", "Resolve"])).await;
    sleep(Duration::from_secs(1)).await;

    let latest = [
        Got::Command(SpeechCmd::SetMode(ListenMode::Conversation)),
        Got::Command(words(&["Eileen", "Resolve"])),
    ];
    for connection in 1..=2 {
        let mut stand_in = rig.connect();
        stand_in.run_for(Duration::from_secs(1)).await.unwrap();
        assert_eq!(stand_in.got[..2], latest, "connection {connection}: the latest first");
        assert!(
            matches!(stand_in.got[2], Got::Audio { .. }),
            "connection {connection}: then audio: {:?}",
            stand_in.got[2]
        );
        let commands = stand_in.got.iter().filter(|got| matches!(got, Got::Command(_))).count();
        assert_eq!(commands, 2, "connection {connection}: no older command is sent");
        drop(stand_in);
        assert_eq!(rig.handed_back().await, ConnectionId::new(connection), "handed back");
    }
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn after_a_kill_no_finished_turn_is_recognised_again_and_an_open_one_once_more() {
    let mut rig = Rig::start(clip(
        Duration::from_secs(14),
        &[
            Say { at: ms(1_000), len: ms(500), level: FIRST },
            Say { at: ms(3_000), len: ms(1_500), level: SECOND },
        ],
    ));
    sleep(ms(500)).await;
    let mut first = rig.connect();
    first.run_for(ms(3_200)).await.unwrap();
    assert_eq!(first.turns.len(), 1, "the first command was recognised live");
    assert_eq!(first.turns[0].level, FIRST);
    let open = first.open.expect("the second command is still being spoken");
    assert_eq!(open.level, SECOND);
    let finished_at = first.turns[0].ended_at;

    drop(first);
    assert_eq!(rig.handed_back().await, ConnectionId::new(1), "the dead connection goes back");
    sleep(Duration::from_secs(2)).await;
    let mut second = rig.connect();
    second.run_for(Duration::from_secs(3)).await.unwrap();

    let audio = second.audio();
    assert!(audio[0].seq > finished_at, "the replay starts after the finished turn");
    assert!(audio.iter().all(|heard| heard.level != FIRST), "the finished command is not resent");
    assert_eq!(second.turns.len(), 1, "the open command is recognised once: {:?}", second.turns);
    assert_eq!(second.turns[0].level, SECOND);
    assert_ne!(second.turns[0].id, open.turn, "under a new turn ID");
    assert!(second.turns[0].replayed, "from replayed frames");
    assert_eq!(ends(&rig.inputs()), [text_of(FIRST), text_of(SECOND)], "each command once");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_reload_closes_and_reconnects_on_the_same_path() {
    let mut rig = Rig::start(clip(
        Duration::from_secs(12),
        &[Say { at: ms(3_000), len: ms(500), level: FIRST }],
    ));
    rig.speech(SpeechCmd::SetMode(ListenMode::Conversation)).await;
    sleep(ms(500)).await;
    let mut listening = rig.connect();
    listening.run_for(ms(1_500)).await.unwrap();
    assert!(listening.turns.is_empty(), "nothing said yet");

    // Before a reload that stops recognition the speech process closes its connection, and
    // connects again once it listens; a command spoken meanwhile waits in the ring.
    drop(listening);
    assert_eq!(rig.handed_back().await, ConnectionId::new(1), "the closed connection goes back");
    sleep(Duration::from_secs(4)).await;
    let mut reloaded = rig.connect();
    reloaded.run_for(Duration::from_secs(2)).await.unwrap();

    assert_eq!(
        reloaded.got[0],
        Got::Command(SpeechCmd::SetMode(ListenMode::Conversation)),
        "the mode comes first again"
    );
    assert_eq!(reloaded.turns.len(), 1, "the command spoken during the reload, once");
    assert_eq!((reloaded.turns[0].level, reloaded.turns[0].replayed), (FIRST, true));
    assert_eq!(ends(&rig.inputs()), [text_of(FIRST)]);
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn an_overrun_sets_the_break_flag_on_the_first_frame_written_after_the_gap() {
    // The ring holds 180 s; a stand-in 185 s late finds the oldest frames dropped, the command
    // among them.
    let mut rig = Rig::start(clip(
        Duration::from_secs(190),
        &[Say { at: ms(1_000), len: ms(500), level: FIRST }],
    ));
    sleep(Duration::from_secs(185)).await;
    let mut late = rig.connect();
    late.run_for(Duration::from_secs(2)).await.unwrap();

    let audio = late.audio();
    assert!(audio[0].seq > 100, "the oldest frames were dropped: first {}", audio[0].seq);
    assert!(audio[0].flags.contains(AudioFlags::BREAK), "the first frame after the gap breaks");
    assert!(audio[1..].iter().all(|heard| !heard.flags.contains(AudioFlags::BREAK)), "only it");
    assert!(audio.windows(2).all(|pair| pair[1].seq == pair[0].seq + 1), "none missing after");
    assert!(late.turns.is_empty(), "the dropped command is never heard");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn frames_waiting_while_capture_was_off_are_never_written_to_a_later_connection() {
    let mut rig = Rig::start(clip(
        Duration::from_secs(10),
        &[
            Say { at: ms(1_000), len: ms(500), level: FIRST },
            Say { at: ms(5_000), len: ms(500), level: SECOND },
        ],
    ));
    sleep(Duration::from_secs(2)).await;
    rig.effect(Effect::Audio(AudioCmd::SetCapture(false))).await;
    sleep(Duration::from_secs(2)).await;
    rig.effect(Effect::Audio(AudioCmd::SetCapture(true))).await;
    sleep(Duration::from_secs(3)).await;
    let mut stand_in = rig.connect();
    stand_in.run_for(Duration::from_secs(2)).await.unwrap();

    let audio = stand_in.audio();
    assert!(audio.iter().all(|heard| heard.level != FIRST), "nothing from before the mute");
    assert!(audio[0].flags.contains(AudioFlags::BREAK), "the first frame after it breaks");
    assert_eq!(stand_in.turns.len(), 1, "{:?}", stand_in.turns);
    assert_eq!(stand_in.turns[0].level, SECOND, "the command after the mute is heard");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn frames_a_dead_connection_left_unconfirmed_are_dropped_by_a_mute_before_the_next() {
    let mut rig = Rig::start(clip(
        Duration::from_secs(10),
        &[Say { at: ms(1_000), len: ms(1_000), level: FIRST }],
    ));
    sleep(ms(500)).await;
    let mut first = rig.connect();
    first.run_for(ms(800)).await.unwrap();
    assert!(first.open.is_some(), "the command is being spoken when the stand-in dies");
    drop(first);
    assert_eq!(rig.handed_back().await, ConnectionId::new(1));

    rig.effect(Effect::Audio(AudioCmd::SetCapture(false))).await;
    sleep(Duration::from_secs(1)).await;
    rig.effect(Effect::Audio(AudioCmd::SetCapture(true))).await;
    sleep(Duration::from_secs(1)).await;
    let mut second = rig.connect();
    second.run_for(Duration::from_secs(1)).await.unwrap();

    let audio = second.audio();
    assert!(audio.iter().all(|heard| heard.level != FIRST), "none of the open command again");
    assert!(audio[0].flags.contains(AudioFlags::BREAK), "the first frame after the mute breaks");
    assert!(second.turns.is_empty(), "nothing recognised: {:?}", second.turns);
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_mute_while_connected_breaks_only_the_first_frame_after_it() {
    let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
    sleep(ms(500)).await;
    let mut stand_in = rig.connect();
    stand_in.run_for(Duration::from_secs(1)).await.unwrap();
    let before = stand_in.audio().len();
    rig.effect(Effect::Audio(AudioCmd::SetCapture(false))).await;
    sleep(Duration::from_secs(1)).await;
    rig.effect(Effect::Audio(AudioCmd::SetCapture(true))).await;
    stand_in.run_for(Duration::from_secs(1)).await.unwrap();

    let audio = stand_in.audio();
    let breaks: Vec<usize> =
        (0..audio.len()).filter(|at| audio[*at].flags.contains(AudioFlags::BREAK)).collect();
    assert_eq!(breaks.len(), 1, "one break: {breaks:?}");
    assert!(breaks[0] >= before, "after the mute");
    let resumed = &audio[breaks[0]..];
    assert!(resumed.windows(2).all(|pair| pair[1].seq == pair[0].seq + 1), "none missing after");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn pings_go_every_second_each_pong_is_a_heartbeat_and_confirms_the_frames_it_names() {
    let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
    sleep(ms(500)).await;
    let mut first = rig.connect();
    first.run_for(ms(3_500)).await.unwrap();
    assert_eq!(first.count(&Got::Ping), 3, "a Ping each second");

    first.send(Frame::new(FrameBody::Ping)).await;
    first.run_for(ms(100)).await.unwrap();
    assert_eq!(first.count(&Got::Pong), 1, "the stand-in's Ping is answered");

    let heartbeats = rig
        .inputs()
        .iter()
        .filter(|input| **input == Input::Sidecar(SidecarId::Speech, SidecarEvent::Heartbeat))
        .count();
    assert_eq!(heartbeats, first.pongs_sent, "each Pong is a heartbeat");

    let confirmed = first.last_pong_seq;
    drop(first);
    assert_eq!(rig.handed_back().await, ConnectionId::new(1));
    let mut second = rig.connect();
    second.run_for(ms(500)).await.unwrap();
    assert_eq!(second.audio()[0].seq, confirmed + 1, "the replay starts after the last Pong");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn pings_keep_coming_while_a_long_replay_is_read_slowly() {
    let mut rig = Rig::start(clip(Duration::from_secs(70), &[]));
    sleep(Duration::from_secs(60)).await;
    let mut slow = rig.connect();
    // 8 KiB each 20 ms, about twelve times real time: a minute of replay takes about 5 s.
    slow.read_pause = ms(20);
    slow.run_for(Duration::from_secs(4)).await.unwrap();

    let audio = slow.audio();
    assert!(audio.len() > 1_000, "the replay is under way: {} frames", audio.len());
    assert!(audio.last().unwrap().flags.contains(AudioFlags::REPLAYED), "and not over after 4 s");
    assert!(
        slow.count(&Got::Ping) >= 3,
        "a Ping each second meanwhile: {}",
        slow.count(&Got::Ping)
    );
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn a_frame_the_codec_refuses_closes_the_connection_and_hands_it_back() {
    let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
    sleep(ms(500)).await;
    let mut garbled = rig.connect();
    garbled.connection.writer().write_all(&[0; 32], WAIT).await.unwrap();
    let ended = garbled.run_for(Duration::from_secs(1)).await;
    assert!(matches!(ended, Err(ConnectionError::Closed)), "closed: {ended:?}");
    assert_eq!(rig.handed_back().await, ConnectionId::new(1), "handed back, not restarted");

    let mut next = rig.connect();
    next.run_for(Duration::from_secs(1)).await.unwrap();
    assert!(!next.audio().is_empty(), "the next connection is served");
    rig.stop().await;
}

#[tokio::test(start_paused = true)]
async fn while_no_connection_is_up_commands_are_answered_kept_or_held() {
    let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
    let speak = SpeakId::from_parts(1, 7);
    let hotwords = SpeechCmd::SetHotwords { words: vec!["Eileen".to_owned()] };
    let embed =
        SpeechCmd::Embed { request: 3, text: Personal::new("open the timeline".to_owned()) };
    rig.speech(hotwords.clone()).await;
    rig.speech(SpeechCmd::Speak {
        speak,
        voice: "default".to_owned(),
        clauses: vec!["Done.".to_owned()],
        whole_line: "Done.".to_owned(),
    })
    .await;
    rig.speech(SpeechCmd::Check {
        request: 7,
        check: SpeechCheckRequest::NameRecognisable { name: "Eileen".to_owned() },
    })
    .await;
    rig.speech(embed.clone()).await;

    let events = voice(&rig.inputs());
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0], VoiceEvent::SpeakDone { speak }, "SpeakDone, and no ClauseReady");
    assert!(
        matches!(
            &events[1],
            VoiceEvent::CheckResult { request: 7, result: SpeechCheck::Unavailable { .. } }
        ),
        "the check is unavailable: {:?}",
        events[1]
    );

    let mut stand_in = rig.connect();
    stand_in.run_for(ms(500)).await.unwrap();
    assert_eq!(stand_in.got[0], Got::Command(hotwords), "the hotwords first");
    assert_eq!(stand_in.got[1], Got::Command(embed), "then the held command");
    assert!(matches!(stand_in.got[2], Got::Audio { .. }), "then audio: {:?}", stand_in.got[2]);
    rig.stop().await;
}

#[test]
fn at_most_64_commands_wait_for_the_next_connection_and_one_past_that_is_logged() {
    let embed =
        |request| SpeechCmd::Embed { request, text: Personal::new("open the timeline".to_owned()) };
    let (got, captured) = logged(async {
        let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
        for request in 0..=64 {
            rig.speech(embed(request)).await;
        }
        let mut stand_in = rig.connect();
        stand_in.run_for(ms(500)).await.unwrap();
        rig.stop().await;
        stand_in.got
    });

    let held: Vec<u64> = got
        .iter()
        .filter_map(|got| {
            if let Got::Command(SpeechCmd::Embed { request, .. }) = got {
                Some(*request)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(held, (0..64).collect::<Vec<u64>>(), "the first 64, in order, and not the 65th");
    assert!(matches!(got[64], Got::Audio { .. }), "then audio: {:?}", got[64]);
    let dropped: Vec<_> = captured
        .events_named(names::CHANNEL_FULL)
        .into_iter()
        .filter(|event| event.fields.get("channel").is_some_and(|c| c == "audio.speech.held"))
        .collect();
    assert_eq!(dropped.len(), 1, "the one past the maximum is logged: {dropped:?}");
    assert_eq!(dropped[0].fields["capacity"], 64, "with the queue's maximum");
    assert_eq!(dropped[0].fields["times"], 1, "as its first drop");
}

#[test]
fn a_stop_hands_the_connection_back_with_no_warning() {
    let ((), captured) = logged(async {
        let mut rig = Rig::start(clip(Duration::from_secs(10), &[]));
        let mut stand_in = rig.connect();
        stand_in.run_for(Duration::from_secs(2)).await.unwrap();
        rig.stop().await;
        drop(stand_in);
    });

    let warnings: Vec<_> = captured.events.iter().filter(|event| event.level == "WARN").collect();
    assert!(warnings.is_empty(), "a clean run and stop warn of nothing: {warnings:?}");
    let handed_back = captured
        .events
        .iter()
        .find(|event| {
            event.fields.get("message").is_some_and(|m| m == "speech connection handed back")
        })
        .expect("the hand-back is logged");
    assert_eq!(handed_back.level, "INFO", "at info, as an ordinary stop");
    assert_eq!(handed_back.fields["reason"], "the component is stopping", "with its reason");
}

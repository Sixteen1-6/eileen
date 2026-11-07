//! The audio component and its end of the speech pipe (BUILD.md section 6.6, "The core end of
//! the speech pipe", "One replay for start-up, restarts and reloads" and "Mute, and the time
//! without a connection"; decision D81).
//!
//! One task, the component's own, does everything: it captures into the ring from the core's
//! start, takes each connection the [`SpeechTransport`] verified, writes every frame the core
//! sends and reads every frame the speech process sends, and serves the effects routed to it.
//!
//! On each new connection it first sends the latest `SetMode` and `SetHotwords`, which a new
//! process lacks, then the commands it held while no connection was up, then what the ring holds
//! (every frame the speech process has not confirmed, flagged by the ring), then live frames as
//! capture gives them. So start-up, a restart and a reload take one path, and the replay starts
//! after the last turn the speech process finished. It sends a `Ping` every second and answers the
//! speech process's, reports each `Pong` as the speech sidecar's heartbeat, confirms frames in
//! the ring from each `Pong` with no turn open and each `EndOfTurn`, and forwards every event to
//! the core as `Input::Voice`.
//!
//! When a read or a write fails, a write misses its deadline, or the codec refuses a frame, the
//! component hands the connection back to the transport, which only closes it. It never restarts
//! anything: the speech process exits when its connection closes, the transport reports the exit
//! and the core decides the restart (section 6.4). A read that misses its deadline is not a
//! failure: the core counts missed heartbeats, not this component.
//!
//! While no connection is up, capture goes on into the ring, which the component drains from the
//! handoff so the handoff never fills; each `Speak` is answered with `SpeakDone` and no
//! `ClauseReady` (nothing was synthesized), each `Check` with `CheckResult` `Unavailable`, the
//! latest `SetMode` and `SetHotwords` are kept, and other commands wait in a bounded queue for
//! the next connection, any past its maximum dropped and logged.
//!
//! Playback (section 6.6, "Duck, and what waits for W2"): the component starts the render
//! stream of `AudioConfig::playback` as it starts, with the renderer in its callback, and takes
//! each PCM frame the speech process sends straight into the player, never through `Core::step`
//! (section 3.3.7). `Duck`, `Unduck`, `Pause`, `Resume` and `Stop` go to the player, and what it
//! reports reaches the core as `Input::Playback`. The pump takes those events every 10 ms while
//! capture runs, and otherwise while playback has something to do and for
//! [`PLAYBACK_SETTLES`] after, so a muted, silent component still wakes only for its `Ping`.
//! `PlayPhrase` and `Record` are W2 work that the core does not send until then: one that comes
//! anyway fails a `never!` soft assertion and is dropped with a logged reason.
//!
//! Every wait has a deadline: each accept, read and write, a write's covering the whole of it.
//! The pending accept, read and effect are owned futures that live across the loop's turns, so
//! none is dropped part way; a read in flight when a write fails is awaited, within its deadline,
//! so the connection's two halves rejoin before it goes back. The handoff is drained before each
//! write and before that last read, so no wait of the loop's can fill it.
//!
//! A mute never waits on the pipe. While a write or that last read waits, the component keeps
//! taking effects: a mute stops capture at once, and any other effect is parked, in order, for
//! the loop to serve once the wait is over. Audio is written a frame at a time, so a mute that
//! comes during a write lets only the frame already begun finish; the rest of the batch, captured
//! before the mute, is dropped with the ring.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use eileen_contracts::connectors::{SidecarEvent, SidecarId};
use eileen_contracts::core_io::{AudioCmd, Effect, EffectFamily, Input, PlaybackEvent};
use eileen_contracts::ids::{Envelope, MonoTime, TraceCtx, WallTime};
use eileen_contracts::ports::{
    Backpressure, Component, ComponentCtx, ComponentError, EffectSource, InputSink,
};
use eileen_contracts::speech::{
    Frame, FrameBody, FrameError, FrameReader, PcmHeader, SpeechCheck, SpeechCmd,
    SpeechCommandFrame, VoiceEvent,
};
use eileen_trace::names;
use tokio::time::{Instant, Interval, MissedTickBehavior};

use crate::capture::Framer;
use crate::playback::{self, PlayError, Player};
use crate::ring::{HANDOFF_FRAMES, STORE_FRAMES, SpeechRing};
use crate::stream::{AudioStream, RenderInfo};
use crate::transport::{
    AcceptError, ConnectionError, ConnectionId, SpeechConnection, SpeechReader, SpeechTransport,
    SpeechWriter,
};
use crate::{SharedSink, SharedSource};

/// A `Ping` goes this often on a connection.
pub(crate) const PING_EVERY: Duration = Duration::from_secs(1);
/// Each wait for a connection; the next wait follows at once.
const ACCEPT_WITHIN: Duration = Duration::from_secs(5);
/// Each read; a read that misses it is simply started again.
const READ_WITHIN: Duration = Duration::from_secs(1);
/// Each write, the whole of it. One that misses it ends the connection.
const WRITE_WITHIN: Duration = Duration::from_secs(2);
/// How often the ring is drained: into the connection while one is up, from the handoff into
/// the store while none is. A frame is 32 ms, so a frame waits at most this long.
const PUMP_EVERY: Duration = Duration::from_millis(10);
/// Audio frames per write (about a second), so the replay leaves room to read between writes.
const FRAMES_PER_WRITE: usize = 32;
/// Commands held while no connection is up.
pub(crate) const HELD_COMMANDS: usize = 64;
/// Effects that came while a write or a read waited, kept for the loop to serve after it. Past
/// this many the component stops taking effects until the wait is over, and the runtime's own
/// bounded queue holds the rest.
const PARKED_EFFECTS: usize = 64;
/// Bytes asked for in each read.
const READ_BYTES: usize = 16 * 1024;
/// The PC microphone's audio source number ([`eileen_contracts::speech::FrameHeader::source`]).
const MICROPHONE: u16 = 0;
/// How long after playback last had something to do (audio queued, an event reported, a frame or
/// a command taken) the pump still takes its events every 10 ms while capture is off: longer than
/// any endpoint's callback period, so what the callback after a stop, a pause or the queue's last
/// sample reports reaches the core within a pump. Past it the once-a-second report takes them.
const PLAYBACK_SETTLES: Duration = Duration::from_millis(200);

/// The audio component: capture into the speech ring, playback and the core end of the speech
/// pipe. Echo cancellation and the energy fallback join it in later parts.
pub(crate) struct AudioComponent {
    speech: Option<Arc<dyn SpeechTransport>>,
    capture: Option<SharedSource>,
    playback: Option<SharedSink>,
}

impl AudioComponent {
    pub(crate) const fn new(
        speech: Option<Arc<dyn SpeechTransport>>,
        capture: Option<SharedSource>,
        playback: Option<SharedSink>,
    ) -> Self {
        Self { speech, capture, playback }
    }
}

impl Component for AudioComponent {
    fn name(&self) -> &'static str {
        "audio"
    }

    fn accepts(&self) -> &'static [EffectFamily] {
        &[EffectFamily::Audio, EffectFamily::Speech]
    }

    fn start(
        self: Box<Self>,
        ctx: ComponentCtx,
    ) -> Pin<Box<dyn Future<Output = Result<(), ComponentError>> + Send>> {
        Box::pin(run(*self, ctx))
    }
}

type Owned<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A finished read, with the half and buffer it used.
struct ReadDone {
    reader: SpeechReader,
    buf: Box<[u8]>,
    result: Result<usize, ConnectionError>,
}

fn next_read(mut reader: SpeechReader, mut buf: Box<[u8]>) -> Owned<ReadDone> {
    Box::pin(async move {
        let result = reader.read(&mut buf, READ_WITHIN).await;
        ReadDone { reader, buf, result }
    })
}

/// A finished wait for an effect, with the source it came from.
type EffectDone = (Box<dyn EffectSource>, Option<Envelope<Effect>>);

fn next_effect(mut effects: Box<dyn EffectSource>) -> Owned<EffectDone> {
    Box::pin(async move {
        let effect = effects.recv().await;
        (effects, effect)
    })
}

fn next_accept(
    transport: Arc<dyn SpeechTransport>,
) -> Owned<Result<SpeechConnection, AcceptError>> {
    Box::pin(async move { transport.accept(ACCEPT_WITHIN).await })
}

/// Waits for the future if there is one, and for ever if not. A future that finished must be
/// replaced or cleared before the next wait.
async fn maybe<T>(future: Option<&mut Owned<T>>) -> T {
    match future {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

fn ticker(every: Duration) -> Interval {
    let mut interval = tokio::time::interval(every);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

async fn run(component: AudioComponent, ctx: ComponentCtx) -> Result<(), ComponentError> {
    let ComponentCtx { inputs, effects, shutdown, .. } = ctx;
    let playback = component.playback.as_ref().and_then(Playback::start);
    let mut speech = Link::new(inputs, effects, component.speech, component.capture, playback);
    speech.capture.set(true);
    let mut stop = shutdown.wait();
    let mut pump = ticker(PUMP_EVERY);
    let mut ping = ticker(PING_EVERY);
    // Frames are waiting to be written at once, beyond the batch just written.
    let mut more = false;
    loop {
        // Effects parked while a write or a read waited go first, in the order they came.
        if let Some(envelope) = speech.incoming.parked.pop_front() {
            speech.on_effect(envelope).await;
            continue;
        }
        if speech.incoming.next.is_none() {
            // The effect source has ended.
            break;
        }
        speech.rearm_accept();
        let drains = speech.drains();
        let reading = speech.live.as_mut().and_then(|live| live.reading.as_mut());
        tokio::select! {
            biased;
            () = &mut stop => break,
            done = maybe(speech.incoming.next.as_mut()) => {
                speech.incoming.arrived(done, &mut speech.capture);
            }
            done = maybe(reading) => speech.on_read(done).await,
            accepted = maybe(speech.accepting.as_mut()) => {
                speech.accepting = None;
                if speech.on_accept(accepted).await {
                    ping.reset();
                    more = true;
                }
            }
            // Before the replay's next batch, so a long replay still pings every second.
            _ = ping.tick() => speech.on_second().await,
            () = std::future::ready(()), if more => more = speech.pump().await,
            _ = pump.tick(), if drains => more = speech.pump().await,
        }
    }
    speech.finish().await;
    Ok(())
}

/// Why a connection went back.
enum Ended {
    /// A read or a write failed.
    Connection(ConnectionError),
    /// The codec refused a frame from the speech process.
    Refused(FrameError),
    /// The component is stopping.
    Shutdown,
}

impl fmt::Display for Ended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => write!(f, "{error}"),
            Self::Refused(error) => write!(f, "the codec refused a frame: {error}"),
            Self::Shutdown => write!(f, "the component is stopping"),
        }
    }
}

/// The connection up now.
struct Live {
    id: ConnectionId,
    writer: SpeechWriter,
    frames: FrameReader,
    /// The read in flight; `None` only while a finished one is handled.
    reading: Option<Owned<ReadDone>>,
}

/// The effects routed to the component: the wait for the next one, and those that came while a
/// write or a read waited.
struct Incoming {
    /// The wait for the next effect; `None` once the source has ended.
    next: Option<Owned<EffectDone>>,
    /// Effects that came while a write or a read waited, oldest first; at most
    /// [`PARKED_EFFECTS`].
    parked: VecDeque<Envelope<Effect>>,
}

impl Incoming {
    /// Takes what the wait for an effect gave and starts the next wait. A mute is served at
    /// once, through `capture`, and drops any `SetCapture` still parked, which it comes after;
    /// any other effect is parked for the loop.
    fn arrived(&mut self, done: EffectDone, capture: &mut Capture) {
        let (source, effect) = done;
        let Some(envelope) = effect else {
            self.next = None;
            return;
        };
        self.next = Some(next_effect(source));
        if matches!(envelope.body, Effect::Audio(AudioCmd::SetCapture(false))) {
            self.parked
                .retain(|parked| !matches!(parked.body, Effect::Audio(AudioCmd::SetCapture(_))));
            capture.set(false);
        } else {
            self.parked.push_back(envelope);
        }
    }
}

/// Drops an audio command the audio series does not serve, which the core sends only from W2
/// (section 6.6): a warning with its reason, and a failed `never!` soft assertion, so a test or
/// the simulator that sends one fails while a running Eileen carries on.
fn unserved(command: &'static str, reason: &'static str) {
    tracing::warn!(command, reason, "an audio command the audio series does not serve was dropped");
    eileen_stdx::never!(true, "the core sent {command}, which the audio series does not serve");
}

/// Awaits `work` while taking effects as they come: a mute stops capture before `work` ends,
/// and any other effect is parked for the loop. With [`PARKED_EFFECTS`] parked it takes no more
/// until `work` ends.
async fn racing<T>(
    work: impl Future<Output = T>,
    incoming: &mut Incoming,
    capture: &mut Capture,
) -> T {
    let mut work = std::pin::pin!(work);
    loop {
        let room = incoming.parked.len() < PARKED_EFFECTS;
        tokio::select! {
            biased;
            out = &mut work => return out,
            done = maybe(incoming.next.as_mut()), if room => incoming.arrived(done, capture),
        }
    }
}

/// The microphone's side: its source, the stream while capture runs, and the ring it fills.
struct Capture {
    ring: SpeechRing,
    source: Option<SharedSource>,
    stream: Option<Box<dyn AudioStream>>,
    /// Mutes so far, so a write of audio can tell that one came while it waited.
    mutes: u64,
}

impl Capture {
    /// Starts or stops capture. Stopping empties the ring, so nothing captured before a mute is
    /// written after it, and the first frame after capture resumes carries the break flag.
    fn set(&mut self, on: bool) {
        if !on {
            if let Some(stream) = self.stream.take() {
                stream.stop();
            }
            self.ring.capture_stopped();
            self.mutes = self.mutes.wrapping_add(1);
            return;
        }
        if self.stream.is_some() {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let mut source = source.lock().unwrap_or_else(PoisonError::into_inner);
        let framer = match Framer::new(source.format(), self.ring.open_capture()) {
            Ok(framer) => framer,
            Err(error) => {
                tracing::warn!(%error, format = ?source.format(), "capture did not start");
                return;
            }
        };
        let started = source.start(framer.into_callback());
        drop(source);
        match started {
            Ok(stream) => self.stream = Some(stream),
            Err(error) => tracing::warn!(%error, "capture did not start"),
        }
    }
}

/// Eileen's playback: the player, the render stream its renderer fills, and when it last had
/// something to do.
struct Playback {
    player: Player,
    stream: Box<dyn AudioStream>,
    /// When it last had something to do; `None` before it first had.
    active_at: Option<Instant>,
}

impl Playback {
    /// Starts the render stream of `sink`, with the renderer in its callback. A sink at a rate
    /// playback has no conversion to, or one whose stream does not start, leaves the component
    /// without playback, logged.
    fn start(sink: &SharedSink) -> Option<Self> {
        let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
        let format = sink.format();
        let (player, mut renderer) =
            match playback::playback(format, playback::default_queue(format)) {
                Ok(halves) => halves,
                Err(error) => {
                    tracing::warn!(%error, ?format, "playback did not start");
                    return None;
                }
            };
        let started =
            sink.start(Box::new(move |out: &mut [f32], info: RenderInfo| renderer.fill(out, info)));
        drop(sink);
        match started {
            Ok(stream) => Some(Self { player, stream, active_at: None }),
            Err(error) => {
                tracing::warn!(%error, ?format, "playback did not start");
                None
            }
        }
    }

    /// Notes that playback has something to do now.
    fn touch(&mut self) {
        self.active_at = Some(Instant::now());
    }

    /// Whether the pump still takes its events: it had something to do within
    /// [`PLAYBACK_SETTLES`].
    fn active(&self) -> bool {
        self.active_at.is_some_and(|at| at.elapsed() < PLAYBACK_SETTLES)
    }

    /// The next event playback reports, if one waits. An event, or audio still queued, counts as
    /// something to do.
    fn next_event(&mut self) -> Option<PlaybackEvent> {
        let event = self.player.next_event();
        if event.is_some() || self.player.has_audio() {
            self.touch();
        }
        event
    }
}

/// The component's state between the loop's turns.
struct Link {
    inputs: Box<dyn InputSink>,
    incoming: Incoming,
    capture: Capture,
    /// Playback, if `AudioConfig::playback` gave a sink and its stream started.
    playback: Option<Playback>,
    transport: Option<Arc<dyn SpeechTransport>>,
    /// The wait for the next connection, while none is up and the transport is open.
    accepting: Option<Owned<Result<SpeechConnection, AcceptError>>>,
    live: Option<Live>,
    /// The latest `SetMode` and `SetHotwords`, for every new connection.
    mode: Option<SpeechCommandFrame>,
    hotwords: Option<SpeechCommandFrame>,
    /// Other commands that came while no connection was up.
    held: VecDeque<SpeechCommandFrame>,
    held_dropped: u64,
    /// Frames the ring lost, in all, as the last report gave them.
    ring_dropped: u64,
    /// Inputs the core's queue refused, in all; the count the last report gave; and the
    /// capacity the queue gave with its last refusal.
    inputs_refused: u64,
    inputs_reported: u64,
    inputs_capacity: usize,
    /// The bytes of the next write, kept to reuse their room.
    out: Vec<u8>,
    /// Where each audio frame in `out` ends, so a write of audio can stop between two frames.
    cuts: Vec<usize>,
}

impl Link {
    fn new(
        inputs: Box<dyn InputSink>,
        effects: Box<dyn EffectSource>,
        transport: Option<Arc<dyn SpeechTransport>>,
        source: Option<SharedSource>,
        playback: Option<Playback>,
    ) -> Self {
        Self {
            inputs,
            incoming: Incoming {
                next: Some(next_effect(effects)),
                parked: VecDeque::with_capacity(PARKED_EFFECTS),
            },
            capture: Capture {
                ring: SpeechRing::new(STORE_FRAMES, HANDOFF_FRAMES),
                source,
                stream: None,
                mutes: 0,
            },
            playback,
            transport,
            accepting: None,
            live: None,
            mode: None,
            hotwords: None,
            held: VecDeque::with_capacity(HELD_COMMANDS),
            held_dropped: 0,
            ring_dropped: 0,
            inputs_refused: 0,
            inputs_reported: 0,
            inputs_capacity: 0,
            out: Vec::with_capacity(FRAMES_PER_WRITE * 1_056),
            cuts: Vec::with_capacity(FRAMES_PER_WRITE),
        }
    }

    /// Starts waiting for the next connection when none is up and none is awaited.
    fn rearm_accept(&mut self) {
        if self.live.is_none() && self.accepting.is_none() {
            self.accepting =
                self.transport.as_ref().map(|transport| next_accept(Arc::clone(transport)));
        }
    }

    /// Takes what an accept gave; true when a connection is up and its first frames written.
    async fn on_accept(&mut self, accepted: Result<SpeechConnection, AcceptError>) -> bool {
        match accepted {
            Ok(connection) => self.connected(connection).await,
            Err(AcceptError::TimedOut { within }) => {
                tracing::debug!(?within, "speech is still not connected");
                false
            }
            Err(AcceptError::Shutdown) => {
                tracing::info!(
                    held = self.held.len(),
                    "the speech transport has shut down; no more connections, held commands dropped"
                );
                self.transport = None;
                self.held.clear();
                false
            }
        }
    }

    /// A new connection: the latest mode and hotwords, then the held commands; the ring's
    /// replay follows from the pump.
    async fn connected(&mut self, connection: SpeechConnection) -> bool {
        let id = connection.id();
        let (reader, writer) = connection.split();
        let buf = vec![0_u8; READ_BYTES].into_boxed_slice();
        self.live = Some(Live {
            id,
            writer,
            frames: FrameReader::for_core_after_hello(),
            reading: Some(next_read(reader, buf)),
        });
        self.capture.ring.connected();
        tracing::info!(connection = %id, "speech connected");
        let first: Vec<SpeechCommandFrame> =
            self.mode.iter().chain(self.hotwords.iter()).chain(self.held.iter()).cloned().collect();
        for command in first {
            self.encode_command(command);
        }
        match self.write_out().await {
            Ok(()) => {
                // Held until written, so a connection that breaks at once loses none of them.
                self.held.clear();
                true
            }
            Err(ended) => {
                self.close(None, ended).await;
                false
            }
        }
    }

    async fn on_effect(&mut self, envelope: Envelope<Effect>) {
        let Envelope { ctx, body, .. } = envelope;
        let family = body.family();
        if let Effect::Speech(cmd) = body {
            // `span` is meant to name the core's span the command belongs to (section 2.13), the
            // `speak` span for a `Speak`, so the speech process can open `tts.synth` under it.
            // An effect's envelope carries no span id, so it stays 0 until the plan settles how
            // one reaches this component; until then the speech series opens nothing under it.
            self.on_command(SpeechCommandFrame { ctx, span: 0, cmd }).await;
        } else if let Effect::Audio(cmd) = body {
            self.on_audio(&cmd);
        } else {
            tracing::warn!(
                ?family,
                "an effect routed to the audio component that it does not take"
            );
        }
    }

    fn on_audio(&mut self, cmd: &AudioCmd) {
        match cmd {
            AudioCmd::SetCapture(on) => self.capture.set(*on),
            AudioCmd::Duck { .. }
            | AudioCmd::Unduck
            | AudioCmd::Pause { .. }
            | AudioCmd::Resume { .. }
            | AudioCmd::Stop { .. } => match self.playback.as_mut() {
                Some(playback) => {
                    playback.player.apply(cmd);
                    playback.touch();
                }
                None => tracing::debug!(?cmd, "there is no playback; the command changes nothing"),
            },
            AudioCmd::Earcon(earcon) => {
                tracing::debug!(?earcon, "earcons are played by a later part of the audio series");
            }
            AudioCmd::PlayPhrase { .. } => unserved(
                "PlayPhrase",
                "the phrase cache is W2 work; until then the core speaks every reply through Speak",
            ),
            AudioCmd::Record { .. } => {
                unserved("Record", "recording for the first run and enrollment is W2 work")
            }
        }
    }

    /// Plays a PCM frame from the speech process: straight into the player, never through the
    /// core (section 3.3.7).
    fn play(&mut self, header: PcmHeader, samples: &[i16]) {
        let Some(playback) = self.playback.as_mut() else {
            tracing::debug!(
                speak = %header.speak,
                clause = header.clause,
                "there is no playback; PCM dropped"
            );
            return;
        };
        let pushed = playback.player.push(header, samples);
        playback.touch();
        match pushed {
            // A frame with no room was logged by the player as `channel.full`.
            Ok(()) | Err(PlayError::Full { .. } | PlayError::Utterances { .. }) => {}
            // Synthesis runs ahead of playback, so clauses of a stopped utterance still come.
            Err(error @ PlayError::Ended) => tracing::debug!(
                speak = %header.speak,
                clause = header.clause,
                %error,
                "PCM for an utterance that takes no more audio was refused"
            ),
            Err(error @ PlayError::Rate { .. }) => tracing::warn!(
                speak = %header.speak,
                clause = header.clause,
                %error,
                "PCM at a rate playback does not take was refused"
            ),
        }
    }

    /// Passes what playback reported to the core, as `Input::Playback`.
    fn report_playback(&mut self) {
        while let Some(event) = self.playback.as_mut().and_then(Playback::next_event) {
            self.send(Input::Playback(event), TraceCtx::default());
        }
    }

    async fn on_command(&mut self, command: SpeechCommandFrame) {
        if matches!(command.cmd, SpeechCmd::SetMode(_)) {
            self.mode = Some(command.clone());
        } else if matches!(command.cmd, SpeechCmd::SetHotwords { .. }) {
            self.hotwords = Some(command.clone());
        }
        if self.live.is_some() {
            self.encode_command(command.clone());
            match self.write_out().await {
                Ok(()) => return,
                // The connection broke under this command: it is served below as one that came
                // while no connection was up.
                Err(ended) => self.close(None, ended).await,
            }
        }
        let ctx = command.ctx;
        match command.cmd {
            SpeechCmd::SetMode(_) | SpeechCmd::SetHotwords { .. } => {}
            SpeechCmd::Speak { speak, .. } => {
                self.send(Input::Voice(VoiceEvent::SpeakDone { speak }), ctx);
            }
            SpeechCmd::Check { request, .. } => {
                let result =
                    SpeechCheck::Unavailable { reason: "speech is not connected".to_owned() };
                self.send(Input::Voice(VoiceEvent::CheckResult { request, result }), ctx);
            }
            SpeechCmd::Cancel { .. }
            | SpeechCmd::Resume { .. }
            | SpeechCmd::Embed { .. }
            | SpeechCmd::LoadModel { .. }
            | SpeechCmd::UnloadModel { .. }
            | SpeechCmd::Enroll { .. }
            | SpeechCmd::CloneFromRecording { .. }
            | SpeechCmd::DeleteVoice { .. } => self.hold(command),
        }
    }

    fn hold(&mut self, command: SpeechCommandFrame) {
        if self.transport.is_none() {
            tracing::debug!(
                cmd = ?command.cmd,
                "no speech connection will come; the command is dropped"
            );
        } else if self.held.len() < HELD_COMMANDS {
            self.held.push_back(command);
        } else {
            self.held_dropped = self.held_dropped.saturating_add(1);
            tracing::warn!(
                name: names::CHANNEL_FULL,
                channel = "audio.speech.held",
                capacity = HELD_COMMANDS,
                times = self.held_dropped,
            );
        }
    }

    /// Passes playback's events to the core, then writes the next batch of the ring's frames;
    /// true when more are waiting at once. With no connection up it moves the handoff's frames
    /// into the store instead.
    async fn pump(&mut self) -> bool {
        self.report_playback();
        if self.live.is_none() {
            self.capture.ring.pull();
            return false;
        }
        let mut batch = 0;
        while batch < FRAMES_PER_WRITE {
            let Some(frame) = self.capture.ring.next_frame() else {
                break;
            };
            let written = Frame::encode_audio_into(
                frame.flags,
                MICROPHONE,
                frame.seq,
                frame.samples,
                &mut self.out,
            );
            match written {
                Ok(()) => self.cuts.push(self.out.len()),
                Err(error) => tracing::warn!(%error, "an audio frame did not encode"),
            }
            batch += 1;
        }
        if batch == 0 {
            return false;
        }
        match self.write_out().await {
            Ok(()) => batch == FRAMES_PER_WRITE,
            Err(ended) => {
                self.close(None, ended).await;
                false
            }
        }
    }

    /// Once a second: a `Ping` on the connection, the counts of what was dropped, and any event
    /// playback reported after the pump stopped taking them.
    async fn on_second(&mut self) {
        self.report();
        self.report_playback();
        if self.live.is_none() {
            return;
        }
        if let Err(error) = Frame::new(FrameBody::Ping).encode_into(&mut self.out) {
            tracing::warn!(%error, "a Ping did not encode");
        }
        if let Err(ended) = self.write_out().await {
            self.close(None, ended).await;
        }
    }

    /// Logs what was dropped, when it changed since the last report: frames the ring lost and
    /// inputs the core's queue refused, each as its running total, as every other
    /// `channel.full` gives it.
    fn report(&mut self) {
        let dropped = self.capture.ring.take_dropped();
        if dropped > 0 {
            self.ring_dropped = self.ring_dropped.saturating_add(dropped);
            tracing::warn!(
                name: names::CHANNEL_FULL,
                channel = "audio.speech.ring",
                capacity = STORE_FRAMES,
                times = self.ring_dropped,
            );
        }
        if self.inputs_refused != self.inputs_reported {
            self.inputs_reported = self.inputs_refused;
            tracing::warn!(
                name: names::CHANNEL_FULL,
                channel = "core.inputs",
                capacity = self.inputs_capacity,
                times = self.inputs_refused,
            );
        }
    }

    async fn on_read(&mut self, done: ReadDone) {
        let ReadDone { reader, buf, result } = done;
        if let Some(live) = self.live.as_mut() {
            live.reading = None;
        }
        let outcome = match result {
            Ok(n) => self.take_bytes(buf.get(..n).unwrap_or_default()).await,
            Err(ConnectionError::TimedOut { .. }) => Ok(()),
            Err(error) => Err(Ended::Connection(error)),
        };
        match outcome {
            Ok(()) => {
                if let Some(live) = self.live.as_mut() {
                    live.reading = Some(next_read(reader, buf));
                }
            }
            Err(ended) => self.close(Some(reader), ended).await,
        }
    }

    /// Takes every frame in `bytes`, then answers the `Ping`s among them with one `Pong`: a
    /// `Pong` carries nothing of a `Ping`'s, and one write keeps the read's answer within one
    /// write's deadline however many `Ping`s came.
    async fn take_bytes(&mut self, mut bytes: &[u8]) -> Result<(), Ended> {
        let mut pinged = false;
        while !bytes.is_empty() {
            let Some(live) = self.live.as_mut() else {
                break;
            };
            let (taken, frame) = live.frames.feed(bytes).map_err(Ended::Refused)?;
            bytes = bytes.get(taken..).unwrap_or_default();
            match frame {
                Some(frame) => pinged |= self.on_frame(frame),
                None if taken == 0 => break,
                None => {}
            }
        }
        if !pinged {
            return Ok(());
        }
        if let Err(error) = Frame::new(FrameBody::Pong).encode_into(&mut self.out) {
            tracing::warn!(%error, "a Pong did not encode");
        }
        self.write_out().await
    }

    /// Takes one frame from the speech process; true for a `Ping`, which the caller answers.
    fn on_frame(&mut self, frame: Frame) -> bool {
        self.capture.ring.on_speech_frame(&frame);
        match frame.body {
            FrameBody::Ping => true,
            FrameBody::Pong => {
                self.send(
                    Input::Sidecar(SidecarId::Speech, SidecarEvent::Heartbeat),
                    TraceCtx::default(),
                );
                false
            }
            FrameBody::Event(event) => {
                self.send(Input::Voice(event.event), event.ctx);
                false
            }
            FrameBody::Pcm { header, samples } => {
                self.play(header, &samples);
                false
            }
            // The reader refuses these from the speech process.
            FrameBody::Audio(_) | FrameBody::Command(_) | FrameBody::Hello(_) => false,
        }
    }

    /// Sends an input to the core. The runtime's input queue gives inputs their place in the
    /// replay order and their times, so the envelope leaves them zero.
    fn send(&mut self, body: Input, ctx: TraceCtx) {
        let input = Envelope { seq: 0, at: WallTime(0), mono: MonoTime(0), ctx, body };
        if let Err(Backpressure { capacity }) = self.inputs.send(input) {
            self.inputs_refused = self.inputs_refused.saturating_add(1);
            self.inputs_capacity = capacity;
        }
    }

    fn encode_command(&mut self, command: SpeechCommandFrame) {
        if let Err(error) = Frame::new(FrameBody::Command(command)).encode_into(&mut self.out) {
            tracing::warn!(%error, "a speech command did not encode and was dropped");
        }
    }

    /// Writes what was encoded, if there is a connection to write it on, within one deadline for
    /// the whole of it. Moves the handoff's frames into the store first, so the wait loses none
    /// of them. Audio goes a frame at a time: once a mute comes, only the frame already begun is
    /// finished, and the rest of the batch, captured before the mute, is dropped as the ring's
    /// frames were.
    async fn write_out(&mut self) -> Result<(), Ended> {
        self.capture.ring.pull();
        let result = self.write_pieces().await;
        self.out.clear();
        self.cuts.clear();
        result
    }

    async fn write_pieces(&mut self) -> Result<(), Ended> {
        let Self { live, out, cuts, incoming, capture, .. } = self;
        let Some(live) = live.as_mut() else {
            return Ok(());
        };
        let deadline = Instant::now() + WRITE_WITHIN;
        let mutes = capture.mutes;
        let mut from = 0;
        // Without cuts, `out` is one piece and holds no audio.
        for end in cuts.iter().copied().chain(std::iter::once(out.len())) {
            if end <= from {
                continue;
            }
            if capture.mutes != mutes {
                break;
            }
            let piece = out.get(from..end).unwrap_or_default();
            let within = deadline.saturating_duration_since(Instant::now());
            racing(live.writer.write_all(piece, within), incoming, capture)
                .await
                .map_err(Ended::Connection)?;
            from = end;
        }
        Ok(())
    }

    /// Hands the connection back to the transport, with its read half: `reader` when the caller
    /// holds it, or else the read in flight's, awaited within its deadline.
    async fn close(&mut self, reader: Option<SpeechReader>, ended: Ended) {
        let reader = match reader {
            Some(reader) => Some(reader),
            None => self.finish_read().await,
        };
        let Some(live) = self.live.take() else {
            return;
        };
        if matches!(ended, Ended::Shutdown) {
            tracing::info!(connection = %live.id, reason = %ended, "speech connection handed back");
        } else {
            tracing::warn!(connection = %live.id, reason = %ended, "speech connection handed back");
        }
        let Some(reader) = reader else {
            return;
        };
        match (SpeechConnection::rejoin(reader, live.writer), self.transport.as_ref()) {
            (Ok(connection), Some(transport)) => transport.close(connection),
            (Ok(connection), None) => drop(connection),
            (Err(halves), _) => {
                tracing::warn!(error = %halves, "a connection's halves did not rejoin");
            }
        }
    }

    /// Waits for the read in flight, within its deadline, and takes the frames it brought, so an
    /// event the speech process sent before the connection broke still reaches the core. Moves
    /// the handoff's frames into the store first, so the wait loses none of them, and takes
    /// effects meanwhile, so a mute does not wait for it. Returns the read half.
    async fn finish_read(&mut self) -> Option<SpeechReader> {
        self.capture.ring.pull();
        let reading = self.live.as_mut()?.reading.take()?;
        let ReadDone { reader, buf, result } =
            racing(reading, &mut self.incoming, &mut self.capture).await;
        if let Ok(n) = result
            && let Err(ended) = self.take_bytes(buf.get(..n).unwrap_or_default()).await
        {
            tracing::debug!(reason = %ended, "the last read of a closing connection ended early");
        }
        Some(reader)
    }

    /// Whether the pump runs: capture runs, or playback had something to do within
    /// [`PLAYBACK_SETTLES`]. With capture off the ring holds nothing (a mute empties it, and with
    /// no microphone nothing ever arrives), so a muted component that is not speaking wakes once
    /// a second for its `Ping`, not a hundred times. A new connection's replay runs on the loop's
    /// `more` flag, not on this tick.
    fn drains(&self) -> bool {
        self.capture.stream.is_some() || self.playback.as_ref().is_some_and(Playback::active)
    }

    /// The component is stopping: capture and playback stop and the connection goes back.
    async fn finish(&mut self) {
        if let Some(stream) = self.capture.stream.take() {
            stream.stop();
        }
        if let Some(playback) = self.playback.take() {
            playback.stream.stop();
        }
        self.close(None, Ended::Shutdown).await;
    }
}

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
//! Every wait has a deadline: each accept, read and write. The pending accept, read and effect
//! are owned futures that live across the loop's turns, so none is dropped part way; a read in
//! flight when a write fails is awaited, within its deadline, so the connection's two halves
//! rejoin before it goes back.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use eileen_contracts::connectors::{SidecarEvent, SidecarId};
use eileen_contracts::core_io::{AudioCmd, Effect, EffectFamily, Input};
use eileen_contracts::ids::{Envelope, MonoTime, TraceCtx, WallTime};
use eileen_contracts::ports::{Component, ComponentCtx, ComponentError, EffectSource, InputSink};
use eileen_contracts::speech::{
    Frame, FrameBody, FrameError, FrameReader, SpeechCheck, SpeechCmd, SpeechCommandFrame,
    VoiceEvent,
};
use eileen_trace::names;
use tokio::time::{Interval, MissedTickBehavior};

use crate::SharedSource;
use crate::capture::Framer;
use crate::ring::{HANDOFF_FRAMES, STORE_FRAMES, SpeechRing};
use crate::stream::AudioStream;
use crate::transport::{
    AcceptError, ConnectionError, ConnectionId, SpeechConnection, SpeechReader, SpeechTransport,
    SpeechWriter,
};

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
/// Bytes asked for in each read.
const READ_BYTES: usize = 16 * 1024;
/// The PC microphone's audio source number ([`eileen_contracts::speech::FrameHeader::source`]).
const MICROPHONE: u16 = 0;

/// The audio component: capture into the speech ring and the core end of the speech pipe.
/// Playback, echo cancellation and the energy fallback join it in later parts.
pub(crate) struct AudioComponent {
    speech: Option<Arc<dyn SpeechTransport>>,
    capture: Option<SharedSource>,
}

impl AudioComponent {
    pub(crate) const fn new(
        speech: Option<Arc<dyn SpeechTransport>>,
        capture: Option<SharedSource>,
    ) -> Self {
        Self { speech, capture }
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
    let mut speech = Link::new(inputs, component.speech, component.capture);
    speech.set_capture(true);
    let mut stop = shutdown.wait();
    let mut effect = Some(next_effect(effects));
    let mut pump = ticker(PUMP_EVERY);
    let mut ping = ticker(PING_EVERY);
    // Frames are waiting to be written at once, beyond the batch just written.
    let mut more = false;
    loop {
        speech.rearm_accept();
        let drains = speech.drains();
        let reading = speech.live.as_mut().and_then(|live| live.reading.as_mut());
        tokio::select! {
            biased;
            () = &mut stop => break,
            (effects, next) = maybe(effect.as_mut()) => match next {
                Some(envelope) => {
                    effect = Some(next_effect(effects));
                    speech.on_effect(envelope).await;
                }
                None => break,
            },
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

/// The component's state between the loop's turns.
struct Link {
    inputs: Box<dyn InputSink>,
    transport: Option<Arc<dyn SpeechTransport>>,
    /// The wait for the next connection, while none is up and the transport is open.
    accepting: Option<Owned<Result<SpeechConnection, AcceptError>>>,
    live: Option<Live>,
    ring: SpeechRing,
    source: Option<SharedSource>,
    stream: Option<Box<dyn AudioStream>>,
    /// The latest `SetMode` and `SetHotwords`, for every new connection.
    mode: Option<SpeechCommandFrame>,
    hotwords: Option<SpeechCommandFrame>,
    /// Other commands that came while no connection was up.
    held: VecDeque<SpeechCommandFrame>,
    held_dropped: u64,
    /// Inputs the core's queue refused since the last report.
    inputs_refused: u64,
    /// The bytes of the next write, kept to reuse their room.
    out: Vec<u8>,
}

impl Link {
    fn new(
        inputs: Box<dyn InputSink>,
        transport: Option<Arc<dyn SpeechTransport>>,
        source: Option<SharedSource>,
    ) -> Self {
        Self {
            inputs,
            transport,
            accepting: None,
            live: None,
            ring: SpeechRing::new(STORE_FRAMES, HANDOFF_FRAMES),
            source,
            stream: None,
            mode: None,
            hotwords: None,
            held: VecDeque::with_capacity(HELD_COMMANDS),
            held_dropped: 0,
            inputs_refused: 0,
            out: Vec::with_capacity(FRAMES_PER_WRITE * 1_056),
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
        self.ring.connected();
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
            AudioCmd::SetCapture(on) => self.set_capture(*on),
            AudioCmd::Duck { .. }
            | AudioCmd::Unduck
            | AudioCmd::Pause { .. }
            | AudioCmd::Resume { .. }
            | AudioCmd::Stop { .. }
            | AudioCmd::PlayPhrase { .. }
            | AudioCmd::Earcon(_)
            | AudioCmd::Record { .. } => {
                tracing::debug!(
                    ?cmd,
                    "this audio command is served by a later part of the audio series"
                );
            }
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

    /// Starts or stops capture. Stopping empties the ring, so nothing captured before a mute is
    /// written after it, and the first frame after capture resumes carries the break flag.
    fn set_capture(&mut self, on: bool) {
        if !on {
            if let Some(stream) = self.stream.take() {
                stream.stop();
            }
            self.ring.capture_stopped();
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

    /// Writes the next batch of the ring's frames; true when more are waiting at once. With no
    /// connection up it moves the handoff's frames into the store instead.
    async fn pump(&mut self) -> bool {
        if self.live.is_none() {
            self.ring.pull();
            return false;
        }
        let mut batch = 0;
        while batch < FRAMES_PER_WRITE {
            let Some(frame) = self.ring.next_frame() else {
                break;
            };
            let written = Frame::encode_audio_into(
                frame.flags,
                MICROPHONE,
                frame.seq,
                frame.samples,
                &mut self.out,
            );
            if let Err(error) = written {
                tracing::warn!(%error, "an audio frame did not encode");
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

    /// Once a second: a `Ping` on the connection, and the counts of what was dropped.
    async fn on_second(&mut self) {
        self.report();
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

    fn report(&mut self) {
        let dropped = self.ring.take_dropped();
        if dropped > 0 {
            tracing::warn!(
                name: names::CHANNEL_FULL,
                channel = "audio.speech.ring",
                capacity = STORE_FRAMES,
                times = dropped,
            );
        }
        let refused = std::mem::take(&mut self.inputs_refused);
        if refused > 0 {
            tracing::warn!(name: names::CHANNEL_FULL, channel = "core.inputs", times = refused);
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

    /// Takes every frame in `bytes`. A failed answer to a `Ping` does not stop it: the frames
    /// after it are still taken, so no event already read is lost, and the failure is returned
    /// at the end.
    async fn take_bytes(&mut self, mut bytes: &[u8]) -> Result<(), Ended> {
        let mut failed = None;
        while !bytes.is_empty() {
            let Some(live) = self.live.as_mut() else {
                break;
            };
            let (taken, frame) = live.frames.feed(bytes).map_err(Ended::Refused)?;
            bytes = bytes.get(taken..).unwrap_or_default();
            match frame {
                Some(frame) => {
                    if let Err(ended) = self.on_frame(frame).await {
                        failed.get_or_insert(ended);
                    }
                }
                None if taken == 0 => break,
                None => {}
            }
        }
        failed.map_or(Ok(()), Err)
    }

    async fn on_frame(&mut self, frame: Frame) -> Result<(), Ended> {
        self.ring.on_speech_frame(&frame);
        match frame.body {
            FrameBody::Ping => {
                if let Err(error) = Frame::new(FrameBody::Pong).encode_into(&mut self.out) {
                    tracing::warn!(%error, "a Pong did not encode");
                }
                self.write_out().await?;
            }
            FrameBody::Pong => {
                self.send(
                    Input::Sidecar(SidecarId::Speech, SidecarEvent::Heartbeat),
                    TraceCtx::default(),
                );
            }
            FrameBody::Event(event) => self.send(Input::Voice(event.event), event.ctx),
            FrameBody::Pcm { header, .. } => {
                tracing::debug!(
                    speak = %header.speak,
                    clause = header.clause,
                    "PCM is played by a later part of the audio series"
                );
            }
            // The reader refuses these from the speech process.
            FrameBody::Audio(_) | FrameBody::Command(_) | FrameBody::Hello(_) => {}
        }
        Ok(())
    }

    /// Sends an input to the core. The runtime's input queue gives inputs their place in the
    /// replay order and their times, so the envelope leaves them zero.
    fn send(&mut self, body: Input, ctx: TraceCtx) {
        let input = Envelope { seq: 0, at: WallTime(0), mono: MonoTime(0), ctx, body };
        if self.inputs.send(input).is_err() {
            self.inputs_refused = self.inputs_refused.saturating_add(1);
        }
    }

    fn encode_command(&mut self, command: SpeechCommandFrame) {
        if let Err(error) = Frame::new(FrameBody::Command(command)).encode_into(&mut self.out) {
            tracing::warn!(%error, "a speech command did not encode and was dropped");
        }
    }

    /// Writes what was encoded, if there is a connection to write it on.
    async fn write_out(&mut self) -> Result<(), Ended> {
        let result = match self.live.as_mut() {
            Some(live) if !self.out.is_empty() => {
                live.writer.write_all(&self.out, WRITE_WITHIN).await.map_err(Ended::Connection)
            }
            _ => Ok(()),
        };
        self.out.clear();
        result
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
    /// the handoff's frames into the store first, so the wait loses none of them. Returns the
    /// read half.
    async fn finish_read(&mut self) -> Option<SpeechReader> {
        self.ring.pull();
        let reading = self.live.as_mut()?.reading.take()?;
        let ReadDone { reader, buf, result } = reading.await;
        if let Ok(n) = result
            && let Err(ended) = self.take_bytes(buf.get(..n).unwrap_or_default()).await
        {
            tracing::debug!(reason = %ended, "the last read of a closing connection ended early");
        }
        Some(reader)
    }

    /// Whether the ring needs draining: capture runs. With capture off the ring holds nothing (a
    /// mute empties it, and with no microphone nothing ever arrives), so a muted component wakes
    /// once a second for its `Ping`, not a hundred times. A new connection's replay runs on the
    /// loop's `more` flag, not on this tick.
    const fn drains(&self) -> bool {
        self.stream.is_some()
    }

    /// The component is stopping: capture stops and the connection goes back.
    async fn finish(&mut self) {
        if let Some(stream) = self.stream.take() {
            stream.stop();
        }
        self.close(None, Ended::Shutdown).await;
    }
}

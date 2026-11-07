//! The audio component's waits (BUILD.md section 6.6, "The core end of the speech pipe" and
//! "Mute, and the time without a connection").
//!
//! Every wait has a deadline: each accept, read and write, a write's covering the whole of it.
//! The pending accept, read and effect are owned futures that live across the loop's turns, so
//! none is dropped part way; a read in flight when a write fails is awaited, within its deadline,
//! so the connection's two halves rejoin before it goes back.
//!
//! A mute and the playback commands never wait on the pipe. While a write or a closing
//! connection's last read waits, the component keeps taking effects ([`racing`]): a mute stops
//! capture at once, a playback command goes to the player at once while playback runs, so a duck
//! or a stop lands by the next output callback (invariant 2), and any other effect is parked, in
//! order, for the loop to serve once the wait is over.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::core_io::{AudioCmd, Effect};
use eileen_contracts::ids::Envelope;
use eileen_contracts::ports::EffectSource;
use tokio::time::{Interval, MissedTickBehavior};

use super::playback::Speakers;
use crate::capture::Capture;
use crate::transport::{
    AcceptError, ConnectionError, SpeechConnection, SpeechReader, SpeechTransport,
};

/// Each wait for a connection; the next wait follows at once.
const ACCEPT_WITHIN: Duration = Duration::from_secs(5);
/// Each read; a read that misses it is simply started again.
const READ_WITHIN: Duration = Duration::from_secs(1);
/// Effects that came while a write or a read waited, kept for the loop to serve after it. Past
/// this many the component stops taking effects until the wait is over, and the runtime's own
/// bounded queue holds the rest.
const PARKED_EFFECTS: usize = 64;

pub(super) type Owned<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// A finished read, with the half and buffer it used.
pub(super) struct ReadDone {
    pub(super) reader: SpeechReader,
    pub(super) buf: Box<[u8]>,
    pub(super) result: Result<usize, ConnectionError>,
}

/// The next read on `reader`, into `buf`, within its deadline.
pub(super) fn next_read(mut reader: SpeechReader, mut buf: Box<[u8]>) -> Owned<ReadDone> {
    Box::pin(async move {
        let result = reader.read(&mut buf, READ_WITHIN).await;
        ReadDone { reader, buf, result }
    })
}

/// A finished wait for an effect, with the source it came from.
pub(super) type EffectDone = (Box<dyn EffectSource>, Option<Envelope<Effect>>);

/// The next wait for an effect from `effects`.
fn next_effect(mut effects: Box<dyn EffectSource>) -> Owned<EffectDone> {
    Box::pin(async move {
        let effect = effects.recv().await;
        (effects, effect)
    })
}

/// The next wait for a connection, within its deadline.
pub(super) fn next_accept(
    transport: Arc<dyn SpeechTransport>,
) -> Owned<Result<SpeechConnection, AcceptError>> {
    Box::pin(async move { transport.accept(ACCEPT_WITHIN).await })
}

/// Waits for the future if there is one, and for ever if not. A future that finished must be
/// replaced or cleared before the next wait.
pub(super) async fn maybe<T>(future: Option<&mut Owned<T>>) -> T {
    match future {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

/// An interval that, when a tick is missed, ticks a whole period after the late one.
pub(super) fn ticker(every: Duration) -> Interval {
    let mut interval = tokio::time::interval(every);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

/// The effects routed to the component: the wait for the next one, and those that came while a
/// write or a read waited.
pub(super) struct Incoming {
    /// The wait for the next effect; `None` once the source has ended.
    pub(super) next: Option<Owned<EffectDone>>,
    /// Effects that came while a write or a read waited, oldest first; at most
    /// [`PARKED_EFFECTS`].
    pub(super) parked: VecDeque<Envelope<Effect>>,
}

impl Incoming {
    /// The effects from `effects`, the first wait begun.
    pub(super) fn new(effects: Box<dyn EffectSource>) -> Self {
        Self { next: Some(next_effect(effects)), parked: VecDeque::with_capacity(PARKED_EFFECTS) }
    }

    /// Takes what the wait for an effect gave and starts the next wait. A mute is served at
    /// once, through `capture`, and drops any `SetCapture` still parked, which it comes after; a
    /// playback command is served at once while playback runs; any other effect is parked for
    /// the loop. Playback commands are never parked while playback runs, so they keep their order
    /// among themselves.
    pub(super) fn arrived(
        &mut self,
        done: EffectDone,
        capture: &mut Capture,
        speakers: &mut Speakers,
    ) {
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
            return;
        }
        if let Effect::Audio(cmd) = &envelope.body
            && speakers.serve(cmd)
        {
            return;
        }
        self.parked.push_back(envelope);
    }
}

/// Awaits `work` while taking effects as they come: a mute stops capture and a playback command
/// reaches the player before `work` ends, and any other effect is parked for the loop. With
/// [`PARKED_EFFECTS`] parked it takes no more until `work` ends.
pub(super) async fn racing<T>(
    work: impl Future<Output = T>,
    incoming: &mut Incoming,
    capture: &mut Capture,
    speakers: &mut Speakers,
) -> T {
    let mut work = std::pin::pin!(work);
    loop {
        let room = incoming.parked.len() < PARKED_EFFECTS;
        tokio::select! {
            biased;
            out = &mut work => return out,
            done = maybe(incoming.next.as_mut()), if room => {
                incoming.arrived(done, capture, speakers);
            }
        }
    }
}

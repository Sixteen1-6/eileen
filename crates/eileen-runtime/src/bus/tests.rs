//! The bus on paused tokio time, fed through its effect queue as the driver feeds it: fan-out
//! in order, the lag notice and its count, and how it ends.

use std::future::Future;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{Effect, EffectFamily, Input};
use eileen_contracts::ids::{Envelope, JobId, MonoTime, SessionId, TraceCtx, WallTime};
use eileen_contracts::ports::{
    Backpressure, Component, ComponentCtx, ComponentError, EffectSource, InputSink,
};
use eileen_rt::channel::{self, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use super::{BusHandle, NAME, Subscriber, bounded};
use crate::driver::Shutdown;

/// Long enough for anything the bus owes a test; on paused time a wait that fails costs
/// nothing.
const PATIENCE: Duration = Duration::from_secs(60);

const fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

/// The bus sends no inputs.
struct NoInputs;

impl InputSink for NoInputs {
    fn send(&self, _input: Envelope<Input>) -> Result<(), Backpressure> {
        Err(Backpressure { capacity: 0 })
    }
}

/// The bus's effect queue, as the driver gives it.
struct Queue(Receiver<Envelope<Effect>>);

impl EffectSource for Queue {
    fn recv(&mut self) -> Pin<Box<dyn Future<Output = Option<Envelope<Effect>>> + Send + '_>> {
        Box::pin(self.0.recv())
    }
}

/// A running bus with the ends a driver would hold.
struct Rig {
    effects: Sender<Envelope<Effect>>,
    shutdown: Shutdown,
    task: JoinHandle<Result<(), ComponentError>>,
    handle: BusHandle,
}

/// Starts a bus that keeps `capacity` events.
fn rig(capacity: usize) -> Rig {
    let (bus, handle) = bounded(nz(capacity));
    let (effects, queue) = channel::bounded("test.bus", nz(64));
    let shutdown = Shutdown::new();
    let ctx = ComponentCtx {
        inputs: Box::new(NoInputs),
        effects: Box::new(Queue(queue)),
        data_dir: PathBuf::from("data"),
        session: SessionId::from_parts(1, 1),
        shutdown: Box::new(shutdown.clone()),
    };
    let task = eileen_rt::task::spawn("test.bus", Box::new(bus).start(ctx));
    Rig { effects, shutdown, task, handle }
}

/// The event the core emits as its `n`th effect: its job, times and body all follow from `n`.
fn event(n: u64) -> Envelope<BusEvent> {
    Envelope {
        seq: n,
        at: WallTime(1_000 + i64::try_from(n).unwrap()),
        mono: MonoTime(n * 10),
        ctx: TraceCtx { job: Some(JobId::from_parts(n, 1)), ..TraceCtx::default() },
        body: BusEvent::MuteChanged { muted: n % 2 == 1 },
    }
}

/// [`event`] as the effect the driver routes to the bus.
fn emit(n: u64) -> Envelope<Effect> {
    let Envelope { seq, at, mono, ctx, body } = event(n);
    Envelope { seq, at, mono, ctx, body: Effect::Emit(body) }
}

/// The notice that `missed` events were lost just before event `next`.
fn lagged(missed: u64, next: u64) -> Envelope<BusEvent> {
    let Envelope { seq, at, mono, .. } = event(next);
    Envelope { seq, at, mono, ctx: TraceCtx::default(), body: BusEvent::Lagged { missed } }
}

impl Rig {
    /// Routes events `first..=last` to the bus and lets it publish them.
    async fn publish(&self, first: u64, last: u64) {
        for n in first..=last {
            self.effects.send(emit(n)).await.unwrap();
        }
        sleep(Duration::from_millis(1)).await;
    }
}

/// The next event, which must already be there or come within [`PATIENCE`].
async fn next(subscriber: &mut Subscriber) -> Option<Envelope<BusEvent>> {
    timeout(PATIENCE, subscriber.recv()).await.expect("the subscriber was left waiting")
}

/// Whether nothing is waiting for `subscriber` and the bus is still open to it.
async fn is_idle(subscriber: &mut Subscriber) -> bool {
    timeout(Duration::from_millis(1), subscriber.recv()).await.is_err()
}

#[test]
fn the_bus_claims_emit_under_its_name() {
    let (bus, _handle) = bounded(nz(4));
    assert_eq!(bus.name(), NAME, "its name");
    assert_eq!(bus.accepts(), [EffectFamily::Emit], "its one family");
}

#[tokio::test(start_paused = true)]
async fn each_subscriber_gets_every_event_as_the_core_emitted_it() {
    let rig = rig(8);
    let mut first = rig.handle.subscribe();
    let mut second = rig.handle.subscribe();
    rig.publish(1, 5).await;
    for n in 1..=5 {
        assert_eq!(next(&mut first).await, Some(event(n)), "the first subscriber, event {n}");
    }
    for n in 1..=5 {
        assert_eq!(next(&mut second).await, Some(event(n)), "the second subscriber, event {n}");
    }
    assert!(is_idle(&mut first).await, "and nothing more");

    // A subscriber gets what is published after it subscribed.
    let mut late = rig.handle.subscribe();
    rig.publish(6, 6).await;
    assert_eq!(next(&mut late).await, Some(event(6)), "the late subscriber starts at event 6");
}

#[tokio::test(start_paused = true)]
async fn a_slow_subscriber_is_told_how_many_it_missed() {
    let rig = rig(4);
    let mut slow = rig.handle.subscribe();
    let mut fast = rig.handle.subscribe();
    for n in 1..=10 {
        rig.publish(n, n).await;
        assert_eq!(next(&mut fast).await, Some(event(n)), "the fast subscriber misses nothing");
    }

    assert_eq!(
        next(&mut slow).await,
        Some(lagged(6, 7)),
        "first the count of what it missed, stamped as the event its view resumes at"
    );
    for n in 7..=10 {
        assert_eq!(next(&mut slow).await, Some(event(n)), "then the four still kept, event {n}");
    }
    assert!(is_idle(&mut slow).await, "and it is caught up");

    // Once caught up it lags again only if it falls behind again.
    rig.publish(11, 12).await;
    assert_eq!(next(&mut slow).await, Some(event(11)), "no notice without a loss");
}

#[tokio::test(start_paused = true)]
async fn every_event_is_delivered_or_counted() {
    let rig = rig(4);
    let mut subscriber = rig.handle.subscribe();
    // Bursts published, each followed by this many reads: some within what the bus keeps, some
    // past it, one overrun while a lag notice's next event is still held.
    let bursts: [(u64, usize); 6] = [(3, 3), (9, 1), (7, 2), (2, 0), (12, 5), (1, 0)];
    let mut got = Vec::new();
    let mut published = 0;
    for (burst, reads) in bursts {
        rig.publish(published + 1, published + burst).await;
        published += burst;
        for _ in 0..reads {
            got.push(next(&mut subscriber).await.expect("the bus is running"));
        }
    }
    rig.shutdown.trigger();
    assert_eq!(rig.task.await.unwrap(), Ok(()), "the bus stops well");
    while let Some(event) = next(&mut subscriber).await {
        got.push(event);
    }

    let mut last = 0;
    let mut owed = 0;
    let mut notices = 0;
    for envelope in &got {
        if let BusEvent::Lagged { missed } = envelope.body {
            assert_eq!(owed, 0, "one notice per gap");
            assert_eq!(envelope.seq, last + missed + 1, "a notice is stamped as the next event");
            owed = missed;
            notices += 1;
        } else {
            assert_eq!(envelope.seq, last + owed + 1, "event {} follows in order", envelope.seq);
            assert_eq!(*envelope, event(envelope.seq), "and is unchanged");
            last = envelope.seq;
            owed = 0;
        }
    }
    assert_eq!((last, owed), (published, 0), "the last event published is the last one given");
    assert!(notices >= 3, "the subscriber lagged in several bursts, got {notices} notices");
}

#[tokio::test(start_paused = true)]
async fn a_recv_dropped_while_waiting_loses_no_count() {
    let rig = rig(4);
    let mut subscriber = rig.handle.subscribe();
    assert!(is_idle(&mut subscriber).await, "a recv that waited and was dropped");
    rig.publish(1, 7).await;
    assert_eq!(next(&mut subscriber).await, Some(lagged(3, 4)), "the notice");
    // The event after the notice is held in the subscriber, so an overrun of the bus between
    // the two cannot take it away.
    rig.publish(8, 20).await;
    assert_eq!(next(&mut subscriber).await, Some(event(4)), "the event the notice was stamped as");
    assert_eq!(next(&mut subscriber).await, Some(lagged(12, 17)), "then the next gap, counted");
    assert_eq!(next(&mut subscriber).await, Some(event(17)), "and the view resumes");
}

#[tokio::test(start_paused = true)]
async fn the_bus_publishes_what_is_queued_then_stops_at_shutdown() {
    let rig = rig(8);
    let mut subscriber = rig.handle.subscribe();
    // Nothing yields between these sends and the shutdown, so the bus finds both at once.
    for n in 1..=3 {
        rig.effects.try_send(emit(n)).unwrap();
    }
    rig.shutdown.trigger();
    assert_eq!(timeout(PATIENCE, rig.task).await.unwrap().unwrap(), Ok(()), "the bus stops well");

    for n in 1..=3 {
        assert_eq!(next(&mut subscriber).await, Some(event(n)), "event {n} was still published");
    }
    assert_eq!(next(&mut subscriber).await, None, "then the subscriber is told the bus stopped");
    assert_eq!(next(&mut subscriber).await, None, "each time it asks");
    assert_eq!(next(&mut rig.handle.subscribe()).await, None, "and so is one who subscribes now");
}

#[tokio::test(start_paused = true)]
async fn the_bus_stops_when_its_queue_closes() {
    let Rig { effects, task, handle, shutdown: _shutdown } = rig(8);
    let mut subscriber = handle.subscribe();
    effects.try_send(emit(1)).unwrap();
    drop(effects);
    assert_eq!(timeout(PATIENCE, task).await.unwrap().unwrap(), Ok(()), "the bus stops well");
    assert_eq!(next(&mut subscriber).await, Some(event(1)), "the queued event was published");
    assert_eq!(next(&mut subscriber).await, None, "and the subscriber is told the bus stopped");
}

#[tokio::test(start_paused = true)]
async fn a_subscriber_that_stopped_far_behind_still_gets_its_count() {
    let rig = rig(4);
    let mut subscriber = rig.handle.subscribe();
    rig.publish(1, 9).await;
    rig.shutdown.trigger();
    assert_eq!(timeout(PATIENCE, rig.task).await.unwrap().unwrap(), Ok(()), "the bus stops well");
    assert_eq!(next(&mut subscriber).await, Some(lagged(5, 6)), "the count survives the stop");
    for n in 6..=9 {
        assert_eq!(next(&mut subscriber).await, Some(event(n)), "event {n}");
    }
    assert_eq!(next(&mut subscriber).await, None, "then the end");
}

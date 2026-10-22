//! The signal-stream helpers, `subscribe` and `drain`, against a fake service on the private bus.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use eileen_dbus_linux::testing::{PrivateBus, nothing_ready, ready_now, round_trip, serve};
use eileen_dbus_linux::{DRAIN_CAP, DbusError, QUEUE_SIZE, Signals, drain, subscribe};
use zbus::MatchRule;
use zbus::match_rule::Builder;
use zbus::message::Type;

/// The fake service's name.
const NAME: &str = "org.eileen.Test.Ticker";
/// Its object.
const PATH: &str = "/org/eileen/Test";
/// The interface its signals come on.
const INTERFACE: &str = "org.eileen.Test.Ticker";
/// What the subscriptions' errors name.
const WHAT: &str = "test subscription";

/// A fake service with nothing to call: the tests send its signals from its connection.
struct Ticker;

#[zbus::interface(name = "org.eileen.Test.Ticker")]
impl Ticker {}

/// The rule for the fake's signals named `member`.
fn rule(member: &str) -> zbus::Result<MatchRule<'_>> {
    MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(INTERFACE)
        .and_then(|rule| rule.member(member))
        .map(Builder::build)
}

/// Sends the signal `member` with `number` from the fake.
async fn send(service: &zbus::Connection, member: &str, number: usize) {
    let number = u32::try_from(number).unwrap();
    service.emit_signal(None::<()>, PATH, INTERFACE, member, &(number,)).await.unwrap();
}

/// The number a signal carries, as `send` sent it.
fn number(signal: &zbus::Message) -> usize {
    usize::try_from(signal.body().deserialize::<u32>().unwrap()).unwrap()
}

/// The fake, a client and the client's subscription to the fake's `Tick`.
async fn ticks(bus: &PrivateBus) -> (zbus::Connection, zbus::Connection, Signals) {
    let service = serve(bus, NAME, PATH, Ticker).await;
    let client = bus.connect().await;
    let ticks = subscribe(&client, rule("Tick"), WHAT).await.unwrap();
    (service, client, ticks)
}

/// A waker that counts its wakes.
#[derive(Debug, Default)]
struct Wakes(AtomicUsize);

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl Wakes {
    /// Runs `poll` once with this waker.
    fn poll<T>(self: &Arc<Self>, poll: impl FnOnce(&mut Context<'_>) -> T) -> T {
        let waker = Waker::from(Arc::clone(self));
        poll(&mut Context::from_waker(&waker))
    }

    /// How many times the waker was woken.
    fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[tokio::test]
async fn a_subscription_brings_the_signals_its_rule_matches_and_nothing_else() {
    let bus = PrivateBus::start();
    let (service, client, mut ticks) = ticks(&bus).await;
    send(&service, "Tock", 1).await;
    send(&service, "Tick", 2).await;
    send(&service, "Tock", 3).await;
    round_trip(&client, Some(NAME)).await;
    let tick = ready_now(&mut ticks).expect("the signal the rule matches");
    assert_eq!(number(&tick), 2, "the member the rule names");
    assert!(nothing_ready(&mut ticks), "and no other");
}

#[tokio::test]
async fn a_burst_of_queue_size_signals_waits_while_the_stream_is_not_polled() {
    let bus = PrivateBus::start();
    let (service, client, mut ticks) = ticks(&bus).await;
    for sent in 0..QUEUE_SIZE {
        send(&service, "Tick", sent).await;
    }
    // A smaller queue would stop the connection reading before the round trip's reply.
    round_trip(&client, Some(NAME)).await;
    for sent in 0..QUEUE_SIZE {
        let tick = ready_now(&mut ticks).expect("every signal of the burst waited");
        assert_eq!(number(&tick), sent, "in the order sent");
    }
    assert!(nothing_ready(&mut ticks), "and nothing else");
}

#[tokio::test]
async fn a_rule_that_cannot_be_built_is_invalid_and_named() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    let rule = MatchRule::builder().msg_type(Type::Signal).path("no slash").map(Builder::build);
    let error = subscribe(&client, rule, WHAT).await.unwrap_err();
    assert!(
        matches!(error, DbusError::Invalid { what: WHAT, .. }),
        "the builder's error, named for what the subscription was for: {error:?}"
    );
}

#[tokio::test]
async fn once_the_bus_has_gone_a_stream_ends_and_a_subscription_fails() {
    let bus = PrivateBus::start();
    let (_service, client, mut ticks) = ticks(&bus).await;
    drop(bus);
    assert!(ticks.next().await.is_none(), "the read error is passed over, then the end");
    let error = subscribe(&client, rule("Tock"), WHAT).await.unwrap_err();
    assert!(
        matches!(error, DbusError::Io { what: WHAT, .. }),
        "the connection has stopped reading: {error:?}"
    );
}

#[tokio::test]
async fn drain_hands_over_what_is_ready_until_take_gives_a_value() {
    let bus = PrivateBus::start();
    let (service, client, mut ticks) = ticks(&bus).await;
    for sent in 1..=4 {
        send(&service, "Tick", sent).await;
    }
    round_trip(&client, Some(NAME)).await;
    let wakes = Arc::new(Wakes::default());
    let mut taken = Vec::new();
    let found = wakes.poll(|cx| {
        drain(&mut ticks, cx, |tick| {
            taken.push(number(&tick));
            (number(&tick) == 2).then_some("two")
        })
    });
    assert_eq!(found, Poll::Ready(Some("two")), "the value take gave");
    assert_eq!(taken, [1, 2], "the signals up to it, in order: the rest wait in the stream");
    let rest = wakes.poll(|cx| {
        drain(&mut ticks, cx, |tick| {
            taken.push(number(&tick));
            None::<()>
        })
    });
    assert_eq!(rest, Poll::Pending, "a take that gives no value takes everything ready");
    assert_eq!(taken, [1, 2, 3, 4], "the rest");
    assert_eq!(wakes.count(), 0, "nothing more is ready, so the stream, not drain, wakes the task");
}

#[tokio::test]
async fn drain_takes_at_most_drain_cap_and_wakes_the_task_to_come_back() {
    let bus = PrivateBus::start();
    let (service, client, mut ticks) = ticks(&bus).await;
    for sent in 0..DRAIN_CAP + 3 {
        send(&service, "Tick", sent).await;
    }
    round_trip(&client, Some(NAME)).await;
    let wakes = Arc::new(Wakes::default());
    let mut taken = 0;
    let first = wakes.poll(|cx| {
        drain(&mut ticks, cx, |_tick| {
            taken += 1;
            None::<()>
        })
    });
    assert_eq!((first, taken), (Poll::Pending, DRAIN_CAP), "one poll yields at the cap");
    assert_eq!(wakes.count(), 1, "and wakes the task to come back for the rest");
    let second = wakes.poll(|cx| {
        drain(&mut ticks, cx, |_tick| {
            taken += 1;
            None::<()>
        })
    });
    assert_eq!((second, taken), (Poll::Pending, DRAIN_CAP + 3), "the next poll takes the rest");
    assert_eq!(wakes.count(), 1, "and stops at the stream's own wait, with no wake of its own");
}

#[tokio::test]
async fn drain_ends_with_its_stream() {
    let bus = PrivateBus::start();
    let (service, client, mut ticks) = ticks(&bus).await;
    send(&service, "Tick", 1).await;
    round_trip(&client, Some(NAME)).await;
    drop(bus);
    let mut taken = Vec::new();
    let end = std::future::poll_fn(|cx| {
        drain(&mut ticks, cx, |tick| {
            taken.push(number(&tick));
            None::<()>
        })
    })
    .await;
    assert_eq!(end, None, "the end, once the connection has stopped reading");
    assert_eq!(taken, [1], "after the signal that came before it, and no read error");
}

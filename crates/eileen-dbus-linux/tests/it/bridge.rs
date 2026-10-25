//! The bridge from the synchronous traits to the async clients, against a fake service on the
//! private bus. The test's own thread calls the bridge as a trait's caller does, blocking; the
//! fake runs on a runtime of its own, whose workers serve it meanwhile. Where a test needs the
//! bridge's steps in an order, a job or a sink blocks its thread on a `std::sync::Barrier`, as
//! product jobs must not, and the test lets it go.

use std::cell::RefCell;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::task::{Context, Poll};
use std::thread::JoinHandle;
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_dbus_linux::{
    Bridge, Buses, DEFAULT_DEADLINE, DbusError, METHOD_TIMEOUT, Names, Signals, Stream,
    owner_changes, subscribe,
};
use eileen_platform::{Deadline, PlatformError, Subscription};
use eileen_stdx::clock::{self, Clock};
use tokio::sync::{mpsc, oneshot};
use zbus::MatchRule;
use zbus::match_rule::Builder;
use zbus::message::Type;

/// The fake service's name.
const NAME: &str = "org.eileen.Test.Bridge";
/// Its object.
const PATH: &str = "/org/eileen/Test";
/// Its interface.
const INTERFACE: &str = "org.eileen.Test.Bridge";
/// What the jobs' D-Bus errors name, apart from the bridge's own `call` names.
const FAKE_CALL: &str = "the fake's method";

/// A fake service: an echo, a method that never answers, and a meeting that answers only once
/// two calls wait in it. The tests send its `Tick` signals from its connection.
struct Fake {
    meeting: tokio::sync::Barrier,
}

#[zbus::interface(name = "org.eileen.Test.Bridge")]
impl Fake {
    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn echo(&self, word: String) -> String {
        word
    }

    async fn hang(&self) {
        std::future::pending::<()>().await;
    }

    async fn meet(&self) {
        let _leader = self.meeting.wait().await;
    }
}

/// The fake, on a multi-thread runtime of its own.
struct Fakes {
    runtime: tokio::runtime::Runtime,
    service: Option<zbus::Connection>,
}

impl Fakes {
    fn start(bus: &PrivateBus) -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        let runtime = runtime.unwrap();
        let fake = Fake { meeting: tokio::sync::Barrier::new(2) };
        let service = runtime.block_on(serve(bus, NAME, PATH, fake));
        Self { runtime, service: Some(service) }
    }

    /// Sends the fake's `Tick` with `number`.
    fn tick(&self, number: u32) {
        let service = self.service.as_ref().unwrap();
        let body = (number,);
        let sent = service.emit_signal(None::<()>, PATH, INTERFACE, "Tick", &body);
        self.runtime.block_on(sent).unwrap();
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        // The connection ends inside its runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.service = None;
    }
}

/// Calls the fake's `member` with no arguments on the session bus.
async fn call_fake(buses: Buses, member: &'static str) -> Result<(), PlatformError> {
    let bus = buses.session().await?;
    let reply = bus.call_method(Some(NAME), PATH, Some(INTERFACE), member, &()).await;
    reply.map_err(|error| DbusError::from_zbus(FAKE_CALL, &error))?;
    Ok(())
}

/// The fake's echo of `word`, through `bridge`.
fn echo(bridge: &Bridge, word: &'static str) -> Result<String, PlatformError> {
    bridge.call("echo", DEFAULT_DEADLINE, move |buses| async move {
        let bus = buses.session().await?;
        let reply = bus.call_method(Some(NAME), PATH, Some(INTERFACE), "Echo", &(word,)).await;
        let reply = reply.map_err(|error| DbusError::from_zbus(FAKE_CALL, &error))?;
        Ok(reply.body().deserialize::<String>().unwrap())
    })
}

/// The ID of the bus `bus` is on, as the bus's `GetId` answers.
async fn bus_id(bus: &zbus::Connection) -> Result<String, PlatformError> {
    const BUS: &str = "org.freedesktop.DBus";
    let reply = bus.call_method(Some(BUS), "/org/freedesktop/DBus", Some(BUS), "GetId", &()).await;
    let reply = reply.map_err(|error| DbusError::from_zbus("GetId", &error))?;
    Ok(reply.body().deserialize::<String>().unwrap())
}

/// A watch of the fake's ticks through `bridge`, each tick's number sent to the receiver.
fn watch_ticks(bridge: &Bridge) -> (Subscription, mpsc::Receiver<u32>) {
    let (sink, ticks) = mpsc::channel(8);
    let open = |buses: Buses| async move {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(INTERFACE)
            .and_then(|rule| rule.member("Tick"))
            .map(Builder::build);
        let bus = buses.session().await?;
        Ok(subscribe(&bus, rule, "the ticks").await?)
    };
    let each = move |signal: zbus::Message| {
        sink.try_send(signal.body().deserialize::<u32>().unwrap()).unwrap();
    };
    (bridge.watch("ticks", DEFAULT_DEADLINE, open, each).unwrap(), ticks)
}

thread_local! {
    /// Dropped when the thread that set it exits.
    static ON_EXIT: RefCell<Option<oneshot::Sender<()>>> = const { RefCell::new(None) };
    /// A connection one job keeps on the bridge's thread for a later one to drop.
    static KEPT: RefCell<Option<zbus::Connection>> = const { RefCell::new(None) };
}

/// A receiver that closes once the bridge's thread has exited.
fn thread_exit(bridge: &Bridge) -> oneshot::Receiver<()> {
    let (exiting, exited) = oneshot::channel();
    let marked = bridge.call("mark the thread", DEFAULT_DEADLINE, move |_buses| async move {
        ON_EXIT.with(|slot| *slot.borrow_mut() = Some(exiting));
        Ok(())
    });
    marked.unwrap();
    exited
}

/// A job that panics.
fn panics() -> Result<(), PlatformError> {
    panic!("the job fails")
}

/// A stream of what a channel brings, so a test makes a watch's items itself.
struct Items<T>(mpsc::Receiver<T>);

impl<T> Stream for Items<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.0.poll_recv(cx)
    }
}

/// A job holding the bridge's one thread, blocked, until released.
struct Hold {
    release: Arc<Barrier>,
    caller: JoinHandle<Result<(), PlatformError>>,
}

/// Holds `bridge`'s thread with a job that blocks it, returning once the job has begun.
fn hold(bridge: &Bridge) -> Hold {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let job = {
        let (entered, release) = (Arc::clone(&entered), Arc::clone(&release));
        move |_buses: Buses| async move {
            entered.wait();
            release.wait();
            Ok(())
        }
    };
    let bridge = bridge.clone();
    let caller = eileen_stdx::thread::spawn_named("eileen-test-holder", move || {
        bridge.call("hold", Deadline::after(Duration::from_secs(60)), job)
    });
    let caller = caller.unwrap();
    entered.wait();
    Hold { release, caller }
}

impl Hold {
    /// Lets the job end, then waits until every job posted to `bridge` before has begun.
    fn release(self, bridge: &Bridge) {
        self.release.wait();
        assert_eq!(self.caller.join().unwrap(), Ok(()), "the holding job answers");
        flush(bridge);
    }
}

/// Returns once every job posted to `bridge` before has taken its first step: jobs begin in
/// the order they were posted, and this one answers in its first.
fn flush(bridge: &Bridge) {
    let flushed = bridge.call("flush", DEFAULT_DEADLINE, |_buses| async move { Ok(()) });
    assert_eq!(flushed, Ok(()), "the bridge serves");
}

#[test]
fn a_call_answers_what_its_job_answers() {
    let bus = PrivateBus::start();
    let _fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    assert_eq!(echo(&bridge, "hello").unwrap(), "hello", "the fake's answer");
    let missing = bridge
        .call("missing", DEFAULT_DEADLINE, |buses| async move { call_fake(buses, "No").await });
    assert_eq!(
        missing,
        Err(PlatformError::NotFound { what: FAKE_CALL }),
        "a D-Bus error, through DbusError's From"
    );
}

#[test]
fn a_call_past_its_deadline_is_cancelled_and_the_next_one_answers() {
    let bus = PrivateBus::start();
    let _fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let (held, released) = oneshot::channel::<()>();
    let started = clock::system().mono();
    let late = bridge.call("the hanging call", Deadline::after(Duration::from_millis(100)), {
        move |buses| async move {
            let _held = held;
            call_fake(buses, "Hang").await
        }
    });
    let since = || Duration::from_nanos(clock::system().mono().0 - started.0);
    let answered = since();
    assert_eq!(
        late,
        Err(PlatformError::DeadlineExceeded { call: "the hanging call" }),
        "the bridge's deadline, not the connection's method timeout, which names the fake"
    );
    assert!(answered < METHOD_TIMEOUT, "answered after {answered:?}");
    assert!(released.blocking_recv().is_err(), "the job was dropped, its sender with it");
    let dropped = since();
    assert!(dropped < METHOD_TIMEOUT, "cancelled after {dropped:?}, not ended by the timeout");
    assert_eq!(echo(&bridge, "after").unwrap(), "after", "the bridge still serves");
}

#[test]
fn two_calls_in_flight_at_once_both_answer() {
    let bus = PrivateBus::start();
    let _fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let callers =
        ["eileen-test-caller-a", "eileen-test-caller-b"].map(|name| {
            let bridge = bridge.clone();
            eileen_stdx::thread::spawn_named(name, move || {
                bridge.call("meet", DEFAULT_DEADLINE, |buses| async move {
                    call_fake(buses, "Meet").await
                })
            })
            .unwrap()
        });
    for caller in callers {
        assert_eq!(caller.join().unwrap(), Ok(()), "the meeting answers only both calls at once");
    }
}

#[test]
fn a_watch_delivers_until_its_subscription_drops() {
    let bus = PrivateBus::start();
    let fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let (subscription, mut ticks) = watch_ticks(&bridge);
    for number in 1..=3 {
        fakes.tick(number);
    }
    for number in 1..=3 {
        assert_eq!(ticks.blocking_recv(), Some(number), "tick {number}, in order");
    }
    drop(subscription);
    assert_eq!(ticks.blocking_recv(), None, "the task ended and dropped its sink");
    assert_eq!(echo(&bridge, "after").unwrap(), "after", "the bridge still serves");
}

#[test]
fn a_watch_whose_stream_does_not_open_answers_why() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let refused = PlatformError::Refused { what: "the stream", reason: "not allowed" };
    let open = {
        let refused = refused.clone();
        move |_buses: Buses| async move { Err::<Signals, _>(refused) }
    };
    let failed = bridge.watch("refused", DEFAULT_DEADLINE, open, |_: zbus::Message| {});
    assert_eq!(failed.unwrap_err(), refused, "the open step's error");
}

#[test]
fn the_buses_are_kept_and_connected_again_once_closed() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let names = bridge.call("names", DEFAULT_DEADLINE, |buses| async move {
        let name = |bus: &zbus::Connection| bus.unique_name().unwrap().to_string();
        let session = buses.session().await?;
        let again = name(&buses.session().await?);
        let system = name(&buses.system().await?);
        let first = name(&session);
        session.close().await.unwrap();
        let reconnected = name(&buses.session().await?);
        Ok([first, again, system, reconnected])
    });
    let [first, again, system, reconnected] = names.unwrap();
    assert_eq!(first, again, "one session connection, kept");
    assert_ne!(first, system, "the system bus has its own");
    assert_ne!(first, reconnected, "a new one after it closed");
}

#[test]
fn a_bridge_on_two_addresses_gives_each_job_its_own_bus() {
    let (first, second) = (PrivateBus::start(), PrivateBus::start());
    let bridge = Bridge::on_addresses(first.address(), second.address()).unwrap();
    let ids = bridge.call("bus ids", DEFAULT_DEADLINE, |buses| async move {
        let session = bus_id(&buses.session().await?).await?;
        let unshared = bus_id(&buses.unshared_session().await?).await?;
        let system = bus_id(&buses.system().await?).await?;
        Ok([session, unshared, system])
    });
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let own = |bus: &PrivateBus| runtime.block_on(async { bus_id(&bus.connect().await).await });
    let (first_id, second_id) = (own(&first).unwrap(), own(&second).unwrap());
    assert_ne!(first_id, second_id, "two buses, each with an ID of its own");
    assert_eq!(
        ids.unwrap(),
        [first_id.clone(), first_id, second_id],
        "the session bus, unshared connections included, at the first address; the system bus \
         at the second"
    );
}

#[test]
fn an_unshared_session_is_a_connection_of_its_own_that_closes_once_dropped() {
    let bus = PrivateBus::start();
    let fakes = Fakes::start(&bus);
    // The watcher's stream drops inside its runtime's context, a panicking test's included.
    let _context = fakes.runtime.enter();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let names = bridge.call("open", DEFAULT_DEADLINE, |buses| async move {
        let name = |bus: &zbus::Connection| bus.unique_name().unwrap().to_string();
        let unshared = buses.unshared_session().await?;
        let names = [name(&buses.session().await?), name(&unshared), name(&buses.session().await?)];
        KEPT.with(|kept| *kept.borrow_mut() = Some(unshared));
        Ok(names)
    });
    let [shared, unshared, again] = names.unwrap();
    assert_ne!(shared, unshared, "a connection of its own");
    assert_eq!(shared, again, "the shared one, kept as it was");
    let fake = fakes.service.as_ref().unwrap();
    let subscribed = fakes.runtime.block_on(owner_changes(fake, Names::Name(&unshared)));
    let mut changes = subscribed.unwrap();
    let dropped = bridge.call("drop", DEFAULT_DEADLINE, |_buses| async move {
        drop(KEPT.with(|kept| kept.borrow_mut().take()));
        Ok(())
    });
    assert_eq!(dropped, Ok(()), "the job dropped it");
    let left = fakes.runtime.block_on(tokio::time::timeout(METHOD_TIMEOUT, changes.next()));
    let change = left.expect("the bus saw it leave").unwrap();
    let owners = (change.old.map(|old| old.to_string()), change.new);
    assert_eq!(owners, (Some(unshared), None), "the unshared connection closed");
    assert_eq!(echo(&bridge, "after").unwrap(), "after", "the shared one still serves");
}

#[test]
fn a_job_that_panics_answers_at_once_and_the_bridge_goes_on() {
    let bus = PrivateBus::start();
    let _fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let failed = bridge.call("the failing call", Deadline::after(Duration::from_secs(20)), {
        |_buses| async move { panics() }
    });
    assert!(
        matches!(failed, Err(PlatformError::Io { what: "the failing call", .. })),
        "dropped unanswered, before the deadline: {failed:?}"
    );
    assert_eq!(echo(&bridge, "after").unwrap(), "after", "the bridge still serves");
}

#[test]
fn the_thread_ends_when_the_last_handle_drops() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let clone = bridge.clone();
    let mut exited = thread_exit(&bridge);
    drop(bridge);
    assert_eq!(exited.try_recv(), Err(oneshot::error::TryRecvError::Empty), "a clone holds it");
    let answered = clone.call("still there", DEFAULT_DEADLINE, |_buses| async move { Ok(1) });
    assert_eq!(answered, Ok(1), "the clone's call");
    drop(clone);
    assert_eq!(exited.try_recv(), Err(oneshot::error::TryRecvError::Closed), "exited");
}

#[test]
fn a_subscription_holds_the_thread_until_it_drops() {
    let bus = PrivateBus::start();
    let fakes = Fakes::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let mut exited = thread_exit(&bridge);
    let (subscription, mut ticks) = watch_ticks(&bridge);
    drop(bridge);
    fakes.tick(1);
    assert_eq!(ticks.blocking_recv(), Some(1), "the watch runs on");
    assert_eq!(exited.try_recv(), Err(oneshot::error::TryRecvError::Empty), "held");
    drop(subscription);
    assert_eq!(exited.try_recv(), Err(oneshot::error::TryRecvError::Closed), "exited");
}

#[test]
fn a_job_whose_caller_has_stopped_waiting_never_begins() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let begun = Arc::new(AtomicBool::new(false));
    // A job that met its dropped stop on its first step once ran its body half the time:
    // twenty rounds miss that one time in a million.
    for round in 0..20 {
        let holding = hold(&bridge);
        let job = {
            let begun = Arc::clone(&begun);
            move |_buses: Buses| async move {
                begun.store(true, Ordering::SeqCst);
                Ok(())
            }
        };
        let late = bridge.call("the late call", Deadline::after(Duration::from_millis(1)), job);
        let expected = Err(PlatformError::DeadlineExceeded { call: "the late call" });
        assert_eq!(late, expected, "round {round}: queued behind the hold");
        holding.release(&bridge);
        assert!(!begun.load(Ordering::SeqCst), "round {round}: the job ran after its answer");
    }
}

#[test]
fn a_sink_takes_nothing_once_its_subscription_drop_has_returned() {
    const BURST: usize = 8;
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let (items, queue) = mpsc::channel(BURST);
    for item in 0..BURST {
        items.try_send(item).unwrap();
    }
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let taken = Arc::new(AtomicUsize::new(0));
    let each = {
        let (inside, release) = (Arc::clone(&inside), Arc::clone(&release));
        let taken = Arc::clone(&taken);
        move |_item: usize| {
            if taken.fetch_add(1, Ordering::SeqCst) == 0 {
                inside.wait();
                release.wait();
            }
        }
    };
    let open = move |_buses: Buses| async move { Ok(Items(queue)) };
    let subscription = bridge.watch("the burst", DEFAULT_DEADLINE, open, each).unwrap();
    inside.wait();
    drop(subscription);
    release.wait();
    flush(&bridge);
    assert_eq!(taken.load(Ordering::SeqCst), 1, "only the item under way when the drop came");
    assert!(items.is_closed(), "the stream was dropped then and there");
}

#[test]
fn the_last_handle_dropped_on_the_bridge_thread_ends_it_without_a_join() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let exited = thread_exit(&bridge);
    let (handles, queue) = mpsc::channel(1);
    let (returned, mut returns) = mpsc::channel(1);
    let each = move |subscription: Subscription| {
        drop(subscription);
        // Reached only if the drop returned: the thread cannot join itself.
        returned.try_send(()).unwrap();
    };
    let open = move |_buses: Buses| async move { Ok(Items(queue)) };
    let subscription = bridge.watch("the last handle", DEFAULT_DEADLINE, open, each).unwrap();
    drop(bridge);
    handles.try_send(subscription).unwrap();
    assert_eq!(returns.blocking_recv(), Some(()), "the sink dropped the last handle and went on");
    assert!(exited.blocking_recv().is_err(), "the thread exited on its own");
}

#[test]
fn a_watch_whose_stream_opens_past_its_deadline_answers_so_and_drops_it() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let (items, queue) = mpsc::channel(1);
    items.try_send(1).unwrap();
    let (go, gone) = oneshot::channel::<()>();
    let open = move |_buses: Buses| async move {
        let _released = gone.await;
        Ok(Items(queue))
    };
    let (sink, mut taken) = mpsc::channel(1);
    let each = move |item: u32| sink.try_send(item).unwrap();
    let budget = Deadline::after(Duration::from_millis(1));
    let late = bridge.watch("the late watch", budget, open, each);
    let expected = PlatformError::DeadlineExceeded { call: "the late watch" };
    assert_eq!(late.unwrap_err(), expected, "open had not answered");
    let _ignored = go.send(());
    assert_eq!(taken.blocking_recv(), None, "the job ended, its sink with it, given no item");
    assert!(items.is_closed(), "the stream did not outlive the job");
}

#[test]
fn a_call_or_watch_made_on_the_bridge_thread_is_refused_at_once() {
    let bus = PrivateBus::start();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let inner = bridge.clone();
    let outer = move |_buses: Buses| async move {
        let call = inner.call("the nested call", DEFAULT_DEADLINE, |_buses| async move { Ok(()) });
        let open = |_buses: Buses| async move { Ok(Items(mpsc::channel::<u32>(1).1)) };
        let watch = inner.watch("the nested watch", DEFAULT_DEADLINE, open, |_item: u32| {});
        Ok((call.err(), watch.err()))
    };
    let nested = bridge.call("the outer call", Deadline::after(Duration::from_secs(20)), outer);
    let (call, watch) = nested.unwrap();
    assert!(
        matches!(call, Some(PlatformError::Refused { what: "the nested call", .. })),
        "not waited out: {call:?}"
    );
    assert!(
        matches!(watch, Some(PlatformError::Refused { what: "the nested watch", .. })),
        "not waited out: {watch:?}"
    );
}

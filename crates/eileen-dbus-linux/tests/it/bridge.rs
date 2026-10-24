//! The bridge from the synchronous traits to the async clients, against a fake service on the
//! private bus. The test's own thread calls the bridge as a trait's caller does, blocking; the
//! fake runs on a runtime of its own, whose workers serve it meanwhile.

use std::cell::RefCell;
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_dbus_linux::{
    Bridge, Buses, DEFAULT_DEADLINE, DbusError, METHOD_TIMEOUT, Signals, subscribe,
};
use eileen_platform::{Deadline, PlatformError};
use eileen_stdx::clock::{self, Clock};
use tokio::sync::{Barrier, mpsc, oneshot};
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
    meeting: Barrier,
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
        let fake = Fake { meeting: Barrier::new(2) };
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

/// A watch of the fake's ticks through `bridge`, each tick's number sent to the receiver.
fn watch_ticks(bridge: &Bridge) -> (eileen_platform::Subscription, mpsc::Receiver<u32>) {
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

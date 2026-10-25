//! The private bus, the method timeout (a private bus's, and a bus's at an address) and the
//! round-trip barrier.

use std::time::Duration;

use eileen_dbus_linux::testing::{
    PrivateBus, next_message, nothing_ready, ready_now, round_trip, serve,
};
use eileen_dbus_linux::{DbusError, METHOD_TIMEOUT, bus_at_within, next};

/// The fake service's name.
const NAME: &str = "org.eileen.Test";
/// Its object.
const PATH: &str = "/org/eileen/Test";
/// Its interface.
const INTERFACE: &str = "org.eileen.Test.Echo";

/// A fake service: it answers with its prefix and the text, and never answers `Hang`.
struct Echo {
    prefix: &'static str,
}

#[zbus::interface(name = "org.eileen.Test.Echo")]
impl Echo {
    fn echo(&self, text: &str) -> String {
        format!("{}{text}", self.prefix)
    }

    async fn hang(&self) {
        std::future::pending::<()>().await;
    }
}

/// The signals of the fake's interface, as `conn` receives them.
async fn signals(conn: &zbus::Connection) -> zbus::MessageStream {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(INTERFACE)
        .unwrap()
        .build();
    zbus::MessageStream::for_match_rule(rule, conn, None).await.unwrap()
}

#[tokio::test]
async fn a_private_bus_carries_a_call_to_a_fake_service() {
    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, Echo { prefix: "echo: " }).await;
    let client = bus.connect().await;
    let reply =
        client.call_method(Some(NAME), PATH, Some(INTERFACE), "Echo", &("hello",)).await.unwrap();
    let text: String = reply.body().deserialize().unwrap();
    assert_eq!(text, "echo: hello", "the fake service answered over the private bus");
}

#[tokio::test]
async fn a_call_past_the_method_timeout_is_a_passed_deadline() {
    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, Echo { prefix: "" }).await;
    let client = bus.connect_within(Duration::from_secs(5)).await;
    // Paused only now: the connections above are made in real time, and from here the only
    // timer is the method timeout, which tokio fires as soon as the runtime is idle.
    tokio::time::pause();
    let error =
        client.call_method(Some(NAME), PATH, Some(INTERFACE), "Hang", &()).await.unwrap_err();
    assert_eq!(
        DbusError::from_zbus("the hanging call", &error),
        DbusError::DeadlineExceeded { call: "the hanging call" },
        "the connection's method timeout is a passed deadline, not an I/O error"
    );
}

#[tokio::test]
async fn a_bus_at_an_address_bounds_each_call_by_its_timeout() {
    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, Echo { prefix: "echo: " }).await;
    let nowhere = format!("unix:path={}", bus.dir().join("no-bus-here").display());
    assert_eq!(
        bus_at_within(&nowhere, "the test bus", Duration::from_secs(5)).await.unwrap_err(),
        DbusError::Unavailable { what: "the test bus" },
        "no bus listens there, and the error names what the caller said"
    );
    // Not METHOD_TIMEOUT, so the test tells the caller's timeout from the default.
    let timeout = Duration::from_secs(2);
    let client = bus_at_within(bus.address(), "the test bus", timeout).await.unwrap();
    let reply =
        client.call_method(Some(NAME), PATH, Some(INTERFACE), "Echo", &("hello",)).await.unwrap();
    let text: String = reply.body().deserialize().unwrap();
    assert_eq!(text, "echo: hello", "the connection reached the bus at the address");
    // Paused only now, as in the test above: from here the only timer is the method timeout.
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let error =
        client.call_method(Some(NAME), PATH, Some(INTERFACE), "Hang", &()).await.unwrap_err();
    assert_eq!(
        DbusError::from_zbus("the hanging call", &error),
        DbusError::DeadlineExceeded { call: "the hanging call" },
        "the method timeout bounds the call"
    );
    // tokio rounds a timer up to its next millisecond, so the paused clock reads 2.001 s.
    let waited = started.elapsed();
    assert!(
        waited >= timeout && waited < METHOD_TIMEOUT,
        "the timeout the caller gave, not the default: {waited:?}"
    );
}

#[tokio::test]
async fn dropping_the_bus_stops_its_daemon_and_removes_its_folder() {
    let bus = PrivateBus::start();
    let proc = std::path::PathBuf::from(format!("/proc/{}", bus.daemon_id()));
    let dir = bus.dir().to_owned();
    assert!(proc.exists() && dir.exists(), "the daemon runs in its folder");
    drop(bus);
    assert!(!proc.exists(), "the daemon is killed and reaped");
    assert!(!dir.exists(), "its folder is removed");
}

#[tokio::test]
async fn after_a_round_trip_every_earlier_signal_is_ready_and_nothing_else() {
    let bus = PrivateBus::start();
    let service = serve(&bus, NAME, PATH, Echo { prefix: "" }).await;
    let client = bus.connect().await;
    let mut signals = signals(&client).await;
    for number in [1_u32, 2] {
        service.emit_signal(None::<()>, PATH, INTERFACE, "Tick", &(number,)).await.unwrap();
    }
    round_trip(&client, Some(NAME)).await;
    for number in [1_u32, 2] {
        let signal = ready_now(&mut signals).expect("ready").expect("a message");
        assert_eq!(signal.body().deserialize::<u32>().unwrap(), number, "in the order sent");
    }
    round_trip(&client, Some(NAME)).await;
    assert!(nothing_ready(&mut signals), "nothing more was sent");
}

#[tokio::test]
async fn next_ends_once_the_bus_has_gone() {
    let bus = PrivateBus::start();
    let service = serve(&bus, NAME, PATH, Echo { prefix: "" }).await;
    let client = bus.connect().await;
    let mut signals = signals(&client).await;
    service.emit_signal(None::<()>, PATH, INTERFACE, "Tick", &(7_u32,)).await.unwrap();
    let signal = next_message(&mut signals).await;
    assert_eq!(signal.body().deserialize::<u32>().unwrap(), 7, "the signal");
    drop(bus);
    assert!(next(&mut signals).await.is_none(), "the read error is passed over, then the end");
}

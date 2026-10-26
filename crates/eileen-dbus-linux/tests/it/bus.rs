//! The private bus, the silent bus, the method timeout and the round-trip barrier.

use std::time::Duration;

use eileen_dbus_linux::testing::{
    PrivateBus, SilentBus, next_message, nothing_ready, ready_now, round_trip, serve,
};
use eileen_dbus_linux::{DbusError, next};

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
async fn dropping_the_bus_stops_its_daemon_and_removes_its_folder() {
    let bus = PrivateBus::start();
    let proc = std::path::PathBuf::from(format!("/proc/{}", bus.daemon_id()));
    let dir = bus.dir().to_owned();
    assert!(proc.exists() && dir.exists(), "the daemon runs in its folder");
    drop(bus);
    assert!(!proc.exists(), "the daemon is killed and reaped");
    assert!(!dir.exists(), "its folder is removed");
}

#[test]
fn a_silent_bus_removes_its_folder_when_it_drops() {
    let silent = SilentBus::start();
    let dir = silent.dir().to_owned();
    assert!(dir.join("silent").exists(), "its socket is in its folder");
    drop(silent);
    assert!(!dir.exists(), "its folder is removed");
}

#[tokio::test(start_paused = true)]
async fn a_connect_to_a_silent_bus_never_finishes() {
    let silent = SilentBus::start();
    let builder = zbus::connection::Builder::address(silent.address()).unwrap();
    // Paused time stands still while zbus connects the socket on a blocking thread. The
    // handshake then waits for an answer that never comes, and with nothing else to run, tokio
    // fires the timer at once.
    let connect = tokio::time::timeout(Duration::from_secs(60), builder.build()).await;
    assert!(connect.is_err(), "the handshake is still waiting a minute on: {connect:?}");
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

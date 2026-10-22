//! The private bus and the method timeout.

use std::time::Duration;

use eileen_dbus_linux::DbusError;
use eileen_dbus_linux::testing::{PrivateBus, serve};

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

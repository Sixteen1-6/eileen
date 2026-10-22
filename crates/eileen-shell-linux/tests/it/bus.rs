//! A private D-Bus bus for one test: a `dbus-daemon` of its own, with a minimal configuration and
//! no service activation, so a fake service never meets a real one and the tests need no desktop
//! session. The daemon stops when the bus drops.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// The configuration: a session-type bus that allows everything between the test's own
/// connections, listening in a fresh folder of the temporary directory (a Unix socket path must
/// stay under about 108 bytes, which Cargo's target folder can exceed).
const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=SOCKET_DIR</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A running private bus.
pub(crate) struct PrivateBus {
    address: String,
    daemon: Child,
    dir: PathBuf,
}

impl PrivateBus {
    /// Starts a bus. Panics with a clear message when `dbus-daemon` (the `dbus` package) is
    /// missing, so a CI image without it fails loudly instead of skipping the D-Bus tests.
    pub(crate) fn start() -> Self {
        let dir = tempfile_dir();
        let config = dir.join("bus.conf");
        std::fs::write(&config, CONFIG.replace("SOCKET_DIR", dir.to_str().unwrap())).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "a test starts its own bus daemon; the supervisor rule is for product code"
        )]
        let spawned = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .arg("--print-address=1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut daemon = spawned.expect("the D-Bus tests need dbus-daemon, from the dbus package");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
        assert!(!address.trim().is_empty(), "dbus-daemon printed no address");
        Self { address: address.trim().to_owned(), daemon, dir }
    }

    /// The bus's address, for `zbus::connection::Builder::address`.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// A new connection to the bus.
    pub(crate) async fn connect(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address()).unwrap().build().await.unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        // Best effort: the daemon may already be gone, and a leftover folder is harmless.
        let _ignored = self.daemon.kill();
        let _ignored = self.daemon.wait();
        let _ignored = std::fs::remove_dir_all(&self.dir);
    }
}

/// A fresh, short folder under the system's temporary directory, unique to this process and
/// call.
fn tempfile_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("eileen-bus-{}-{n}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake service for the harness's own test: it answers with its prefix and the text.
struct Echo {
    prefix: &'static str,
}

#[zbus::interface(name = "org.eileen.Test.Echo")]
impl Echo {
    fn echo(&self, text: &str) -> String {
        format!("{}{text}", self.prefix)
    }
}

#[tokio::test]
async fn a_private_bus_carries_a_call_to_a_fake_service() {
    let bus = PrivateBus::start();
    let _service = zbus::connection::Builder::address(bus.address())
        .unwrap()
        .name("org.eileen.Test")
        .unwrap()
        .serve_at("/org/eileen/Test", Echo { prefix: "echo: " })
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = bus.connect().await;
    let reply = client
        .call_method(
            Some("org.eileen.Test"),
            "/org/eileen/Test",
            Some("org.eileen.Test.Echo"),
            "Echo",
            &("hello",),
        )
        .await
        .unwrap();
    let text: String = reply.body().deserialize().unwrap();
    assert_eq!(text, "echo: hello", "the fake service answered over the private bus");
}

//! Test support, behind the `private-bus` feature: a private D-Bus bus for one test, a fake
//! service on it, and the helpers that keep D-Bus tests deterministic.
//!
//! - [`PrivateBus`] runs a `dbus-daemon` of its own, with a minimal configuration and no service
//!   activation, so a fake service never meets a real one and the tests need no desktop session.
//!   The daemon is killed and its folder removed when the bus drops, a panicking test included.
//! - [`SilentBus`] listens and never answers, as a bus that has hung: a client's connect
//!   completes, and its handshake then waits for good. Its folder is removed when it drops.
//! - [`serve`] puts a fake service on the bus under a well-known name.
//! - [`round_trip`], [`nothing_ready`] and [`ready_now`] assert that nothing more arrived without a wall-clock
//!   window. The bus delivers each sender's messages in order, so after a round trip to the
//!   sender of the last expected signal (the fake, with `org.freedesktop.DBus.Peer.Ping`, or the
//!   bus itself for its own signals, with `GetId`), every signal sent before it is in the stream.
//! - [`next_message`] has no timer, so it works under tokio's paused time; nextest's slow-timeout
//!   ends a test that waits for good.
//!
//! These helpers panic with a clear message instead of returning errors: they are for tests.
#![expect(clippy::expect_used, reason = "test support fails the test with a clear message")]

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::task::{Context, Waker};
use std::time::Duration;

use zbus::address::transport::{Transport, Unix, UnixSocket};

use crate::stream::Stream;

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

/// What errors of the private bus's connections name.
const PRIVATE_BUS: &str = "the private bus";

/// A running private bus. Fields drop in order: the daemon is killed before its folder goes.
#[derive(Debug)]
pub struct PrivateBus {
    address: String,
    daemon: Daemon,
    dir: TempDir,
}

impl PrivateBus {
    /// Starts a bus. Panics with a clear message when `dbus-daemon` (the `dbus` package) is
    /// missing, so a CI image without it fails loudly instead of skipping the D-Bus tests.
    #[must_use]
    pub fn start() -> Self {
        let dir = TempDir::new();
        let config = dir.path().join("bus.conf");
        let socket_dir = dir.path().to_str().expect("the temporary folder's path is UTF-8");
        std::fs::write(&config, CONFIG.replace("SOCKET_DIR", socket_dir))
            .expect("the bus configuration is written");
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
        // Guarded before anything else can panic, so the daemon never outlives the test.
        let mut daemon =
            Daemon(spawned.expect("the D-Bus tests need dbus-daemon, from the dbus package"));
        let stdout = daemon.0.stdout.take().expect("the daemon's output is piped");
        let mut address = String::new();
        BufReader::new(stdout).read_line(&mut address).expect("the daemon printed its address");
        let address = address.trim().to_owned();
        assert!(!address.is_empty(), "dbus-daemon printed no address");
        Self { address, daemon, dir }
    }

    /// The bus's address, for `zbus::connection::Builder::address`.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// A new connection to the bus, with no method timeout: tokio's paused time would fire one
    /// while the daemon, another process, answers. Use [`Self::connect_within`] to test one.
    pub async fn connect(&self) -> zbus::Connection {
        self.builder().build().await.expect("a connection to the private bus")
    }

    /// A new connection to the bus whose method calls are bounded by `timeout`, built as
    /// [`crate::session_bus_within`] builds its own.
    pub async fn connect_within(&self, timeout: Duration) -> zbus::Connection {
        crate::bus::build_within(self.builder(), PRIVATE_BUS, timeout)
            .await
            .expect("a connection to the private bus")
    }

    /// A connection builder for the bus.
    fn builder(&self) -> zbus::connection::Builder<'static> {
        zbus::connection::Builder::address(self.address.as_str())
            .expect("the daemon printed a valid address")
    }

    /// The daemon's process ID, for a test of the guard.
    #[must_use]
    pub fn daemon_id(&self) -> u32 {
        self.daemon.0.id()
    }

    /// The bus's folder, for a test of the guard.
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }
}

/// A bus that never answers: a Unix socket that listens and never accepts. The kernel completes
/// a client's connect, and the client's SASL `AUTH` then waits for good, as on a bus that
/// accepts and has hung. Fields drop in order: the socket closes before its folder goes.
#[derive(Debug)]
pub struct SilentBus {
    address: String,
    /// Held for as long as the bus lives: closed, it would refuse a connect at once.
    _listener: UnixListener,
    dir: TempDir,
}

impl SilentBus {
    /// Starts listening, in a fresh folder of the temporary directory.
    #[must_use]
    pub fn start() -> Self {
        let dir = TempDir::new();
        let path = dir.path().join("silent");
        let listener = UnixListener::bind(&path).expect("the silent bus's socket");
        // The address escapes what the folder's path holds, as a D-Bus address must.
        let address = zbus::Address::from(Transport::Unix(Unix::new(UnixSocket::File(path))));
        Self { address: address.to_string(), _listener: listener, dir }
    }

    /// The bus's address, for `zbus::connection::Builder::address` or
    /// [`Bridge::on_addresses`](crate::Bridge::on_addresses).
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The bus's folder, for a test of the guard.
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }
}

/// The daemon, killed and reaped when it drops.
#[derive(Debug)]
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        // Best effort: the daemon may already be gone.
        let _ignored = self.0.kill();
        let _ignored = self.0.wait();
    }
}

/// A fresh, short folder under the system's temporary directory, unique to this process and
/// call, removed when it drops.
#[derive(Debug)]
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-bus-{}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temporary folder for the bus");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a leftover folder is harmless.
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// A fake service on `bus`: a connection that owns `name` and serves `iface` at `path`. Hold it
/// for as long as the service should run; dropping it takes the service off the bus.
pub async fn serve<I: zbus::object_server::Interface>(
    bus: &PrivateBus,
    name: &str,
    path: &str,
    iface: I,
) -> zbus::Connection {
    bus.builder()
        .name(name)
        .expect("a valid bus name")
        .serve_at(path, iface)
        .expect("a valid object path")
        .build()
        .await
        .expect("the fake service is on the bus")
}

/// The next message `stream` brings. Panics when the stream ends or brings a read error.
pub async fn next_message(stream: &mut zbus::MessageStream) -> zbus::Message {
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx))
        .await
        .expect("the stream stays open")
        .expect("a message, not a read error")
}

/// A round trip from `conn` to `peer`, a fake service's name, with
/// `org.freedesktop.DBus.Peer.Ping`, which zbus answers for every connection; with `None`, to
/// the bus itself, with `GetId`. When it returns, every signal `peer` (or the bus) sent before
/// it has reached `conn`'s streams.
pub async fn round_trip(conn: &zbus::Connection, peer: Option<&str>) {
    let (destination, interface, member) = match peer {
        Some(peer) => (peer, "org.freedesktop.DBus.Peer", "Ping"),
        None => ("org.freedesktop.DBus", "org.freedesktop.DBus", "GetId"),
    };
    let _reply = conn
        .call_method(Some(destination), "/org/freedesktop/DBus", Some(interface), member, &())
        .await
        .expect("the round trip");
}

/// Whether `stream` has nothing ready now, polled once without waiting: neither an item nor
/// its end. After a [`round_trip`], `true` means nothing more was sent. An item that is ready is
/// taken, so assert on [`ready_now`] to look at it.
pub fn nothing_ready<S: Stream + Unpin>(stream: &mut S) -> bool {
    let mut cx = Context::from_waker(Waker::noop());
    Pin::new(stream).poll_next(&mut cx).is_pending()
}

/// The item `stream` has ready now, polled once without waiting; `None` when it has none or has
/// ended. After a [`round_trip`], every item sent before it is ready.
pub fn ready_now<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    let mut cx = Context::from_waker(Waker::noop());
    match Pin::new(stream).poll_next(&mut cx) {
        std::task::Poll::Ready(item) => item,
        std::task::Poll::Pending => None,
    }
}

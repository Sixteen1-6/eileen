//! Fake AT-SPI2 services: on a session bus, the launcher's `org.a11y.Bus` and `org.a11y.Status`
//! ([`Session`]); on an accessibility bus, a registry ([`registry`]) and apps ([`App`]), each of
//! whose objects is a [`Node`] that serves `org.a11y.atspi.Accessible`, and
//! `org.a11y.atspi.Text` when it has text. The numbers here are AT-SPI2's own, written out, so
//! the tests do not read the crate's tables to check them.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_desktop_linux::ObjectRef;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// The launcher's object on the session bus.
const LAUNCHER: &str = "/org/a11y/bus";
/// The registry's bus name on the accessibility bus.
const REGISTRY: &str = "org.a11y.atspi.Registry";
/// The registry's root object, and every app's.
pub(crate) const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The path of the null reference.
pub(crate) const NULL: &str = "/org/a11y/atspi/null";

/// `ATSPI_STATE_DEFUNCT`.
pub(crate) const DEFUNCT: u32 = 6;
/// `ATSPI_STATE_FOCUSED`.
pub(crate) const FOCUSED: u32 = 12;
/// `ATSPI_STATE_SHOWING`.
pub(crate) const SHOWING: u32 = 25;
/// `ATSPI_STATE_MANAGES_DESCENDANTS`.
pub(crate) const MANAGES_DESCENDANTS: u32 = 31;
/// `ATSPI_STATE_READ_ONLY`, in the second word.
pub(crate) const READ_ONLY: u32 = 43;

/// The bus's error for a call its callee let pass its timeout.
pub(crate) const NO_REPLY: &str = "org.freedesktop.DBus.Error.NoReply";
/// The error for a method the object does not have.
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
/// The error for an object the app does not serve, or no longer does.
pub(crate) const UNKNOWN_OBJECT: &str = "org.freedesktop.DBus.Error.UnknownObject";
/// The error for arguments the object cannot use.
const INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";

/// A reference as AT-SPI2 sends one: a bus name and a path.
pub(crate) type Reference = (String, OwnedObjectPath);

/// A failed method call, answered by the error's name alone: one of D-Bus's own, or any name the
/// app chose, which the bus carries as it is.
#[derive(Debug)]
struct Failure(&'static str);

impl zbus::DBusError for Failure {
    fn create_reply(&self, msg: &zbus::message::Header<'_>) -> zbus::Result<zbus::Message> {
        zbus::Message::error(msg, self.0)?.build(&())
    }

    fn name(&self) -> zbus::names::ErrorName<'_> {
        zbus::names::ErrorName::from_static_str_unchecked(self.0)
    }

    fn description(&self) -> Option<&str> {
        None
    }
}

/// `GetState`'s two words for `states`.
pub(crate) fn words(states: &[u32]) -> Vec<u32> {
    let mut words = vec![0_u32; 2];
    for &state in states {
        words[usize::try_from(state / 32).unwrap()] |= 1 << (state % 32);
    }
    words
}

/// How many calls `counter` counted.
pub(crate) fn count(counter: &AtomicU32) -> u32 {
    counter.load(Ordering::SeqCst)
}

/// The fake launcher's `org.a11y.Bus`.
struct Launcher {
    address: Arc<Mutex<String>>,
}

#[zbus::interface(name = "org.a11y.Bus")]
impl Launcher {
    fn get_address(&self) -> String {
        self.address.lock().unwrap().clone()
    }
}

/// The fake launcher's `org.a11y.Status`, which counts the writes of `IsEnabled`.
struct Status {
    enabled: Arc<AtomicBool>,
    writes: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.a11y.Status")]
impl Status {
    #[zbus(property)]
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    #[zbus(property)]
    fn set_is_enabled(&mut self, enabled: bool) {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.enabled.store(enabled, Ordering::SeqCst);
    }
}

/// A session bus with the fake launcher on it.
pub(crate) struct Session {
    /// A connection to the session bus, as the client is given one.
    pub(crate) conn: zbus::Connection,
    /// What the launcher's `GetAddress` answers.
    pub(crate) address: Arc<Mutex<String>>,
    /// What `IsEnabled` reads: off at the start.
    pub(crate) enabled: Arc<AtomicBool>,
    /// How many times `IsEnabled` was written.
    pub(crate) writes: Arc<AtomicU32>,
    _launcher: zbus::Connection,
    _bus: PrivateBus,
}

impl Session {
    /// A session bus whose launcher answers `address`.
    pub(crate) async fn start(address: &str) -> Self {
        let bus = PrivateBus::start();
        let address = Arc::new(Mutex::new(address.to_owned()));
        let launcher = Launcher { address: Arc::clone(&address) };
        let launcher = serve(&bus, "org.a11y.Bus", LAUNCHER, launcher).await;
        let (enabled, writes) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU32::new(0)));
        let status = Status { enabled: Arc::clone(&enabled), writes: Arc::clone(&writes) };
        assert!(launcher.object_server().at(LAUNCHER, status).await.unwrap(), "Status is served");
        let conn = bus.connect().await;
        Self { conn, address, enabled, writes, _launcher: launcher, _bus: bus }
    }
}

/// The calls one fake object received.
#[derive(Debug, Clone, Default)]
pub(crate) struct Calls {
    /// How many `GetChildren` calls listed its children.
    pub(crate) listings: Arc<AtomicU32>,
    /// How many `GetChildAtIndex` calls read a child.
    pub(crate) index_reads: Arc<AtomicU32>,
    /// The range each `Text.GetText` call asked for.
    pub(crate) text_reads: Arc<Mutex<Vec<(i32, i32)>>>,
}

/// What a fake object answers. The default has no name, role 0, no states and no children.
#[derive(Debug, Clone, Default)]
pub(crate) struct Node {
    /// `Name`.
    pub(crate) name: String,
    /// `GetRole`'s number.
    pub(crate) role: u32,
    /// `GetState`'s words.
    pub(crate) states: Vec<u32>,
    /// `GetInterfaces`' names.
    pub(crate) interfaces: Vec<String>,
    /// `ChildCount`, which a test can make wrong.
    pub(crate) child_count: i32,
    /// What `GetChildren` answers, and `GetChildAtIndex` by index.
    pub(crate) children: Vec<Reference>,
    /// `GetChildren` is an unknown method, as in an app that has none.
    pub(crate) no_get_children: bool,
    /// Every read answers `org.freedesktop.DBus.Error.NoReply`, as the bus does for an app
    /// that let a call pass its timeout.
    pub(crate) no_reply: bool,
    /// Every method, but no property, answers this error name, one the app chose.
    pub(crate) error_name: Option<&'static str>,
    /// `GetChildren` and `GetChildAtIndex` answer this error name, and every other read answers.
    pub(crate) listing_error: Option<&'static str>,
    /// `GetChildren` and `GetChildAtIndex` each wait this long on tokio's clock first, for a
    /// test in paused time.
    pub(crate) slow_listing: Option<Duration>,
    /// `GetRole` never answers.
    pub(crate) silent: bool,
    /// The text `Text.GetText` answers, whatever range is asked for; `None` serves no `Text`.
    pub(crate) text: Option<String>,
    /// The calls it receives.
    pub(crate) calls: Calls,
}

impl Node {
    /// A showing object with `children`, which says how many it has.
    pub(crate) fn showing(name: &str, children: Vec<Reference>) -> Self {
        let child_count = i32::try_from(children.len()).unwrap();
        Self {
            name: name.to_owned(),
            states: words(&[SHOWING]),
            child_count,
            children,
            ..Self::default()
        }
    }

    /// Fails a property read as an app past its timeout does, when the node says so.
    fn property(&self) -> fdo::Result<()> {
        if self.no_reply {
            return Err(fdo::Error::NoReply("the app did not answer".to_owned()));
        }
        Ok(())
    }

    /// Fails a method as the node says: as an app past its timeout, or with its own error name.
    fn read(&self) -> Result<(), Failure> {
        if self.no_reply {
            return Err(Failure(NO_REPLY));
        }
        self.error_name.map_or(Ok(()), |name| Err(Failure(name)))
    }

    /// What a listing, by index or not, does first: waits when the node is slow, and fails when
    /// the node says so.
    async fn listing(&self) -> Result<(), Failure> {
        if let Some(wait) = self.slow_listing {
            tokio::time::sleep(wait).await;
        }
        self.listing_error.map_or(Ok(()), |name| Err(Failure(name)))
    }
}

/// A fake object's `org.a11y.atspi.Accessible`.
struct Object(Node);

#[zbus::interface(name = "org.a11y.atspi.Accessible")]
impl Object {
    #[zbus(property)]
    fn name(&self) -> fdo::Result<String> {
        self.0.property().map(|()| self.0.name.clone())
    }

    #[zbus(property)]
    fn child_count(&self) -> fdo::Result<i32> {
        self.0.property().map(|()| self.0.child_count)
    }

    async fn get_role(&self) -> Result<u32, Failure> {
        if self.0.silent {
            std::future::pending::<()>().await;
        }
        self.0.read().map(|()| self.0.role)
    }

    fn get_state(&self) -> Result<Vec<u32>, Failure> {
        self.0.read().map(|()| self.0.states.clone())
    }

    fn get_interfaces(&self) -> Result<Vec<String>, Failure> {
        self.0.read().map(|()| self.0.interfaces.clone())
    }

    async fn get_children(&self) -> Result<Vec<Reference>, Failure> {
        self.0.calls.listings.fetch_add(1, Ordering::SeqCst);
        self.0.listing().await?;
        if self.0.no_get_children {
            return Err(Failure(UNKNOWN_METHOD));
        }
        self.0.read().map(|()| self.0.children.clone())
    }

    async fn get_child_at_index(&self, index: i32) -> Result<Reference, Failure> {
        self.0.calls.index_reads.fetch_add(1, Ordering::SeqCst);
        self.0.listing().await?;
        self.0.read()?;
        let child = usize::try_from(index).ok().and_then(|index| self.0.children.get(index));
        child.cloned().ok_or(Failure(INVALID_ARGS))
    }
}

/// A fake object's `org.a11y.atspi.Text`. It answers its whole text whatever range is asked
/// for, so the bound a test sees is the client's own.
struct Text {
    text: String,
    reads: Arc<Mutex<Vec<(i32, i32)>>>,
}

#[zbus::interface(name = "org.a11y.atspi.Text")]
impl Text {
    fn get_text(&self, start_offset: i32, end_offset: i32) -> String {
        self.reads.lock().unwrap().push((start_offset, end_offset));
        self.text.clone()
    }
}

/// A fake app on an accessibility bus: a connection that serves the objects a test adds.
pub(crate) struct App {
    conn: zbus::Connection,
}

impl App {
    /// An app with no objects yet.
    pub(crate) async fn start(bus: &PrivateBus) -> Self {
        Self { conn: bus.connect().await }
    }

    /// The unique name of the app's connection.
    pub(crate) fn name(&self) -> String {
        self.conn.unique_name().unwrap().to_string()
    }

    /// Closes the app's connection, as an app that quits does.
    pub(crate) async fn close(self) {
        self.conn.close().await.unwrap();
    }

    /// A reference to the app's object at `path`, as AT-SPI2 sends one.
    pub(crate) fn reference(&self, path: &str) -> Reference {
        (self.name(), OwnedObjectPath::try_from(path).unwrap())
    }

    /// References to the app's objects at `paths`.
    pub(crate) fn references(&self, paths: &[&str]) -> Vec<Reference> {
        paths.iter().map(|path| self.reference(path)).collect()
    }

    /// The app's object at `path`, as the client names it.
    pub(crate) fn object(&self, path: &str) -> ObjectRef {
        let bus = self.conn.unique_name().unwrap().to_owned();
        ObjectRef { bus, path: OwnedObjectPath::try_from(path).unwrap() }
    }

    /// Serves `node` at `path`, and its `Text` when it has text; the calls it will receive.
    pub(crate) async fn add(&self, path: &str, node: Node) -> Calls {
        let calls = node.calls.clone();
        let server = self.conn.object_server();
        if let Some(text) = node.text.clone() {
            let text = Text { text, reads: Arc::clone(&calls.text_reads) };
            assert!(server.at(path, text).await.unwrap(), "Text is served at {path}");
        }
        assert!(server.at(path, Object(node)).await.unwrap(), "{path} is served");
        calls
    }

    /// Serves a showing object at each path, with the children each lists by path.
    pub(crate) async fn tree(&self, nodes: &[(&str, &[&str])]) {
        for (path, children) in nodes {
            let name = path.rsplit('/').next().unwrap();
            self.add(path, Node::showing(name, self.references(children))).await;
        }
    }
}

/// Serves the registry on `bus`, whose root lists `apps`.
pub(crate) async fn registry(bus: &PrivateBus, apps: Vec<Reference>) -> zbus::Connection {
    registry_root(bus, Node::showing("main", apps)).await
}

/// Serves the registry on `bus`, whose root is `root`.
pub(crate) async fn registry_root(bus: &PrivateBus, root: Node) -> zbus::Connection {
    serve(bus, REGISTRY, ROOT, Object(root)).await
}

//! The notification client against fake notification servers on a private bus: a toast's
//! arguments and hints, the body's escaping with and without `body-markup`, the capabilities
//! kept per client and per server, ids and closing, servers that restart or take the name over,
//! malformed replies, errors that never carry the text, and a bus where no server owns the name.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use eileen_contracts::core_io::Toast;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_dbus_linux::{Names, owner_changes};
use eileen_shell_linux::{
    DbusError, NotificationId, NotificationServer, NotificationServerInfo, Notifier,
    NotifierSettings,
};
use zbus::object_server::{Interface, SignalEmitter};
use zbus::zvariant::{OwnedValue, Value};

/// The server's well-known name.
const NAME: &str = "org.freedesktop.Notifications";
/// Its object.
const PATH: &str = "/org/freedesktop/Notifications";
/// Its interface.
const INTERFACE: &str = "org.freedesktop.Notifications";

/// A title and a body that must never reach an error.
const TITLE: &str = "Results from Dr. Rivera";
const BODY: &str = "Your appointment moved to Friday";

/// A hint's value as the fake received it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hint {
    /// A `BYTE`.
    Byte(u8),
    /// A `STRING`.
    Text(String),
    /// Any other type, by its signature.
    Other(String),
}

impl Hint {
    fn of(value: &Value<'_>) -> Self {
        if let Value::U8(byte) = value {
            return Self::Byte(*byte);
        }
        if let Value::Str(text) = value {
            return Self::Text(text.as_str().to_owned());
        }
        Self::Other(value.value_signature().to_string())
    }
}

/// One `Notify` call as the fake received it.
#[derive(Debug, Clone)]
struct NotifyCall {
    app_name: String,
    replaces_id: u32,
    app_icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: BTreeMap<String, Hint>,
    expire_timeout: i32,
}

/// What a fake server was asked.
#[derive(Debug, Default)]
struct Log {
    notifies: Vec<NotifyCall>,
    closes: Vec<u32>,
    capability_reads: u32,
}

/// A fake server's log, shared with the test.
#[derive(Debug, Clone, Default)]
struct Recorder(Arc<Mutex<Log>>);

impl Recorder {
    fn record(&self, entry: impl FnOnce(&mut Log)) {
        entry(&mut self.0.lock().unwrap());
    }

    fn notifies(&self) -> Vec<NotifyCall> {
        self.0.lock().unwrap().notifies.clone()
    }

    fn closes(&self) -> Vec<u32> {
        self.0.lock().unwrap().closes.clone()
    }

    fn capability_reads(&self) -> u32 {
        self.0.lock().unwrap().capability_reads
    }
}

/// The fake's errors, named outside the bus's own error names, as a real server names its own.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.Notifications")]
enum FakeError {
    /// What zbus itself reports.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The id names no open notification.
    InvalidId(String),
    /// The server will not show the notification.
    Refused(String),
}

/// A notification server that records every call. Ids count up from `next_id`, and
/// `CloseNotification` sends `NotificationClosed` with reason 3, as the specification asks.
struct FakeServer {
    capabilities: Vec<String>,
    log: Recorder,
    next_id: u32,
    open: BTreeSet<u32>,
    /// Refuse every `Notify`, with an error message that repeats the title and the body.
    refuse: bool,
}

impl FakeServer {
    fn new(capabilities: &[&str]) -> (Self, Recorder) {
        let log = Recorder::default();
        let server = Self {
            capabilities: capabilities.iter().map(|name| (*name).to_owned()).collect(),
            log: log.clone(),
            next_id: 1,
            open: BTreeSet::new(),
            refuse: false,
        };
        (server, log)
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl FakeServer {
    fn get_capabilities(&self) -> Vec<String> {
        self.log.record(|log| log.capability_reads += 1);
        self.capabilities.clone()
    }

    #[expect(clippy::too_many_arguments, reason = "the specification's Notify takes eight")]
    fn notify(
        &mut self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> Result<u32, FakeError> {
        if self.refuse {
            return Err(FakeError::Refused(format!("cannot show {summary}: {body}")));
        }
        let hints = hints.into_iter().map(|(key, value)| (key, Hint::of(&value))).collect();
        let call = NotifyCall {
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
        };
        self.log.record(|log| log.notifies.push(call));
        let id = self.next_id;
        self.next_id += 1;
        self.open.insert(id);
        Ok(id)
    }

    async fn close_notification(
        &mut self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), FakeError> {
        self.log.record(|log| log.closes.push(id));
        if !self.open.remove(&id) {
            return Err(FakeError::InvalidId(format!("no notification {id}")));
        }
        emitter.emit(INTERFACE, "NotificationClosed", &(id, 3_u32)).await?;
        Ok(())
    }

    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn get_server_information(&self) -> (String, String, String, String) {
        let fields = ["fake-notifyd", "Eileen's tests", "0.1", "1.3"];
        fields.map(str::to_owned).into()
    }
}

/// A server whose replies have the wrong types: `Notify` answers a string for the id and
/// `GetServerInformation` two strings for four.
struct WrongReplies;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl WrongReplies {
    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_owned()]
    }

    // With no arguments declared, zbus takes the call whatever its arguments.
    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn notify(&self) -> String {
        "7".to_owned()
    }

    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn get_server_information(&self) -> (String, String) {
        ("fake-notifyd".to_owned(), "Eileen's tests".to_owned())
    }
}

/// A server whose capability list is a number.
struct WrongCapabilities;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl WrongCapabilities {
    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn get_capabilities(&self) -> u32 {
        7
    }
}

/// A server that gives the bus's answer when it could not start a server for the name, which
/// the bus sends for a failed activation. The private bus starts no services, so a fake sends it.
struct Unstartable;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Unstartable {
    #[expect(clippy::unused_self, reason = "an interface method takes self")]
    fn get_capabilities(&self) -> zbus::fdo::Result<Vec<String>> {
        Err(zbus::fdo::Error::SpawnChildExited("the server exited at once".to_owned()))
    }
}

/// Serves `server` as the notification server on `bus`, with the name requested as `flags` set
/// it: whether another may take it over, or whether this one takes it over.
async fn serve_with(
    bus: &PrivateBus,
    server: impl Interface,
    flags: impl FnOnce(zbus::connection::Builder<'static>) -> zbus::connection::Builder<'static>,
) -> zbus::Connection {
    flags(zbus::connection::Builder::address(bus.address()).unwrap())
        .name(NAME)
        .unwrap()
        .serve_at(PATH, server)
        .unwrap()
        .build()
        .await
        .unwrap()
}

/// Stops the fake server on `service` and returns once the bus has seen it go: its
/// `NameOwnerChanged` for the name, with no new owner, has come. The bus answers any later call
/// to the server as one to a connection that is gone.
async fn stop(bus: &PrivateBus, service: zbus::Connection) {
    let watcher = bus.connect().await;
    let mut changes = owner_changes(&watcher, Names::Name(NAME)).await.unwrap();
    service.close().await.unwrap();
    while changes.next().await.expect("the bus stays").new.is_some() {}
}

/// A client with the default settings.
async fn client(bus: &PrivateBus) -> Notifier {
    Notifier::new(&bus.connect().await, &NotifierSettings::default())
}

/// A toast about no job.
fn toast(title: &str, body: &str) -> Toast {
    Toast { title: title.to_owned(), body: body.to_owned(), job: None }
}

/// The server a fake's `connection` is.
fn server_of(connection: &zbus::Connection) -> NotificationServer {
    NotificationServer::new(connection.unique_name().unwrap().as_str()).unwrap()
}

/// The id the fake on `connection` numbered `number`.
fn id(connection: &zbus::Connection, number: u32) -> NotificationId {
    NotificationId::new(server_of(connection), number).unwrap()
}

/// Asserts that `error` is `Invalid` for `what` because the reply's types were not the
/// specification's: its detail is zvariant's signature mismatch, which names the reply's
/// signature, and never the name of an error the server answered instead.
fn assert_mistyped(error: &DbusError, what: &str, signature: &str) {
    let DbusError::Invalid { what: call, detail } = error else {
        panic!("{what}: not Invalid: {error:?}");
    };
    assert_eq!(*call, what, "the call");
    assert!(
        detail.starts_with("Signature mismatch") && detail.contains(&format!("`{signature}`")),
        "{what}: a reply of the wrong types, not an error reply: {detail}"
    );
    assert!(!detail.contains("org.freedesktop."), "{what}: no error name: {detail}");
}

/// `future`, unchanged: it compiles only if the future may move between threads, as a
/// `ShellHost` adapter's may.
fn sendable<F: Send>(future: F) -> F {
    future
}

/// The one `Notify` a fake listing `capabilities` receives for `toast`.
async fn sent(capabilities: &[&str], toast: &Toast) -> NotifyCall {
    let bus = PrivateBus::start();
    let (server, log) = FakeServer::new(capabilities);
    let _service = serve(&bus, NAME, PATH, server).await;
    client(&bus).await.notify(toast).await.unwrap();
    let mut calls = log.notifies();
    assert_eq!(calls.len(), 1, "one Notify per toast");
    calls.pop().unwrap()
}

#[test]
fn a_server_is_a_unique_bus_name() {
    let server = NotificationServer::new(":1.42").expect("a unique name");
    assert_eq!(server.as_str(), ":1.42", "kept as given");
    for other in ["", ":1", "1.42", "org.freedesktop.Notifications", "org.freedesktop.DBus"] {
        assert_eq!(NotificationServer::new(other), None, "{other:?} names no connection");
    }
}

#[test]
fn an_id_is_never_zero_and_names_its_server() {
    let server = NotificationServer::new(":1.7").expect("a unique name");
    assert_eq!(NotificationId::new(server.clone(), 0), None, "zero names no notification");
    let id = NotificationId::new(server.clone(), 7).expect("a notification");
    assert_eq!((id.get(), id.server()), (7, &server), "kept as given");
    let max = NotificationId::new(server, u32::MAX).map(|id| id.get());
    assert_eq!(max, Some(u32::MAX), "max");
    let restarted = NotificationServer::new(":1.8").expect("a unique name");
    assert_ne!(
        NotificationId::new(restarted, 7),
        Some(id),
        "the same number from another server is another notification"
    );
}

#[test]
fn the_settings_name_eileen_by_default() {
    let settings = NotifierSettings::default();
    assert_eq!(settings.desktop_entry, "eileen", "eileen.desktop");
    assert_eq!(settings.app_icon, "", "no icon of its own");
}

#[tokio::test]
async fn a_toast_is_one_notify_with_the_specified_arguments_and_hints() {
    let call = sent(&["body", "actions"], &toast("Build finished", "All 12 tests passed")).await;
    assert_eq!(call.app_name, "Eileen", "the app name");
    assert_eq!(call.replaces_id, 0, "a new notification, replacing none");
    assert_eq!(call.app_icon, "", "no icon of its own");
    assert_eq!(call.summary, "Build finished", "the title is the summary");
    assert_eq!(call.body, "All 12 tests passed", "the body");
    assert!(call.actions.is_empty(), "no actions, though the server offers them");
    let expected = BTreeMap::from([
        ("desktop-entry".to_owned(), Hint::Text("eileen".to_owned())),
        ("urgency".to_owned(), Hint::Byte(1)),
    ]);
    assert_eq!(call.hints, expected, "exactly these hints: the default desktop entry, normal");
    assert_eq!(call.expire_timeout, -1, "the server's own timeout");
}

#[tokio::test]
async fn the_settings_name_the_icon_and_the_desktop_entry() {
    let bus = PrivateBus::start();
    let (server, log) = FakeServer::new(&["body"]);
    let _service = serve(&bus, NAME, PATH, server).await;
    let settings = NotifierSettings {
        desktop_entry: "org.example.Eileen.desktop".to_owned(),
        app_icon: "eileen-symbolic".to_owned(),
    };
    let notifier = Notifier::new(&bus.connect().await, &settings);
    notifier.notify(&toast("Muted", "Say \"unmute\" to listen again")).await.unwrap();
    let malformed =
        NotifierSettings { desktop_entry: "eil\0een".to_owned(), app_icon: "\0".to_owned() };
    let second = Notifier::new(&bus.connect().await, &malformed);
    second.notify(&toast("Muted", "Again")).await.unwrap();
    let calls = log.notifies();
    assert_eq!(calls[0].app_icon, "eileen-symbolic", "the settings' icon");
    assert_eq!(
        calls[0].hints.get("desktop-entry"),
        Some(&Hint::Text("org.example.Eileen".to_owned())),
        "the desktop file ID, its .desktop suffix dropped"
    );
    assert_eq!(calls[1].app_icon, "", "U+0000 removed from the icon");
    assert_eq!(
        calls[1].hints.get("desktop-entry"),
        Some(&Hint::Text("eileen".to_owned())),
        "and from the desktop file ID"
    );
}

#[tokio::test]
async fn a_body_is_escaped_exactly_when_the_server_reads_markup() {
    let toast = toast("Tom & Jerry <3", "Fish & chips <b>now</b> > later");
    let markup = sent(&["body", "body-markup"], &toast).await;
    assert_eq!(
        markup.body, "Fish &amp; chips &lt;b&gt;now&lt;/b&gt; &gt; later",
        "a server that reads markup shows the plain text only once escaped"
    );
    assert_eq!(markup.summary, "Tom & Jerry <3", "the summary is never markup");
    let plain = sent(&["body"], &toast).await;
    assert_eq!(plain.body, "Fish & chips <b>now</b> > later", "any other server shows it as sent");
    assert_eq!(plain.summary, "Tom & Jerry <3", "the summary as it is");
}

#[tokio::test]
async fn capabilities_are_read_once_per_client_and_kept() {
    let bus = PrivateBus::start();
    let (server, log) = FakeServer::new(&["body", "body-markup", "x-vendor-glow"]);
    let _service = serve(&bus, NAME, PATH, server).await;
    let connection = bus.connect().await;
    let notifier = Notifier::new(&connection, &NotifierSettings::default());
    let capabilities = notifier.capabilities().await.unwrap();
    assert!(capabilities.body_markup(), "body-markup listed");
    assert!(capabilities.contains("x-vendor-glow"), "a vendor's own capability is kept");
    assert!(!capabilities.contains("actions"), "what the server does not list");
    notifier.notify(&toast("One", "1")).await.unwrap();
    notifier.notify(&toast("Two", "2")).await.unwrap();
    assert!(notifier.capabilities().await.unwrap().body_markup(), "the kept list");
    assert_eq!(log.capability_reads(), 1, "read once, then kept");
    let second = Notifier::new(&connection, &NotifierSettings::default());
    second.notify(&toast("Three", "3")).await.unwrap();
    assert_eq!(log.capability_reads(), 2, "each client reads them once");
}

#[tokio::test]
async fn notify_returns_the_server_ids_and_close_sends_them_back() {
    let bus = PrivateBus::start();
    let (mut server, log) = FakeServer::new(&["body"]);
    server.next_id = 41;
    let service = serve(&bus, NAME, PATH, server).await;
    let notifier = client(&bus).await;
    let first = sendable(notifier.notify(&toast("First", "1"))).await.unwrap();
    let second = notifier.notify(&toast("Second", "2")).await.unwrap();
    assert_eq!((first.get(), second.get()), (41, 42), "the server's numbering");
    assert_eq!(first.server(), &server_of(&service), "and the server that showed them");
    sendable(notifier.close(&second)).await.unwrap();
    notifier.close(&first).await.unwrap();
    assert_eq!(log.closes(), [42, 41], "each close names its notification");
}

#[tokio::test]
async fn closing_a_notification_that_is_gone_succeeds() {
    let bus = PrivateBus::start();
    let (server, log) = FakeServer::new(&["body"]);
    let service = serve(&bus, NAME, PATH, server).await;
    let notifier = client(&bus).await;
    let shown = notifier.notify(&toast("Done", "The job finished")).await.unwrap();
    notifier.close(&shown).await.unwrap();
    assert_eq!(
        notifier.close(&shown).await,
        Ok(()),
        "closed already: the server's error is no error"
    );
    assert_eq!(notifier.close(&id(&service, 99)).await, Ok(()), "never shown");
    assert_eq!(log.closes(), [shown.get(), shown.get(), 99], "all three reached the server");
}

#[tokio::test]
async fn closing_on_a_server_that_is_gone_succeeds() {
    let bus = PrivateBus::start();
    let (server, _log) = FakeServer::new(&["body"]);
    let service = serve(&bus, NAME, PATH, server).await;
    let notifier = client(&bus).await;
    let shown = notifier.notify(&toast("Done", "The job finished")).await.unwrap();
    stop(&bus, service).await;
    assert_eq!(notifier.close(&shown).await, Ok(()), "gone with its server");
    let never = NotificationId::new(NotificationServer::new(":1.999").unwrap(), 1).unwrap();
    assert_eq!(notifier.close(&never).await, Ok(()), "a server that never ran");
}

#[tokio::test]
async fn the_client_follows_a_server_that_starts_later() {
    let bus = PrivateBus::start();
    let notifier = client(&bus).await;
    let early = notifier.notify(&toast("Ready", "Eileen is listening")).await;
    assert!(matches!(early, Err(DbusError::Unavailable { .. })), "no server yet: {early:?}");
    let (server, log) = FakeServer::new(&["body"]);
    let service = serve(&bus, NAME, PATH, server).await;
    let shown = notifier.notify(&toast("Ready", "Eileen is listening")).await.unwrap();
    assert_eq!(shown.server(), &server_of(&service), "the server that started");
    assert_eq!(log.capability_reads(), 1, "the failed read was not kept, so this one asked");
}

#[tokio::test]
async fn ids_stay_with_the_server_that_showed_them_across_a_restart() {
    let bus = PrivateBus::start();
    let (first, _first_log) = FakeServer::new(&["body"]);
    let first = serve(&bus, NAME, PATH, first).await;
    let notifier = client(&bus).await;
    let ours = notifier.notify(&toast("Approval", "Push the branch?")).await.unwrap();
    assert_eq!(ours.server(), &server_of(&first), "the server that showed it");
    stop(&bus, first).await;
    let (second, second_log) = FakeServer::new(&["body"]);
    let _second = serve(&bus, NAME, PATH, second).await;
    let other_app = client(&bus).await;
    let theirs = other_app.notify(&toast("Mail", "3 new messages")).await.unwrap();
    assert_eq!(theirs.get(), ours.get(), "it numbers from 1 again");
    assert_ne!(theirs, ours, "yet its notification is not ours");
    assert_eq!(notifier.close(&ours).await, Ok(()), "ours went with its server");
    assert!(second_log.closes().is_empty(), "and closing it never reached the new server");
}

#[tokio::test]
async fn capabilities_follow_the_name_to_a_new_server() {
    let bus = PrivateBus::start();
    let (first, first_log) = FakeServer::new(&["body"]);
    let first = serve(&bus, NAME, PATH, first).await;
    let notifier = client(&bus).await;
    let markup = toast("Build", "<b>green</b> & done");
    notifier.notify(&markup).await.unwrap();
    assert_eq!(first_log.notifies()[0].body, "<b>green</b> & done", "no body-markup: as written");
    stop(&bus, first).await;
    let (second, second_log) = FakeServer::new(&["body", "body-markup"]);
    let second = serve(&bus, NAME, PATH, second).await;
    let shown = notifier.notify(&markup).await.unwrap();
    assert_eq!(shown.server(), &server_of(&second), "the new server showed it");
    assert_eq!(
        second_log.notifies()[0].body,
        "&lt;b&gt;green&lt;/b&gt; &amp; done",
        "escaped for the server that shows it"
    );
    assert!(notifier.capabilities().await.unwrap().body_markup(), "its capabilities are kept");
    assert_eq!(second_log.capability_reads(), 1, "read once");
    assert_eq!(first_log.capability_reads(), 1, "as the first server's were");
}

#[tokio::test]
async fn a_server_that_takes_the_name_over_gets_the_next_toast_and_old_ids_stay_with_the_old() {
    let bus = PrivateBus::start();
    let (first, first_log) = FakeServer::new(&["body"]);
    let _first = serve_with(&bus, first, |builder| builder.allow_name_replacements(true)).await;
    let notifier = client(&bus).await;
    let old = notifier.notify(&toast("One", "1 < 2")).await.unwrap();
    let (second, second_log) = FakeServer::new(&["body", "body-markup"]);
    let second = serve_with(&bus, second, |builder| builder.replace_existing_names(true)).await;
    let new = notifier.notify(&toast("Two", "2 < 3")).await.unwrap();
    assert_eq!(new.server(), &server_of(&second), "the toast goes to the name's owner now");
    assert_eq!(second_log.notifies()[0].body, "2 &lt; 3", "escaped for it");
    assert_eq!(first_log.notifies().len(), 1, "the first server got the first toast only");
    notifier.close(&old).await.unwrap();
    notifier.close(&new).await.unwrap();
    assert_eq!(first_log.closes(), [old.get()], "an id goes back to the server that showed it");
    assert_eq!(second_log.closes(), [new.get()], "both numbered 1, each to its own");
}

#[tokio::test]
async fn without_a_server_every_call_is_unavailable() {
    let bus = PrivateBus::start();
    let notifier = client(&bus).await;
    let error = notifier.notify(&toast(TITLE, BODY)).await.unwrap_err();
    assert_eq!(
        error,
        DbusError::Unavailable { what: "org.freedesktop.Notifications.GetCapabilities" },
        "the first toast asks for the capabilities first"
    );
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(TITLE) && !shown.contains(BODY), "no text in {shown}");
    assert_eq!(
        notifier.capabilities().await,
        Err(DbusError::Unavailable { what: "org.freedesktop.Notifications.GetCapabilities" }),
        "capabilities, still unread"
    );
    assert_eq!(
        notifier.server_information().await,
        Err(DbusError::Unavailable { what: "org.freedesktop.Notifications.GetServerInformation" }),
        "server information"
    );
}

#[tokio::test]
async fn a_server_the_bus_could_not_start_is_unavailable() {
    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, Unstartable).await;
    let error = client(&bus).await.notify(&toast(TITLE, BODY)).await.unwrap_err();
    assert_eq!(
        error,
        DbusError::Unavailable { what: "org.freedesktop.Notifications.GetCapabilities" },
        "the bus's Spawn error is the server's absence, not a malformed answer"
    );
}

#[tokio::test]
async fn the_server_names_itself() {
    let bus = PrivateBus::start();
    let (server, _log) = FakeServer::new(&["body"]);
    let _service = serve(&bus, NAME, PATH, server).await;
    let information = client(&bus).await.server_information().await.unwrap();
    let expected = NotificationServerInfo {
        name: Untrusted::new("fake-notifyd".to_owned()),
        vendor: Untrusted::new("Eileen's tests".to_owned()),
        version: Untrusted::new("0.1".to_owned()),
        spec_version: Untrusted::new("1.3".to_owned()),
    };
    assert_eq!(information, expected, "the four fields in order");
}

#[tokio::test]
async fn malformed_replies_are_invalid() {
    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, WrongReplies).await;
    let notifier = client(&bus).await;
    let error = notifier.notify(&toast(TITLE, BODY)).await.unwrap_err();
    assert_mistyped(&error, "org.freedesktop.Notifications.Notify", "s");
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(TITLE) && !shown.contains(BODY), "no text in {shown}");
    let error = notifier.server_information().await.unwrap_err();
    assert_mistyped(&error, "org.freedesktop.Notifications.GetServerInformation", "(ss)");

    let bus = PrivateBus::start();
    let _service = serve(&bus, NAME, PATH, WrongCapabilities).await;
    let notifier = client(&bus).await;
    let error = notifier.notify(&toast(TITLE, BODY)).await.unwrap_err();
    // No toast without knowing whether to escape.
    assert_mistyped(&error, "org.freedesktop.Notifications.GetCapabilities", "u");
}

#[tokio::test]
async fn an_id_of_zero_is_invalid() {
    let bus = PrivateBus::start();
    let (mut server, _log) = FakeServer::new(&["body"]);
    server.next_id = 0;
    let _service = serve(&bus, NAME, PATH, server).await;
    let notifier = client(&bus).await;
    assert_eq!(
        notifier.notify(&toast(TITLE, BODY)).await,
        Err(DbusError::Invalid {
            what: "org.freedesktop.Notifications.Notify",
            detail: "the server answered notification id 0".to_owned(),
        }),
        "the specification forbids zero"
    );
    let next = notifier.notify(&toast(TITLE, BODY)).await.map(|id| id.get());
    assert_eq!(next, Ok(1), "next");
}

#[tokio::test]
async fn title_and_body_never_appear_in_errors() {
    let bus = PrivateBus::start();
    let (mut server, _log) = FakeServer::new(&["body"]);
    server.refuse = true;
    let _service = serve(&bus, NAME, PATH, server).await;
    let error = client(&bus).await.notify(&toast(TITLE, BODY)).await.unwrap_err();
    assert_eq!(
        error,
        DbusError::Invalid {
            what: "org.freedesktop.Notifications.Notify",
            detail: "org.freedesktop.Notifications.Refused".to_owned(),
        },
        "the error's name only, though the server's message repeats the text"
    );
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(TITLE) && !shown.contains(BODY), "no text in {shown}");
}

#[tokio::test]
async fn a_nul_never_reaches_the_bus() {
    let bus = PrivateBus::start();
    let (server, log) = FakeServer::new(&["body", "body-markup"]);
    let _service = serve(&bus, NAME, PATH, server).await;
    let notifier = client(&bus).await;
    let shown = notifier.notify(&toast("Disk\0 full", "Free\0 <space>")).await.unwrap();
    let call = log.notifies().pop().unwrap();
    assert_eq!(call.summary, "Disk full", "U+0000 removed from the summary");
    assert_eq!(call.body, "Free &lt;space&gt;", "and from the body, which is then escaped");
    notifier.close(&shown).await.unwrap();
    assert_eq!(log.closes(), [shown.get()], "the bus kept the connection");
}

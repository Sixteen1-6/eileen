//! The `GlobalShortcuts` client against a fake portal on a private bus: the host registry's call
//! before the session, the bind's shortcuts and the desktop's triggers, refusals and odd
//! answers, the session's signals and its close, a bind left unanswered, and portals without the
//! interface. `shell_host`'s tests use the same fake.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::testing::{PrivateBus, nothing_ready, round_trip, serve};
use eileen_shell_linux::{BoundShortcut, DbusError, Shortcut, ShortcutEvent, ShortcutsSession};
use tokio::sync::mpsc;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// The portal's service and object, and the interfaces the fake serves or signals on.
pub(super) const PORTAL: &str = "org.freedesktop.portal.Desktop";
pub(super) const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";
/// The app ID the tests register.
const APP_ID: &str = "org.eileen.Eileen";
/// The name of a program that sends the portal's signals without being the portal.
const IMPOSTOR: &str = "org.eileen.Test.Impostor";
/// The deadline for a portal that answers: far longer than a private bus needs.
const WAIT: Duration = Duration::from_secs(30);
/// The deadline a silent portal passes, in tokio's paused time.
const DEADLINE: Duration = Duration::from_secs(5);
/// What the client's errors name.
const SESSION_WHAT: &str = "GlobalShortcuts session";
const BIND_WHAT: &str = "GlobalShortcuts binding";

/// How the fake answers a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// `Response` 0 with its results, before the reply.
    Grant,
    /// `Response` with this code and no results, before the reply.
    Code(u32),
    /// The reply alone: the test answers, or lets the deadline pass.
    Later,
}

/// How the fake answers `CreateSession` and `BindShortcuts`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Modes {
    pub(super) create: Mode,
    pub(super) bind: Mode,
}

impl Modes {
    /// Every request granted.
    pub(super) const GRANT: Self = Self { create: Mode::Grant, bind: Mode::Grant };
}

/// A shortcut as a bind lists it: its ID, description and preferred trigger.
pub(super) type Listed = (String, Option<String>, Option<String>);

/// What the fake saw, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Report {
    /// `Register`: the app ID, and whether no other call had come from its caller before.
    Registered { app_id: String, options: Vec<String>, first: bool },
    /// `CreateSession`: the session it made.
    Created(OwnedObjectPath),
    /// `BindShortcuts`.
    Bound(Bind),
    /// A request's `Close`.
    RequestClosed(OwnedObjectPath),
    /// A session's `Close`.
    SessionClosed(OwnedObjectPath),
}

/// One `BindShortcuts` as the fake received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Bind {
    /// The caller's unique name, which the portal's signals go to.
    pub(super) caller: String,
    pub(super) session: OwnedObjectPath,
    pub(super) request: OwnedObjectPath,
    pub(super) shortcuts: Vec<Listed>,
    /// The window the dialog is to be modal to, as the caller named it.
    pub(super) window: String,
}

/// A fake `GlobalShortcuts` portal. It makes each request's and session's handle from the
/// caller's unique name and its token, serves an object there that reports its `Close`, and
/// answers as its modes say; a second bind on a session is answered 2, as GNOME answers it. Its
/// host registry, [`FakeRegistry`], shares what it saw.
#[derive(Debug, Clone)]
pub(super) struct FakeShortcuts {
    modes: Modes,
    reports: mpsc::Sender<Report>,
    /// The callers that have made a call, and the sessions that have had a bind.
    seen: Arc<Mutex<(BTreeSet<String>, BTreeSet<String>)>>,
}

impl FakeShortcuts {
    /// A portal that answers as `modes` say, and what it reports.
    pub(super) fn new(modes: Modes) -> (Self, mpsc::Receiver<Report>) {
        let (reports, received) = mpsc::channel(64);
        (Self { modes, reports, seen: Arc::default() }, received)
    }

    /// The caller of the call `header` carries, and whether it had made no call before.
    fn caller(&self, header: &Header<'_>) -> (String, bool) {
        let caller = header.sender().unwrap().to_string();
        let first = self.seen.lock().unwrap().0.insert(caller.clone());
        (caller, first)
    }

    /// Serves a request object at `path` on `conn`.
    async fn serve_request(&self, conn: &zbus::Connection, path: &OwnedObjectPath) {
        let closer = Closer { path: path.clone(), reports: self.reports.clone() };
        conn.object_server().at(path, FakeRequest(closer)).await.unwrap();
    }
}

#[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
impl FakeShortcuts {
    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let (caller, _first) = self.caller(&header);
        let request = handle("request", &caller, &text(&options, "handle_token").unwrap());
        let session = handle("session", &caller, &text(&options, "session_handle_token").unwrap());
        self.serve_request(conn, &request).await;
        let closer = Closer { path: session.clone(), reports: self.reports.clone() };
        conn.object_server().at(&session, FakeSession(closer)).await?;
        let _ignored = self.reports.send(Report::Created(session.clone())).await;
        // A string, as xdg-desktop-portal sends it: the documented erratum.
        let results = HashMap::from([("session_handle", Value::from(session.to_string()))]);
        answer(conn, &caller, &request, self.modes.create, results).await;
        Ok(request)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the portal's method has four arguments, and zbus hands over two more"
    )]
    async fn bind_shortcuts(
        &self,
        session_handle: OwnedObjectPath,
        shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
        parent_window: &str,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let (caller, _first) = self.caller(&header);
        let request = handle("request", &caller, &text(&options, "handle_token").unwrap());
        self.serve_request(conn, &request).await;
        let again = !self.seen.lock().unwrap().1.insert(session_handle.to_string());
        let listed: Vec<Listed> = shortcuts
            .iter()
            .map(|(id, details)| {
                (id.clone(), text(details, "description"), text(details, "preferred_trigger"))
            })
            .collect();
        let bind = Bind {
            caller: caller.clone(),
            session: session_handle,
            request: request.clone(),
            shortcuts: listed,
            window: parent_window.to_owned(),
        };
        let results = HashMap::from([("shortcuts", triggers(&bind.shortcuts))]);
        let _ignored = self.reports.send(Report::Bound(bind)).await;
        let mode = if again { Mode::Code(2) } else { self.modes.bind };
        answer(conn, &caller, &request, mode, results).await;
        Ok(request)
    }
}

/// The fake portal's host registry, which reports each `Register`.
#[derive(Debug, Clone)]
pub(super) struct FakeRegistry(pub(super) FakeShortcuts);

#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl FakeRegistry {
    async fn register(
        &self,
        app_id: &str,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
    ) {
        let (_caller, first) = self.0.caller(&header);
        let options = options.into_keys().collect();
        let report = Report::Registered { app_id: app_id.to_owned(), options, first };
        let _ignored = self.0.reports.send(report).await;
    }
}

/// A request or session object's report of its `Close`.
#[derive(Debug, Clone)]
struct Closer {
    path: OwnedObjectPath,
    reports: mpsc::Sender<Report>,
}

/// A request the fake made.
struct FakeRequest(Closer);

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl FakeRequest {
    async fn close(&self) {
        let _ignored = self.0.reports.send(Report::RequestClosed(self.0.path.clone())).await;
    }
}

/// A session the fake made.
struct FakeSession(Closer);

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl FakeSession {
    async fn close(&self) {
        let _ignored = self.0.reports.send(Report::SessionClosed(self.0.path.clone())).await;
    }
}

/// Serves `fake` as the portal, with its host registry beside it.
pub(super) async fn serve_portal(bus: &PrivateBus, fake: FakeShortcuts) -> zbus::Connection {
    let portal = serve(bus, PORTAL, PORTAL_PATH, fake.clone()).await;
    portal.object_server().at(PORTAL_PATH, FakeRegistry(fake)).await.unwrap();
    portal
}

/// The handle of the kind `kind` (`request` or `session`) the portal gives `caller`'s `token`.
fn handle(kind: &str, caller: &str, token: &str) -> OwnedObjectPath {
    let caller = caller.trim_start_matches(':').replace('.', "_");
    OwnedObjectPath::try_from(format!("{PORTAL_PATH}/{kind}/{caller}/{token}")).unwrap()
}

/// The text option `key` of `options`.
fn text(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options.get(key).map(|value| <&str>::try_from(value).unwrap().to_owned())
}

/// The desktop's answer to a bind of `listed`: each shortcut with its description and, for one
/// offered with a trigger, a description of the trigger that names it.
pub(super) fn triggers(listed: &[Listed]) -> Value<'static> {
    let described = |(id, description, trigger): &Listed| {
        let mut details: HashMap<String, Value<'static>> = HashMap::new();
        if let Some(description) = description {
            details.insert("description".to_owned(), Value::from(description.clone()));
        }
        if let Some(trigger) = trigger {
            details
                .insert("trigger_description".to_owned(), Value::from(format!("Press {trigger}")));
        }
        (id.clone(), details)
    };
    Value::from(listed.iter().map(described).collect::<Vec<_>>())
}

/// Answers `request` from `conn` to `caller` as `mode` says, with `results` when it grants.
async fn answer(
    conn: &zbus::Connection,
    caller: &str,
    request: &OwnedObjectPath,
    mode: Mode,
    results: HashMap<&str, Value<'_>>,
) {
    match mode {
        Mode::Grant => respond(conn, caller, request, 0, results).await,
        Mode::Code(code) => respond(conn, caller, request, code, HashMap::new()).await,
        Mode::Later => {}
    }
}

/// Emits `Response` with `code` and `results` on `request` from `conn` to `caller`.
pub(super) async fn respond(
    conn: &zbus::Connection,
    caller: &str,
    request: &OwnedObjectPath,
    code: u32,
    results: HashMap<&str, Value<'_>>,
) {
    conn.emit_signal(Some(caller), request, REQUEST, "Response", &(code, results)).await.unwrap();
}

/// Emits the portal's `member`, `Activated` or `Deactivated`, for the shortcut `id` of
/// `session`, from `conn` to `caller`.
pub(super) async fn press(
    conn: &zbus::Connection,
    caller: &str,
    member: &str,
    session: &OwnedObjectPath,
    id: &str,
) {
    let body = (session, id, 7_u64, HashMap::<&str, Value<'_>>::new());
    conn.emit_signal(Some(caller), PORTAL_PATH, SHORTCUTS, member, &body).await.unwrap();
}

/// What a bind lists for `shortcuts`.
pub(super) fn listed(shortcuts: &[(&str, Option<&str>, Option<&str>)]) -> Vec<Listed> {
    let owned = |text: Option<&str>| text.map(str::to_owned);
    shortcuts
        .iter()
        .map(|(id, description, trigger)| ((*id).to_owned(), owned(*description), owned(*trigger)))
        .collect()
}

/// The hotkey a test binds.
fn mute() -> Shortcut {
    let trigger = Some("CTRL+ALT+m".to_owned());
    Shortcut { id: "hotkey-1".to_owned(), description: "Mute or unmute".to_owned(), trigger }
}

/// A session from the fake on a new connection to `bus`, and that connection.
async fn opened(bus: &PrivateBus) -> (zbus::Connection, ShortcutsSession) {
    let client = bus.connect().await;
    let session = ShortcutsSession::open(&client, APP_ID, WAIT).await.unwrap();
    (client, session)
}

#[tokio::test]
async fn the_app_registers_before_the_session_and_the_bind_reports_triggers() {
    let bus = PrivateBus::start();
    let (fake, mut reports) = FakeShortcuts::new(Modes::GRANT);
    let _portal = serve_portal(&bus, fake).await;
    let (_client, session) = opened(&bus).await;
    let registered = Report::Registered { app_id: APP_ID.to_owned(), options: vec![], first: true };
    assert_eq!(reports.recv().await, Some(registered), "before any other call to the portal");
    let Some(Report::Created(created)) = reports.recv().await else { panic!("then the session") };
    assert_eq!(session.handle().as_str(), created.as_str(), "the handle the portal named");
    let dictation = Shortcut {
        id: "dictation".to_owned(),
        description: "Hold to dictate".to_owned(),
        trigger: None,
    };
    let bound = session.bind(&[mute(), dictation], WAIT).await.unwrap();
    let Some(Report::Bound(bind)) = reports.recv().await else { panic!("one bind") };
    let expected = listed(&[
        ("hotkey-1", Some("Mute or unmute"), Some("CTRL+ALT+m")),
        ("dictation", Some("Hold to dictate"), None),
    ]);
    assert_eq!(bind.shortcuts, expected, "each ID and description, and a trigger only when set");
    assert_eq!(bind.session.as_str(), session.handle().as_str(), "on the session");
    assert_eq!(bind.window, "", "with no window for the dialog to sit on");
    let pressed = |id: &str, trigger: &str| BoundShortcut {
        id: id.to_owned(),
        trigger: Untrusted::new(trigger.to_owned()),
    };
    assert_eq!(
        bound,
        [pressed("hotkey-1", "Press CTRL+ALT+m"), pressed("dictation", "")],
        "the desktop's own words for each trigger, and none for one it gave none"
    );
}

#[tokio::test]
async fn a_cancelled_bind_is_refused() {
    let bus = PrivateBus::start();
    let (fake, _reports) = FakeShortcuts::new(Modes { bind: Mode::Code(1), ..Modes::GRANT });
    let _portal = serve_portal(&bus, fake).await;
    let (_client, session) = opened(&bus).await;
    assert_eq!(
        session.bind(&[mute()], WAIT).await,
        Err(DbusError::Refused { what: BIND_WHAT, reason: "cancelled by the user" }),
        "code 1, the documentation's cancel"
    );
    assert_eq!(
        session.bind(&[mute()], WAIT).await,
        Err(DbusError::Refused { what: BIND_WHAT, reason: "refused or cancelled" }),
        "code 2, which GNOME answers a dismissed dialog with, and a second bind on a session"
    );
}

#[tokio::test]
async fn a_session_the_portal_refuses_or_answers_oddly_is_not_opened() {
    for (code, error) in [
        (2, DbusError::Refused { what: SESSION_WHAT, reason: "refused or cancelled" }),
        (3, DbusError::Invalid { what: "portal response", detail: "code 3".to_owned() }),
    ] {
        let bus = PrivateBus::start();
        let (fake, _reports) =
            FakeShortcuts::new(Modes { create: Mode::Code(code), ..Modes::GRANT });
        let _portal = serve_portal(&bus, fake).await;
        let client = bus.connect().await;
        let opened = ShortcutsSession::open(&client, APP_ID, WAIT).await;
        assert_eq!(opened.unwrap_err(), error, "code {code}");
    }
}

#[tokio::test]
async fn only_this_sessions_signals_from_the_portal_are_events() {
    let bus = PrivateBus::start();
    let (fake, _reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = serve_portal(&bus, fake.clone()).await;
    let (client, session) = opened(&bus).await;
    let mut events = session.events().await.unwrap();
    let to = client.unique_name().unwrap().to_string();
    let ours = OwnedObjectPath::from(session.handle().to_owned());
    let theirs = handle("session", ":1.999", "other");
    press(&portal, &to, "Activated", &theirs, "hotkey-1").await;
    portal.emit_signal(Some(&*to), PORTAL_PATH, SHORTCUTS, "Activated", &("odd",)).await.unwrap();
    // Served, so that it answers the round trip below.
    let impostor = serve(&bus, IMPOSTOR, PORTAL_PATH, FakeRegistry(fake)).await;
    press(&impostor, &to, "Activated", &ours, "hotkey-1").await;
    round_trip(&client, Some(PORTAL)).await;
    round_trip(&client, Some(IMPOSTOR)).await;
    assert!(
        nothing_ready(&mut events),
        "another session's press, a malformed one, and one from a connection that is not the \
         portal are passed over"
    );
    press(&portal, &to, "Activated", &ours, "hotkey-1").await;
    press(&portal, &to, "Deactivated", &ours, "hotkey-1").await;
    let down = Some(ShortcutEvent::Activated("hotkey-1".to_owned()));
    assert_eq!(events.next().await, down, "the session's own press");
    let up = Some(ShortcutEvent::Deactivated("hotkey-1".to_owned()));
    assert_eq!(events.next().await, up, "and its release, in order");
}

#[tokio::test]
async fn the_sessions_close_ends_its_events() {
    let bus = PrivateBus::start();
    let (fake, _reports) = FakeShortcuts::new(Modes::GRANT);
    let portal = serve_portal(&bus, fake).await;
    let (client, session) = opened(&bus).await;
    let mut events = session.events().await.unwrap();
    let to = client.unique_name().unwrap().to_string();
    let ours = OwnedObjectPath::from(session.handle().to_owned());
    let no_details = (HashMap::<&str, Value<'_>>::new(),);
    let theirs = handle("session", ":1.999", "other");
    portal.emit_signal(Some(&*to), &theirs, SESSION, "Closed", &no_details).await.unwrap();
    press(&portal, &to, "Activated", &ours, "hotkey-1").await;
    let down = Some(ShortcutEvent::Activated("hotkey-1".to_owned()));
    assert_eq!(events.next().await, down, "another session's close is passed over");
    portal.emit_signal(Some(&*to), &ours, SESSION, "Closed", &no_details).await.unwrap();
    assert_eq!(events.next().await, Some(ShortcutEvent::Closed), "the session's own close");
    press(&portal, &to, "Activated", &ours, "hotkey-1").await;
    assert_eq!(events.next().await, None, "and the stream ends there");
}

#[tokio::test(start_paused = true)]
async fn a_bind_left_unanswered_closes_its_request_at_the_deadline() {
    let bus = PrivateBus::start();
    let (fake, mut reports) = FakeShortcuts::new(Modes { bind: Mode::Later, ..Modes::GRANT });
    let _portal = serve_portal(&bus, fake).await;
    let (_client, session) = opened(&bus).await;
    assert_eq!(
        session.bind(&[mute()], DEADLINE).await,
        Err(DbusError::DeadlineExceeded { call: "portal response" }),
        "the dialog was never answered"
    );
    let request = loop {
        if let Some(Report::Bound(bind)) = reports.recv().await {
            break bind.request;
        }
    };
    loop {
        if let Some(Report::RequestClosed(closed)) = reports.recv().await {
            assert_eq!(closed, request, "the bind's own request, closed past the deadline");
            break;
        }
    }
}

#[tokio::test]
async fn a_portal_without_the_interface_is_not_found_and_no_portal_unavailable() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    assert_eq!(
        ShortcutsSession::open(&client, APP_ID, WAIT).await.unwrap_err(),
        DbusError::Unavailable { what: SESSION_WHAT },
        "nothing owns the portal's name"
    );
    let (fake, _reports) = FakeShortcuts::new(Modes::GRANT);
    let _portal = serve(&bus, PORTAL, PORTAL_PATH, FakeRegistry(fake)).await;
    let client = bus.connect().await;
    assert_eq!(
        ShortcutsSession::open(&client, APP_ID, WAIT).await.unwrap_err(),
        DbusError::NotFound { what: SESSION_WHAT },
        "a portal without GlobalShortcuts; Ubuntu 24.04's lacks the registry too"
    );
}

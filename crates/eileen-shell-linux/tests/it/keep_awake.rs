//! Keeping the session awake against fake logind, GNOME session manager and Inhibit portal
//! services on a private bus.

use std::collections::HashMap;
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, round_trip, serve};
use eileen_dbus_linux::{Names, OwnerChanges, owner_changes};
use eileen_shell_linux::{DbusError, DesktopHolder, LinuxPowerRequest};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// The reason the tests give.
const WHY: &str = "An armed test run is in progress";

const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const GNOME: &str = "org.gnome.SessionManager";
const GNOME_PATH: &str = "/org/gnome/SessionManager";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";
/// The request handle a portal before 0.9 names, whatever the token.
const OLD_REQUEST_PATH: &str = "/org/freedesktop/portal/desktop/request/old/1";
/// The cookie the fake GNOME session manager hands out.
const COOKIE: u32 = 7;
/// The deadline for a portal that answers: far longer than a private bus needs.
const WAIT: Duration = Duration::from_secs(30);
/// The deadline a silent portal passes, in tokio's paused time.
const DEADLINE: Duration = Duration::from_secs(5);
/// What the errors name.
const PORTAL_WHAT: &str = "Inhibit portal";
const LOCK_WHAT: &str = "logind inhibitor lock";

/// The part of `connection`'s unique name a portal request's handle carries: no `:`, and `_`
/// for each `.`.
fn sender_part(connection: &zbus::Connection) -> String {
    connection.unique_name().unwrap().as_str().trim_start_matches(':').replace('.', "_")
}

/// One `Inhibit` call to the fake logind: what, who, why and mode.
type InhibitCall = (String, String, String, String);

/// A fake logind manager that records each `Inhibit` call and keeps logind's end of each lock,
/// or refuses as polkit does when `refuse` is set.
#[derive(Debug, Default, Clone)]
struct FakeManager {
    refuse: bool,
    calls: Arc<Mutex<Vec<InhibitCall>>>,
    /// logind's ends: each reads end of file once every copy of the caller's end has closed.
    watches: Arc<Mutex<Vec<UnixStream>>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl FakeManager {
    fn inhibit(
        &self,
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedFd> {
        let call = (what.to_owned(), who.to_owned(), why.to_owned(), mode.to_owned());
        self.calls.lock().unwrap().push(call);
        if self.refuse {
            return Err(zbus::fdo::Error::AccessDenied("Permission denied".to_owned()));
        }
        let (held, watch) = UnixStream::pair().unwrap();
        self.watches.lock().unwrap().push(watch);
        Ok(OwnedFd::from(held).into())
    }
}

/// A logind whose `Inhibit` answers a number where the descriptor belongs: how many calls it
/// has had.
#[derive(Debug, Default, Clone)]
struct NumberManager {
    calls: Arc<Mutex<Vec<InhibitCall>>>,
}

#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl NumberManager {
    fn inhibit(&self, what: &str, who: &str, why: &str, mode: &str) -> u32 {
        let mut calls = self.calls.lock().unwrap();
        calls.push((what.to_owned(), who.to_owned(), why.to_owned(), mode.to_owned()));
        u32::try_from(calls.len()).unwrap()
    }
}

/// One `Inhibit` call to the fake GNOME session manager.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GnomeCall {
    /// The caller's unique bus name.
    sender: String,
    app_id: String,
    toplevel_xid: u32,
    reason: String,
    flags: u32,
}

/// A fake GNOME session manager that records its inhibitors and their ends, or refuses as during
/// a forced logout when `refuse` is set.
#[derive(Debug, Default, Clone)]
struct FakeGnome {
    refuse: bool,
    calls: Arc<Mutex<Vec<GnomeCall>>>,
    uninhibits: Arc<Mutex<Vec<u32>>>,
}

#[zbus::interface(name = "org.gnome.SessionManager")]
impl FakeGnome {
    #[expect(clippy::needless_pass_by_value, reason = "zbus hands the header over by value")]
    fn inhibit(
        &self,
        #[zbus(header)] header: Header<'_>,
        app_id: &str,
        toplevel_xid: u32,
        reason: &str,
        flags: u32,
    ) -> zbus::fdo::Result<u32> {
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        let call = GnomeCall {
            sender,
            app_id: app_id.to_owned(),
            toplevel_xid,
            reason: reason.to_owned(),
            flags,
        };
        self.calls.lock().unwrap().push(call);
        if self.refuse {
            return Err(zbus::fdo::Error::Failed("Forced logout cannot be inhibited".to_owned()));
        }
        Ok(COOKIE)
    }

    fn uninhibit(&self, inhibit_cookie: u32) {
        self.uninhibits.lock().unwrap().push(inhibit_cookie);
    }
}

/// The portal's own error names.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The desktop's lockdown or the permission store turned the portal off.
    NotAllowed(String),
}

/// What the fake portal does with an `Inhibit` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Emits `Response` with this code before it replies, as a quick portal can.
    Now(u32),
    /// Replies and stays silent: the test answers after the reply, or lets the deadline pass.
    Later,
    /// Replies with [`OLD_REQUEST_PATH`], as a portal before 0.9 does, and stays silent.
    OldPath,
    /// Emits `Response` 0 before it replies, but serves no request object, so closing it fails.
    Unserved,
    /// Refuses the call with `org.freedesktop.portal.Error.NotAllowed`.
    NotAllowed,
}

/// One `Inhibit` call to the fake portal.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PortalCall {
    window: String,
    flags: u32,
    /// The options' names, sorted.
    options: Vec<String>,
    reason: Option<String>,
    token: Option<String>,
    /// The request the fake made.
    request: OwnedObjectPath,
}

/// A fake Inhibit portal. It makes each request at the handle the real portal gives it, from the
/// caller's unique name and its `handle_token`, serves a [`FakeRequest`] there and answers as
/// `answer` says.
#[derive(Debug, Clone)]
struct FakePortal {
    answer: Answer,
    calls: mpsc::Sender<PortalCall>,
    on_close: mpsc::Sender<OwnedObjectPath>,
}

#[zbus::interface(name = "org.freedesktop.portal.Inhibit")]
impl FakePortal {
    async fn inhibit(
        &self,
        window: &str,
        flags: u32,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        let text = |key: &str| options.get(key).map(|value| <&str>::try_from(value).unwrap());
        let token = text("handle_token").map(str::to_owned);
        let request = if self.answer == Answer::OldPath {
            OwnedObjectPath::try_from(OLD_REQUEST_PATH).unwrap()
        } else {
            let sender = header.sender().unwrap().trim_start_matches(':').replace('.', "_");
            let last = token.as_deref().unwrap_or("t");
            OwnedObjectPath::try_from(format!("{PORTAL_PATH}/request/{sender}/{last}")).unwrap()
        };
        let mut names: Vec<String> = options.keys().cloned().collect();
        names.sort();
        let call = PortalCall {
            window: window.to_owned(),
            flags,
            options: names,
            reason: text("reason").map(str::to_owned),
            token,
            request: request.clone(),
        };
        let _ignored = self.calls.send(call).await;
        if self.answer == Answer::NotAllowed {
            return Err(PortalError::NotAllowed("Inhibit is disabled".to_owned()));
        }
        if self.answer != Answer::Unserved {
            let object = FakeRequest { path: request.clone(), on_close: self.on_close.clone() };
            connection.object_server().at(&request, object).await?;
        }
        match self.answer {
            Answer::Now(code) => respond(connection, &request, code).await,
            Answer::Unserved => respond(connection, &request, 0).await,
            Answer::Later | Answer::OldPath | Answer::NotAllowed => {}
        }
        Ok(request)
    }
}

/// The portal's request object, which reports its closes.
#[derive(Debug, Clone)]
struct FakeRequest {
    path: OwnedObjectPath,
    on_close: mpsc::Sender<OwnedObjectPath>,
}

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl FakeRequest {
    async fn close(&self) {
        let _ignored = self.on_close.send(self.path.clone()).await;
    }
}

/// A fake portal on the bus: its connection, which the test holds, the calls it saw and the
/// requests that were closed.
struct Portal {
    service: zbus::Connection,
    seen: mpsc::Receiver<PortalCall>,
    closed: mpsc::Receiver<OwnedObjectPath>,
}

impl Portal {
    /// Serves a [`FakePortal`] that answers as `answer` says.
    async fn start(bus: &PrivateBus, answer: Answer) -> Self {
        let (calls, seen) = mpsc::channel(16);
        let (on_close, closed) = mpsc::channel(16);
        let service = serve(bus, PORTAL, PORTAL_PATH, FakePortal { answer, calls, on_close }).await;
        Self { service, seen, closed }
    }

    /// The next call the portal sees.
    async fn call(&mut self) -> PortalCall {
        self.seen.recv().await.expect("the portal stays up")
    }

    /// The calls the portal saw since the last look.
    fn calls(&mut self) -> Vec<PortalCall> {
        std::iter::from_fn(|| self.seen.try_recv().ok()).collect()
    }

    /// Answers the next call with `code` once the fake has handled it, as the real portal
    /// answers after its reply.
    async fn answer(&mut self, code: u32) -> PortalCall {
        let call = self.call().await;
        respond(&self.service, &call.request, code).await;
        call
    }

    /// The next request that is closed.
    async fn next_close(&mut self) -> OwnedObjectPath {
        self.closed.recv().await.expect("the portal stays up")
    }

    /// The requests closed since the last look.
    fn closes(&mut self) -> Vec<OwnedObjectPath> {
        std::iter::from_fn(|| self.closed.try_recv().ok()).collect()
    }
}

/// Emits the portal's `Response` with `code` on `request` from `connection`, to every connection
/// whose match rule takes it.
async fn respond(connection: &zbus::Connection, request: &OwnedObjectPath, code: u32) {
    respond_to(connection, None, request, code).await;
}

/// Emits `Response` with `code` on `request` from `connection`: broadcast without a
/// `destination`, else addressed to that connection, which the bus delivers whatever its match
/// rules say.
async fn respond_to(
    connection: &zbus::Connection,
    destination: Option<&str>,
    request: &OwnedObjectPath,
    code: u32,
) {
    let results: HashMap<String, OwnedValue> = HashMap::new();
    connection
        .emit_signal(destination, request, REQUEST, "Response", &(code, results))
        .await
        .unwrap();
}

/// Serves `manager` as logind.
async fn serve_logind(bus: &PrivateBus, manager: FakeManager) -> zbus::Connection {
    serve(bus, LOGIND, MANAGER_PATH, manager).await
}

/// Serves `gnome` as GNOME's session manager.
async fn serve_gnome(bus: &PrivateBus, gnome: FakeGnome) -> zbus::Connection {
    serve(bus, GNOME, GNOME_PATH, gnome).await
}

/// The owner changes of `connection`'s unique name, as `observer` sees them from now on.
async fn watch_leaving(observer: &zbus::Connection, connection: &zbus::Connection) -> OwnerChanges {
    let name = connection.unique_name().unwrap().to_string();
    owner_changes(observer, Names::Name(&name)).await.unwrap()
}

/// Waits until the watched connection has left the bus: its name has no owner any more.
async fn left(changes: &mut OwnerChanges) -> bool {
    changes.next().await.is_some_and(|change| change.new.is_none())
}

/// logind's end of the fake's latest lock, which reads without waiting.
fn watch_end(manager: &FakeManager) -> UnixStream {
    let watch = manager.watches.lock().unwrap().pop().unwrap();
    watch.set_nonblocking(true).unwrap();
    watch
}

/// Whether the caller holds the lock now: logind's end has nothing to read, not even its end.
fn still_held(watch: &mut UnixStream) -> bool {
    let mut byte = [0_u8; 1];
    matches!(watch.read(&mut byte), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
}

/// Waits until every copy of the caller's end has closed: logind's end reads end of file.
async fn released(watch: UnixStream) -> bool {
    let mut watch = tokio::net::UnixStream::from_std(watch).unwrap();
    let mut byte = [0_u8; 1];
    matches!(watch.read(&mut byte).await, Ok(0))
}

#[tokio::test]
async fn logind_holds_sleep_until_the_request_drops() {
    let bus = PrivateBus::start();
    let manager = FakeManager::default();
    let _logind = serve_logind(&bus, manager.clone()).await;
    let system = bus.connect().await;
    let request = LinuxPowerRequest::keep_awake(Some(&system), None, WHY, WAIT).await.unwrap();
    assert!(request.holds_sleep(), "logind's lock");
    assert_eq!(request.desktop(), None, "no session bus, so no desktop hold");
    assert_eq!(
        request.missed(),
        [DbusError::Unavailable { what: "session bus" }],
        "the missing desktop hold is named"
    );
    let expected =
        ("sleep:idle".to_owned(), "Eileen".to_owned(), WHY.to_owned(), "block".to_owned());
    assert_eq!(*manager.calls.lock().unwrap(), [expected], "a blocking lock on sleep and idle");
    let mut watch = watch_end(&manager);
    assert!(still_held(&mut watch), "the lock holds while the request lives");
    drop(request);
    assert!(released(watch).await, "dropping the request closes the lock's descriptor");
}

#[tokio::test]
async fn release_ends_loginds_lock_and_gnomes_inhibitor() {
    let bus = PrivateBus::start();
    let manager = FakeManager::default();
    let gnome = FakeGnome::default();
    let _logind = serve_logind(&bus, manager.clone()).await;
    let _gnome = serve_gnome(&bus, gnome.clone()).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert!(request.holds_sleep(), "logind's lock");
    assert_eq!(request.desktop(), Some(DesktopHolder::GnomeSession), "GNOME's inhibitor");
    assert!(request.missed().is_empty(), "nothing missed: {:?}", request.missed());
    let call = gnome.calls.lock().unwrap().first().cloned().unwrap();
    assert_eq!(
        (call.app_id.as_str(), call.toplevel_xid, call.reason.as_str(), call.flags),
        ("Eileen", 0, WHY, 12),
        "suspend and idle, with no window"
    );
    let watch = watch_end(&manager);
    request.release().await.unwrap();
    assert_eq!(*gnome.uninhibits.lock().unwrap(), [COOKIE], "uninhibited by its cookie");
    assert!(released(watch).await, "logind's lock is closed too");
}

#[tokio::test]
async fn the_portal_holds_suspend_and_idle_even_while_logind_holds() {
    let bus = PrivateBus::start();
    let _logind = serve_logind(&bus, FakeManager::default()).await;
    let mut portal = Portal::start(&bus, Answer::Now(0)).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let sender = sender_part(&session);
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert!(request.holds_sleep(), "logind's lock");
    assert_eq!(request.desktop(), Some(DesktopHolder::Portal), "the portal's inhibition");
    assert!(request.missed().is_empty(), "GNOME's absence is no miss when the portal holds");
    let calls = portal.calls();
    let [call] = calls.as_slice() else { panic!("one Inhibit call: {calls:?}") };
    assert_eq!(
        (call.window.as_str(), call.flags, call.reason.as_deref()),
        ("", 12, Some(WHY)),
        "suspend and idle though logind holds, since before systemd 257 logind lets the user's \
         own suspend through; no window; the reason as an option"
    );
    assert_eq!(call.options, ["handle_token", "reason"], "no other option");
    let token = call.token.as_deref().unwrap();
    assert!(token.starts_with("eileen_"), "Eileen's prefix: {token}");
    assert_eq!(
        call.request.as_str(),
        format!("{PORTAL_PATH}/request/{sender}/{token}"),
        "the handle the token names, which the caller had subscribed on before the early Response"
    );
    request.release().await.unwrap();
    assert_eq!(portal.closes(), std::slice::from_ref(&call.request), "the request was closed");
}

#[tokio::test]
async fn a_refusal_after_the_reply_is_asked_again_for_idle_alone() {
    let bus = PrivateBus::start();
    let mut portal = Portal::start(&bus, Answer::Later).await;
    let session = bus.connect().await;
    // As xdg-desktop-portal-gtk does without GNOME's session manager: the portal replies first,
    // and then its backend refuses anything but idle.
    let (request, (first, second)) =
        tokio::join!(LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT), async {
            (portal.answer(2).await, portal.answer(0).await)
        });
    assert_eq!((first.flags, second.flags), (12, 8), "suspend and idle first, then idle alone");
    assert_ne!(first.request, second.request, "each request on a handle of its own");
    let request = request.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::Portal), "idle is held");
    assert_eq!(
        request.missed(),
        [DbusError::Unavailable { what: "system bus" }],
        "only the system bus's absence is named"
    );
    assert!(portal.calls().is_empty(), "two calls, and no third");
    request.release().await.unwrap();
    assert_eq!(portal.closes(), [second.request], "the request that holds was closed");
}

#[tokio::test]
async fn a_backend_that_refuses_after_the_reply_holds_nothing() {
    let bus = PrivateBus::start();
    let mut portal = Portal::start(&bus, Answer::Later).await;
    let refused = DbusError::Refused { what: PORTAL_WHAT, reason: "its backend failed" };
    let session = bus.connect().await;
    let (error, ()) =
        tokio::join!(LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT), async {
            portal.answer(2).await;
            portal.answer(2).await;
        });
    assert_eq!(error.unwrap_err(), refused, "nothing holds, so the portal's refusal is the error");
    let _logind = serve_logind(&bus, FakeManager::default()).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let (request, ()) = tokio::join!(
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT),
        async {
            portal.answer(2).await;
            portal.answer(2).await;
        }
    );
    let request = request.unwrap();
    assert!(request.holds_sleep(), "logind's lock");
    assert_eq!(request.desktop(), None, "the desktop holds nothing");
    assert_eq!(request.missed(), [refused], "and the portal's refusal is named");
}

#[tokio::test(start_paused = true)]
async fn a_portal_that_never_answers_holds_nothing_past_the_deadline() {
    let bus = PrivateBus::start();
    let _logind = serve_logind(&bus, FakeManager::default()).await;
    // As when the permission store masks every flag: the portal replies and never answers.
    let mut portal = Portal::start(&bus, Answer::Later).await;
    let unconfirmed = DbusError::DeadlineExceeded { call: "portal response" };
    let system = bus.connect().await;
    let session = bus.connect().await;
    let observer = bus.connect().await;
    let mut leaving = watch_leaving(&observer, &session).await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, DEADLINE).await.unwrap();
    assert!(request.holds_sleep(), "logind's lock");
    assert_eq!(request.desktop(), None, "an unconfirmed request is no hold");
    assert_eq!(
        request.missed(),
        std::slice::from_ref(&unconfirmed),
        "the portal passed the deadline"
    );
    let calls = portal.calls();
    let [call] = calls.as_slice() else { panic!("asked once, since silence is no refusal") };
    assert_eq!(portal.next_close().await, call.request, "the unconfirmed request was closed");
    assert!(left(&mut leaving).await, "and the session connection went with the missed hold");
    let session = bus.connect().await;
    let error =
        LinuxPowerRequest::keep_awake(None, Some(session), WHY, DEADLINE).await.unwrap_err();
    assert_eq!(error, unconfirmed, "with nothing else holding, the deadline is the error");
}

#[tokio::test]
async fn only_the_portals_own_response_counts() {
    let bus = PrivateBus::start();
    let mut portal = Portal::start(&bus, Answer::Later).await;
    let stranger = bus.connect().await;
    let session = bus.connect().await;
    let caller = session.unique_name().unwrap().to_string();
    let (error, ()) =
        tokio::join!(LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT), async {
            let call = portal.call().await;
            // A broadcast, which the bus's match rule keeps from the caller, and a signal
            // addressed to the caller's connection, which the bus delivers whatever the rule
            // says. The round trip puts both before the portal's own answer.
            respond_to(&stranger, None, &call.request, 0).await;
            respond_to(&stranger, Some(&caller), &call.request, 0).await;
            round_trip(&stranger, None).await;
            respond(&portal.service, &call.request, 1).await;
        });
    assert_eq!(
        error.unwrap_err(),
        DbusError::Refused { what: PORTAL_WHAT, reason: "cancelled by the user" },
        "the stranger's 0 did not count, and the portal's own 1 did"
    );
    assert!(portal.calls().is_empty(), "a cancellation is not asked again");
}

#[tokio::test]
async fn the_request_path_an_older_portal_names_is_followed() {
    let bus = PrivateBus::start();
    let mut portal = Portal::start(&bus, Answer::OldPath).await;
    let old = OwnedObjectPath::try_from(OLD_REQUEST_PATH).unwrap();
    let session = bus.connect().await;
    let mut pending = pin!(LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT));
    // The caller subscribes on the portal's own handle only once the reply has come, so the test
    // answers there until the caller hears it, letting it run between answers. No timer is
    // involved: the loop ends as soon as the caller has subscribed.
    let request = loop {
        if let Poll::Ready(request) =
            std::future::poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx))).await
        {
            break request;
        }
        respond(&portal.service, &old, 0).await;
        tokio::task::yield_now().await;
    };
    let request = request.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::Portal), "held on the portal's own handle");
    request.release().await.unwrap();
    assert_eq!(portal.closes(), [old], "and closed there");
}

#[tokio::test]
async fn gnomes_session_manager_comes_before_the_portal() {
    let bus = PrivateBus::start();
    let gnome = FakeGnome::default();
    let _gnome = serve_gnome(&bus, gnome.clone()).await;
    let mut portal = Portal::start(&bus, Answer::Now(0)).await;
    let session = bus.connect().await;
    let request = LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::GnomeSession), "GNOME's inhibitor");
    assert_eq!(gnome.calls.lock().unwrap().len(), 1, "GNOME was asked");
    assert!(portal.calls().is_empty(), "the portal was not");
}

#[tokio::test]
async fn a_portal_without_the_inhibit_interface_counts_as_absent() {
    let bus = PrivateBus::start();
    // The portal's name is owned, but nothing serves `Inhibit`: a portal with no backend for it.
    // It serves one other object, so that its object server answers the call.
    let (on_close, _closed) = mpsc::channel(1);
    let old = OwnedObjectPath::try_from(OLD_REQUEST_PATH).unwrap();
    let _portal = serve(&bus, PORTAL, OLD_REQUEST_PATH, FakeRequest { path: old, on_close }).await;
    let nothing = DbusError::Unavailable { what: "keep-awake holder" };
    let session = bus.connect().await;
    let error = LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT).await.unwrap_err();
    assert_eq!(error, nothing, "no GNOME and no Inhibit: nothing holds, and nothing refused");
    let _logind = serve_logind(&bus, FakeManager::default()).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(
        request.missed(),
        [DbusError::NotFound { what: PORTAL_WHAT }],
        "with GNOME's session manager absent, the desktop's miss is the portal's"
    );
}

#[tokio::test]
async fn a_portal_the_desktop_does_not_allow_is_a_refusal() {
    let bus = PrivateBus::start();
    let _logind = serve_logind(&bus, FakeManager::default()).await;
    let mut portal = Portal::start(&bus, Answer::NotAllowed).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), None, "the desktop holds nothing");
    assert_eq!(
        request.missed(),
        [DbusError::Refused { what: PORTAL_WHAT, reason: "not allowed" }],
        "the portal's NotAllowed is a refusal, not a malformed answer"
    );
    assert_eq!(portal.calls().len(), 1, "a refused call is not asked again for idle alone");
}

#[tokio::test]
async fn release_reports_a_desktop_hold_it_could_not_end_and_still_closes_the_lock() {
    let bus = PrivateBus::start();
    let manager = FakeManager::default();
    let _logind = serve_logind(&bus, manager.clone()).await;
    // The portal hands out a request it does not serve, so closing it fails.
    let _portal = Portal::start(&bus, Answer::Unserved).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::Portal), "the portal's inhibition");
    let watch = watch_end(&manager);
    let error = request.release().await.unwrap_err();
    assert_eq!(error, DbusError::NotFound { what: PORTAL_WHAT }, "no such request");
    assert!(released(watch).await, "logind's lock is closed all the same");
}

#[tokio::test]
async fn dropping_the_request_leaves_the_bus_so_the_desktop_ends_its_hold() {
    let bus = PrivateBus::start();
    let gnome = FakeGnome::default();
    let _gnome = serve_gnome(&bus, gnome.clone()).await;
    let session = bus.connect().await;
    let caller = session.unique_name().unwrap().to_string();
    let observer = bus.connect().await;
    let mut leaving = watch_leaving(&observer, &session).await;
    let request = LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::GnomeSession), "GNOME's inhibitor");
    let sender = gnome.calls.lock().unwrap().first().unwrap().sender.clone();
    assert_eq!(sender, caller, "the hold was taken on the connection the request keeps");
    drop(request);
    assert!(
        left(&mut leaving).await,
        "the caller left the bus, which ends GNOME's inhibitor and the portal's request"
    );
    assert!(gnome.uninhibits.lock().unwrap().is_empty(), "no Uninhibit: leaving the bus ends it");
}

#[tokio::test]
async fn a_request_dropped_on_a_plain_thread_still_releases() {
    // The trait's PowerRequest runs its release closure wherever the request drops, outside the
    // tokio runtime too: the request must hold nothing whose drop needs the runtime.
    let bus = PrivateBus::start();
    let manager = FakeManager::default();
    let _logind = serve_logind(&bus, manager.clone()).await;
    let _gnome = serve_gnome(&bus, FakeGnome::default()).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let observer = bus.connect().await;
    let mut leaving = watch_leaving(&observer, &session).await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::GnomeSession), "GNOME's inhibitor");
    let watch = watch_end(&manager);
    std::thread::scope(|scope| {
        let dropped = scope.spawn(move || drop(request));
        assert!(dropped.join().is_ok(), "dropping it off the runtime does not panic");
    });
    assert!(released(watch).await, "logind's lock is closed");
    assert!(left(&mut leaving).await, "and the session connection left the bus");
}

#[tokio::test]
async fn with_no_holder_there_is_nothing_to_keep_awake() {
    let bus = PrivateBus::start();
    let system = bus.connect().await;
    let nothing = DbusError::Unavailable { what: "keep-awake holder" };
    let session = bus.connect().await;
    let error =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap_err();
    assert_eq!(error, nothing, "no logind, no GNOME, no portal");
    let error = LinuxPowerRequest::keep_awake(None, None, WHY, WAIT).await.unwrap_err();
    assert_eq!(error, nothing, "no buses");
    let error = LinuxPowerRequest::keep_awake(Some(&system), None, " ", WAIT).await.unwrap_err();
    assert_eq!(
        error,
        DbusError::Invalid { what: "keep-awake reason", detail: "empty".to_owned() },
        "a blank reason"
    );
    assert_eq!(error.to_string(), "invalid keep-awake reason: empty", "named as a bare noun");
}

#[tokio::test]
async fn a_refusal_is_the_error_when_nothing_else_holds() {
    let bus = PrivateBus::start();
    let manager = FakeManager { refuse: true, ..FakeManager::default() };
    let _logind = serve_logind(&bus, manager.clone()).await;
    let system = bus.connect().await;
    let refused = DbusError::Refused { what: LOCK_WHAT, reason: "not authorized" };
    let error = LinuxPowerRequest::keep_awake(Some(&system), None, WHY, WAIT).await.unwrap_err();
    assert_eq!(error, refused, "polkit said no");
    assert_eq!(manager.calls.lock().unwrap().len(), 1, "logind was asked");
    let _gnome = serve_gnome(&bus, FakeGnome::default()).await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::GnomeSession), "the desktop still holds");
    assert_eq!(request.missed(), [refused], "and logind's refusal is named");
}

#[tokio::test]
async fn a_lock_reply_without_a_descriptor_is_invalid() {
    let bus = PrivateBus::start();
    let manager = NumberManager::default();
    let _logind = serve(&bus, LOGIND, MANAGER_PATH, manager.clone()).await;
    let gnome = FakeGnome::default();
    let _gnome = serve_gnome(&bus, gnome.clone()).await;
    let system = bus.connect().await;
    let session = bus.connect().await;
    let request =
        LinuxPowerRequest::keep_awake(Some(&system), Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(manager.calls.lock().unwrap().len(), 1, "logind answered once");
    assert!(!request.holds_sleep(), "no descriptor, no lock");
    assert!(
        matches!(request.missed(), [DbusError::Invalid { what: LOCK_WHAT, .. }]),
        "the malformed reply is named: {:?}",
        request.missed()
    );
    let flags: Vec<u32> = gnome.calls.lock().unwrap().iter().map(|call| call.flags).collect();
    assert_eq!(flags, [12], "GNOME is asked for suspend and idle either way");
}

#[tokio::test]
async fn the_portal_stands_in_when_gnomes_session_manager_refuses() {
    let bus = PrivateBus::start();
    let _gnome = serve_gnome(&bus, FakeGnome { refuse: true, ..FakeGnome::default() }).await;
    let gnome_refused = DbusError::Invalid {
        what: "GNOME session manager",
        detail: "org.freedesktop.DBus.Error.Failed".to_owned(),
    };
    let session = bus.connect().await;
    let error = LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT).await.unwrap_err();
    assert_eq!(error, gnome_refused, "without the portal, GNOME's refusal is the error");
    let _portal = Portal::start(&bus, Answer::Now(0)).await;
    let session = bus.connect().await;
    let request = LinuxPowerRequest::keep_awake(None, Some(session), WHY, WAIT).await.unwrap();
    assert_eq!(request.desktop(), Some(DesktopHolder::Portal), "the portal holds instead");
}

#[test]
fn a_request_can_move_into_the_traits_release_closure() {
    fn release_closure<T: Send + 'static>() {}
    release_closure::<LinuxPowerRequest>();
}

#[test]
fn keeping_awake_can_move_to_another_thread_with_its_deadline() {
    fn on_any_worker<F: Send>(_future: F) {}
    on_any_worker(LinuxPowerRequest::keep_awake(None, None, WHY, WAIT));
}

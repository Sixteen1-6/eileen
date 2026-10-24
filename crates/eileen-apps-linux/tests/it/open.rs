//! The opener against fakes on the private bus: an `OpenURI` portal that records each call and
//! answers on the request handle its `handle_token` names, as the real portal does, and
//! D-Bus-activatable apps. The `apps` tests reuse the fakes through the trait.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_apps_linux::{
    AppEntry, AppKind, DbusError, DesktopEntry, FileError, OpenError, Opener, Session, entry_for,
    index,
};
use eileen_contracts::ids::Personal;
use eileen_dbus_linux::testing::{PrivateBus, round_trip, serve};
use rustix::fs::{CWD, Mode as Permissions, OFlags, fcntl_getfl, mkfifoat};
use tokio::sync::mpsc;
use zbus::message::Header;
use zbus::zvariant::{OwnedFd, OwnedObjectPath, OwnedValue};

/// A deadline no answer on a private bus comes near.
const DEADLINE: Duration = Duration::from_secs(30);
/// The portal's names.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// What the fake portal does with a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Emits `Response` with this code before it replies, as a quick portal can.
    Now(u32),
    /// Replies and stays silent: the test answers, or lets the deadline pass.
    Later,
    /// Refuses, as the portal does when the desktop's lockdown turns handlers off.
    NotAllowed,
}

/// What the fake portal saw.
#[derive(Debug)]
pub(crate) enum Event {
    /// A call to the portal.
    Call(PortalCall),
    /// `Close` on a request.
    Closed(OwnedObjectPath),
}

/// One call to the portal.
#[derive(Debug)]
pub(crate) struct PortalCall {
    pub(crate) method: &'static str,
    parent_window: String,
    /// The URI, or the path the descriptor names.
    pub(crate) target: String,
    /// How the descriptor is open; `None` for a URI.
    mode: Option<Mode>,
    /// The options' names.
    options: Vec<String>,
    /// The `handle_token` option.
    token: Option<String>,
    /// The handle the fake answered.
    pub(crate) handle: OwnedObjectPath,
}

/// The portal's errors the fake answers.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotAllowed(String),
}

/// The fake `OpenURI` portal.
struct FakePortal {
    answer: Answer,
    events: mpsc::Sender<Event>,
}

impl FakePortal {
    /// Answers a call as the real portal does: a request at the handle from the caller's unique
    /// name and its `handle_token`, and a `Response` on it.
    async fn answer(
        &self,
        mut call: PortalCall,
        options: &HashMap<String, OwnedValue>,
        header: &Header<'_>,
        connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        if self.answer == Answer::NotAllowed {
            return Err(PortalError::NotAllowed("Application handlers disabled".to_owned()));
        }
        let mut names: Vec<String> = options.keys().cloned().collect();
        names.sort();
        call.options = names;
        call.token = options
            .get("handle_token")
            .and_then(|value| <&str>::try_from(value).ok())
            .map(str::to_owned);
        let sender = header.sender().unwrap().as_str().trim_start_matches(':').replace('.', "_");
        let token = call.token.clone().unwrap_or_default();
        let handle =
            OwnedObjectPath::try_from(format!("{PORTAL_PATH}/request/{sender}/{token}")).unwrap();
        connection.object_server().at(&handle, FakeRequest { events: self.events.clone() }).await?;
        call.handle = handle.clone();
        self.events.send(Event::Call(call)).await.unwrap();
        if let Answer::Now(code) = self.answer {
            respond(connection, handle.as_str(), code).await;
        }
        Ok(handle)
    }
}

#[zbus::interface(name = "org.freedesktop.portal.OpenURI")]
impl FakePortal {
    #[zbus(name = "OpenURI")]
    async fn open_uri(
        &self,
        parent_window: &str,
        uri: &str,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        let call = portal_call("OpenURI", parent_window, uri.to_owned(), None);
        self.answer(call, &options, &header, connection).await
    }

    async fn open_file(
        &self,
        parent_window: &str,
        fd: OwnedFd,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        let (target, mode) = describe(&fd);
        let call = portal_call("OpenFile", parent_window, target, Some(mode));
        self.answer(call, &options, &header, connection).await
    }

    async fn open_directory(
        &self,
        parent_window: &str,
        fd: OwnedFd,
        options: HashMap<String, OwnedValue>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, PortalError> {
        let (target, mode) = describe(&fd);
        let call = portal_call("OpenDirectory", parent_window, target, Some(mode));
        self.answer(call, &options, &header, connection).await
    }
}

/// A call before the fake has read its options and named its handle.
fn portal_call(
    method: &'static str,
    parent_window: &str,
    target: String,
    mode: Option<Mode>,
) -> PortalCall {
    PortalCall {
        method,
        parent_window: parent_window.to_owned(),
        target,
        mode,
        options: Vec::new(),
        token: None,
        handle: OwnedObjectPath::default(),
    }
}

/// How a descriptor is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mode {
    read_only: bool,
    /// Opened with `O_NONBLOCK`, so the open could not wait for a FIFO's writer.
    non_blocking: bool,
}

/// How the opener opens a file or folder for the portal.
const READ_ONLY_NON_BLOCKING: Mode = Mode { read_only: true, non_blocking: true };

/// The path a descriptor names, from `/proc/self/fd`, and how it is open.
fn describe(fd: &OwnedFd) -> (String, Mode) {
    let path = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_fd().as_raw_fd())).unwrap();
    let flags = fcntl_getfl(fd).unwrap();
    let mode = Mode {
        read_only: flags & OFlags::ACCMODE == OFlags::RDONLY,
        non_blocking: flags.contains(OFlags::NONBLOCK),
    };
    (path.to_string_lossy().into_owned(), mode)
}

/// The fake portal's request object, which records `Close`.
struct FakeRequest {
    events: mpsc::Sender<Event>,
}

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl FakeRequest {
    async fn close(&self, #[zbus(header)] header: Header<'_>) {
        let path = header.path().unwrap().to_owned();
        self.events.send(Event::Closed(path.into())).await.unwrap();
    }
}

/// A portal whose `OpenURI` answers a string where a request handle belongs.
struct OddPortal {
    reply: &'static str,
}

#[zbus::interface(name = "org.freedesktop.portal.OpenURI")]
impl OddPortal {
    #[zbus(name = "OpenURI")]
    fn open_uri(
        &self,
        parent_window: &str,
        uri: &str,
        options: HashMap<String, OwnedValue>,
    ) -> String {
        let options = options.into_keys().count();
        format!("{} ({parent_window:?}, {uri}, {options} options)", self.reply)
    }
}

/// The fake portal on `bus`, and what it sees. Hold the connection for the whole test.
pub(crate) async fn portal(
    bus: &PrivateBus,
    answer: Answer,
) -> (zbus::Connection, mpsc::Receiver<Event>) {
    let (events, seen) = mpsc::channel(16);
    (serve(bus, PORTAL, PORTAL_PATH, FakePortal { answer, events }).await, seen)
}

/// Emits the portal's `Response` with `code` on `handle` from `connection`, to every connection
/// whose match rule takes it.
async fn respond(connection: &zbus::Connection, handle: &str, code: u32) {
    respond_to(connection, None, handle, code).await;
}

/// Emits `Response` with `code` on `handle` from `connection`: broadcast without a
/// `destination`, else addressed to that connection, which the bus delivers whatever its match
/// rules say.
async fn respond_to(
    connection: &zbus::Connection,
    destination: Option<&str>,
    handle: &str,
    code: u32,
) {
    let results: HashMap<String, OwnedValue> = HashMap::new();
    connection
        .emit_signal(destination, handle, REQUEST, "Response", &(code, results))
        .await
        .unwrap();
}

/// What the fake saw next. No timer: nextest's slow-timeout ends a test that waits for good.
pub(crate) async fn next(seen: &mut mpsc::Receiver<Event>) -> Event {
    seen.recv().await.expect("the fake portal is still on the bus")
}

/// The next call the fake portal saw.
pub(crate) async fn call_seen(seen: &mut mpsc::Receiver<Event>) -> PortalCall {
    match next(seen).await {
        Event::Call(call) => call,
        other @ Event::Closed(_) => panic!("not a call: {other:?}"),
    }
}

/// A fresh folder of the temporary directory, removed on drop. The recent files' tests and the
/// index's use it too.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-open-{}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A file in the folder.
    pub(crate) fn file(&self, name: &str) -> PathBuf {
        self.write(name, "a fixture document")
    }

    /// A file in the folder that holds `contents`.
    pub(crate) fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// A FIFO in the folder, which nothing writes to.
    pub(crate) fn fifo(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        mkfifoat(CWD, &path, Permissions::RUSR | Permissions::WUSR).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// A runtime whose blocking pool has one thread. tokio runs the opener's file checks there.
fn one_blocking_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
}

/// Takes the blocking pool's one thread until the returned sender drops, so a file check queued
/// behind it waits, as a stat on a FUSE or network mount whose server hangs does. Call it in a
/// runtime from [`one_blocking_thread`]; a test that panics drops the sender as it unwinds.
fn stall_file_checks() -> tokio::sync::oneshot::Sender<()> {
    let (release, released) = tokio::sync::oneshot::channel();
    let _stall = tokio::runtime::Handle::current().spawn_blocking(move || released.blocking_recv());
    release
}

/// The test app's names: its well-known name, its object path, and its desktop file ID.
const APP: &str = "org.eileen.TestApp";
const APP_PATH: &str = "/org/eileen/TestApp";
const APP_ID: &str = "org.eileen.TestApp.desktop";

/// What a fake app does with `Activate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Behaviour {
    /// Answers at once.
    Answer,
    /// Records the call, then never answers.
    Hang,
    /// Fails as the bus answers for a service whose process exited as it started.
    ExitAtStart,
}

/// One call to a fake app's `org.freedesktop.Application`.
#[derive(Debug)]
pub(crate) struct AppCall {
    pub(crate) method: &'static str,
    pub(crate) uris: Vec<String>,
    platform_data: Vec<String>,
}

/// The error the bus answers when a service it started exited at once.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.DBus.Error.Spawn")]
enum SpawnError {
    #[zbus(error)]
    ZBus(zbus::Error),
    ChildExited(String),
}

/// A fake D-Bus-activatable app.
struct FakeApp {
    behaviour: Behaviour,
    calls: mpsc::Sender<AppCall>,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl FakeApp {
    async fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> Result<(), SpawnError> {
        if self.behaviour == Behaviour::ExitAtStart {
            return Err(SpawnError::ChildExited("exited with status 1".to_owned()));
        }
        let platform_data = platform_data.into_keys().collect();
        let call = AppCall { method: "Activate", uris: Vec::new(), platform_data };
        self.calls.send(call).await.unwrap();
        if self.behaviour == Behaviour::Hang {
            std::future::pending::<()>().await;
        }
        Ok(())
    }

    async fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedValue>) {
        let platform_data = platform_data.into_keys().collect();
        self.calls.send(AppCall { method: "Open", uris, platform_data }).await.unwrap();
    }
}

/// A fake app owning `name` on `bus` with its interface at `path`, and the calls it sees. Hold
/// the connection for the whole test.
pub(crate) async fn app(
    bus: &PrivateBus,
    name: &str,
    path: &str,
    behaviour: Behaviour,
) -> (zbus::Connection, mpsc::Receiver<AppCall>) {
    let (calls, seen) = mpsc::channel(16);
    (serve(bus, name, path, FakeApp { behaviour, calls }).await, seen)
}

/// The next call a fake app saw. No timer, as for the portal's.
pub(crate) async fn app_seen(seen: &mut mpsc::Receiver<AppCall>) -> AppCall {
    seen.recv().await.expect("the fake app is still on the bus")
}

/// A desktop entry with the desktop file ID `id`.
fn entry(id: &str, dbus_activatable: bool, kind: AppKind) -> DesktopEntry {
    DesktopEntry {
        id: id.to_owned(),
        name: "Test App".to_owned(),
        generic_name: None,
        keywords: Vec::new(),
        exec: Some(Personal::new("test-app %U".to_owned())),
        terminal: false,
        dbus_activatable,
        startup_wm_class: None,
        kind,
        path: Personal::new(PathBuf::from("/usr/share/applications").join(id)),
    }
}

#[tokio::test]
async fn a_web_address_opens_through_the_portal_on_the_predicted_handle() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
    let client = bus.connect().await;
    let sender = client.unique_name().unwrap().as_str().trim_start_matches(':').replace('.', "_");
    let opener = Opener::new(&client);
    let url = "https://www.youtube.com/watch?v=abc&t=200";
    assert_eq!(
        opener.open_url(url, DEADLINE).await,
        Ok(()),
        "the Response came before the reply, and the opener had already subscribed"
    );
    let call = call_seen(&mut seen).await;
    assert_eq!((call.method, call.target.as_str()), ("OpenURI", url), "the address as given");
    assert_eq!(call.parent_window, "", "no parent window");
    assert_eq!(call.options, ["handle_token"], "no other option");
    let token = call.token.unwrap();
    assert!(token.starts_with("eileen_"), "Eileen's prefix: {token}");
    assert_eq!(
        call.handle.as_str(),
        format!("{PORTAL_PATH}/request/{sender}/{token}"),
        "the handle the token names"
    );
}

#[tokio::test]
async fn a_response_after_the_reply_is_awaited() {
    let bus = PrivateBus::start();
    let (service, mut seen) = portal(&bus, Answer::Later).await;
    let opener = Opener::new(&bus.connect().await);
    let (outcome, ()) = tokio::join!(opener.open_uri("mailto:sam@example.com", DEADLINE), async {
        let call = call_seen(&mut seen).await;
        assert_eq!(call.target, "mailto:sam@example.com", "a URI other than a web address");
        respond(&service, call.handle.as_str(), 0).await;
    });
    assert_eq!(outcome, Ok(()), "the portal's later answer");
}

#[tokio::test]
async fn response_codes_say_what_happened() {
    let what = "OpenURI request";
    let cases = [
        (1, Err(DbusError::Refused { what, reason: "cancelled by the user" })),
        (2, Err(DbusError::Refused { what, reason: "not opened" })),
        (7, Err(DbusError::Invalid { what, detail: "response code 7".to_owned() })),
    ];
    for (code, expected) in cases {
        let bus = PrivateBus::start();
        let (_service, _seen) = portal(&bus, Answer::Now(code)).await;
        let opener = Opener::new(&bus.connect().await);
        assert_eq!(opener.open_url("https://example.com", DEADLINE).await, expected, "code {code}");
    }
}

#[tokio::test]
async fn a_reply_that_is_not_a_request_handle_is_invalid() {
    let bus = PrivateBus::start();
    let _service =
        serve(&bus, PORTAL, PORTAL_PATH, OddPortal { reply: "not a request handle" }).await;
    let opener = Opener::new(&bus.connect().await);
    let outcome = opener.open_url("https://example.com", DEADLINE).await;
    assert!(
        matches!(outcome, Err(DbusError::Invalid { what: "portal request", .. })),
        "a string where the handle belongs: {outcome:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_silent_portal_passes_the_deadline_and_the_request_is_closed() {
    // Paused time: the deadline is the only timer, and tokio fires it once the runtime is idle.
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Later).await;
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.open_url("https://example.com", DEADLINE).await,
        Err(DbusError::DeadlineExceeded { call: "portal response" }),
        "no Response within the deadline"
    );
    let call = call_seen(&mut seen).await;
    match next(&mut seen).await {
        Event::Closed(path) => assert_eq!(path, call.handle, "the pending request closed"),
        other @ Event::Call(_) => panic!("not a Close: {other:?}"),
    }
}

#[tokio::test]
async fn only_the_portal_can_answer_a_request() {
    let bus = PrivateBus::start();
    let (service, mut seen) = portal(&bus, Answer::Later).await;
    let stranger = bus.connect().await;
    let client = bus.connect().await;
    let opener_name = client.unique_name().unwrap().to_string();
    let opener = Opener::new(&client);
    // A broadcast, which the bus's match rule keeps from the opener, and a signal addressed to
    // the opener's connection, which the bus delivers whatever the rule says.
    for destination in [None, Some(opener_name.as_str())] {
        let (outcome, ()) = tokio::join!(opener.open_url("https://example.com", DEADLINE), async {
            let call = call_seen(&mut seen).await;
            respond_to(&stranger, destination, call.handle.as_str(), 0).await;
            // The bus has routed the stranger's signal before it answers the stranger.
            round_trip(&stranger, None).await;
            respond(&service, call.handle.as_str(), 1).await;
        });
        assert_eq!(
            outcome,
            Err(DbusError::Refused { what: "OpenURI request", reason: "cancelled by the user" }),
            "another client's success did not count, sent to {destination:?}"
        );
    }
}

#[tokio::test]
async fn the_desktops_lockdown_refuses() {
    let bus = PrivateBus::start();
    let (_service, _seen) = portal(&bus, Answer::NotAllowed).await;
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.open_url("https://example.com", DEADLINE).await,
        Err(DbusError::Refused { what: "OpenURI request", reason: "not allowed" }),
        "org.freedesktop.portal.Error.NotAllowed"
    );
}

#[tokio::test]
async fn schemes_are_checked_before_the_portal_is_asked() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
    let opener = Opener::new(&bus.connect().await);
    for url in ["ftp://example.com/file", "mailto:sam@example.com", "file:///etc/hosts", "https://"]
    {
        let outcome = opener.open_url(url, DEADLINE).await;
        assert!(
            matches!(outcome, Err(DbusError::Invalid { what: "web address", .. })),
            "{url}: {outcome:?}"
        );
    }
    for uri in [
        "https://example.com",
        "file:///home/sam/.ssh/id_ed25519",
        "FILE:///etc/hosts",
        "example.com/page",
        "mailto:sam@example.com\nBcc: someone",
    ] {
        let outcome = opener.open_uri(uri, DEADLINE).await;
        assert!(
            matches!(outcome, Err(DbusError::Invalid { what: "URI", .. })),
            "{uri}: {outcome:?}"
        );
        let text = outcome.unwrap_err().to_string();
        assert!(!text.contains("sam") && !text.contains("example"), "no URI in it: {text}");
    }
    // The fake records a call before it replies, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the portal was never asked");
}

#[tokio::test]
async fn a_file_opens_read_only_through_open_file() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
    let opener = Opener::new(&bus.connect().await);
    let scratch = Scratch::new();
    let file = scratch.file("Quarterly report #3.odt");
    assert_eq!(opener.open_path(&file, DEADLINE).await, Ok(()), "opened");
    let call = call_seen(&mut seen).await;
    assert_eq!(call.method, "OpenFile", "a file goes to OpenFile");
    assert_eq!(Path::new(&call.target), file, "the descriptor names the file");
    assert_eq!(call.mode, Some(READ_ONLY_NON_BLOCKING), "opened read-only, without waiting");
    assert_eq!(call.options, ["handle_token"], "not writable, no other option");
}

#[tokio::test]
async fn a_folder_opens_itself_through_open_file() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
    let opener = Opener::new(&bus.connect().await);
    let scratch = Scratch::new();
    assert_eq!(opener.open_path(&scratch.0, DEADLINE).await, Ok(()), "opened");
    let call = call_seen(&mut seen).await;
    assert_eq!(
        call.method, "OpenFile",
        "OpenFile opens the folder; OpenDirectory would show it in its parent"
    );
    assert_eq!(Path::new(&call.target), scratch.0, "the descriptor names the folder");
    assert_eq!(call.mode, Some(READ_ONLY_NON_BLOCKING), "opened read-only, without waiting");
}

#[tokio::test]
async fn paths_are_checked_before_the_portal_is_asked() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
    let opener = Opener::new(&bus.connect().await);
    let scratch = Scratch::new();
    let refused = |error| Err(OpenError::File(error));
    assert_eq!(
        opener.open_path(&scratch.0.join("gone.txt"), DEADLINE).await,
        refused(FileError::NotFound { what: "file to open" }),
        "a missing file"
    );
    assert_eq!(
        opener.open_path(Path::new("Documents/notes.txt"), DEADLINE).await,
        refused(FileError::Invalid { what: "path", detail: "not absolute".to_owned() }),
        "a relative path"
    );
    let not_a_file =
        refused(FileError::Invalid { what: "path", detail: "not a file or folder".to_owned() });
    assert_eq!(
        opener.open_path(Path::new("/dev/null"), DEADLINE).await,
        not_a_file,
        "a device is not a file or folder"
    );
    assert_eq!(
        opener.open_path(&scratch.fifo("pipe"), DEADLINE).await,
        not_a_file,
        "a FIFO is not a file or folder, and nothing waited for a writer"
    );
    // The fake records a call before it replies, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the portal was never asked");
}

#[test]
fn a_file_check_that_stalls_passes_the_deadline() {
    // A real timer: tokio's paused time stands still while a blocking task runs, and the checks
    // run on the blocking pool. Nothing is asserted about how long anything took.
    one_blocking_thread().block_on(async {
        let bus = PrivateBus::start();
        let (_service, mut seen) = portal(&bus, Answer::Now(0)).await;
        let opener = Opener::new(&bus.connect().await);
        let scratch = Scratch::new();
        let file = scratch.file("notes.txt");
        let release = stall_file_checks();
        assert_eq!(
            opener.open_path(&file, Duration::from_millis(50)).await,
            Err(OpenError::File(FileError::Io {
                what: "file to open",
                detail: "timed out".to_owned()
            })),
            "the checks still waited behind the stall at the deadline"
        );
        // open_path answered before any call to the portal.
        assert!(seen.try_recv().is_err(), "the portal was not asked");
        drop(release);
        assert_eq!(opener.open_path(&file, DEADLINE).await, Ok(()), "the same file, unstalled");
        assert_eq!(call_seen(&mut seen).await.method, "OpenFile", "asked once the checks ran");
    });
}

#[tokio::test]
async fn without_the_portal_opening_is_unavailable() {
    let bus = PrivateBus::start();
    let opener = Opener::new(&bus.connect().await);
    let unavailable = |what| Err(DbusError::Unavailable { what });
    assert_eq!(
        opener.open_url("https://example.com", DEADLINE).await,
        unavailable("OpenURI request"),
        "a web address"
    );
    assert_eq!(
        opener.open_uri("mailto:sam@example.com", DEADLINE).await,
        unavailable("OpenURI request"),
        "a URI"
    );
    let scratch = Scratch::new();
    assert_eq!(
        opener.open_path(&scratch.file("notes.txt"), DEADLINE).await,
        unavailable("OpenFile request").map_err(OpenError::Bus),
        "a file"
    );
}

#[tokio::test]
async fn an_activatable_app_is_activated_and_its_process_id_returned() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.launch(&entry(APP_ID, true, AppKind::Desktop)).await,
        Ok(std::process::id()),
        "the fake app answers from the test's own process"
    );
    let call = app_seen(&mut seen).await;
    assert_eq!(call.method, "Activate", "started without files");
    assert!(call.platform_data.is_empty(), "no platform data");
    assert_eq!(
        opener.launch(&entry(APP_ID, true, AppKind::Packaged)).await,
        Ok(std::process::id()),
        "a packaged app launches the same way"
    );
}

#[tokio::test]
async fn a_dash_in_the_name_is_an_underscore_in_the_path() {
    let bus = PrivateBus::start();
    let (_service, mut seen) =
        app(&bus, "org.eileen.Test-App", "/org/eileen/Test_App", Behaviour::Answer).await;
    let (_elsewhere, _ignored) =
        app(&bus, "org.eileen.Elsewhere", "/somewhere/else", Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    let dashed = entry("org.eileen.Test-App.desktop", true, AppKind::Desktop);
    assert_eq!(opener.launch(&dashed).await, Ok(std::process::id()), "at /org/eileen/Test_App");
    assert_eq!(app_seen(&mut seen).await.method, "Activate", "activated");
    assert_eq!(
        opener.launch(&entry("org.eileen.Elsewhere.desktop", true, AppKind::Desktop)).await,
        Err(DbusError::NotFound { what: "app Activate call" }),
        "an app that serves the interface elsewhere"
    );
}

#[tokio::test]
async fn the_indexed_text_editor_launches_from_its_app_entry() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/session/usr/share");
    let mut session =
        Session::from_vars(|name| (name == "XDG_CURRENT_DESKTOP").then(|| "GNOME".into()));
    session.data_dirs = vec![fixture];
    let entries = index(&session);
    // What the trait's caller holds from `Apps::index`, and gives back to `Apps::launch`.
    let listed: Vec<AppEntry> = entries.iter().map(AppEntry::from).collect();
    let editor = listed.iter().find(|app| app.name == "Text Editor").unwrap();
    let bus = PrivateBus::start();
    let (_service, mut seen) =
        app(&bus, "org.gnome.TextEditor", "/org/gnome/TextEditor", Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    let desktop = entry_for(&entries, editor).unwrap();
    assert_eq!(opener.launch(desktop).await, Ok(std::process::id()), "launched");
    assert_eq!(app_seen(&mut seen).await.method, "Activate", "over D-Bus, not its Exec line");
}

#[tokio::test]
async fn apps_that_cannot_be_activated_over_d_bus_are_refused_first() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.launch(&entry(APP_ID, false, AppKind::Desktop)).await,
        Err(DbusError::Unavailable { what: "launching apps that are not D-Bus activatable" }),
        "an Exec-only app needs eileen_supervisor::user_launch"
    );
    assert!(
        matches!(
            opener.launch(&entry("kde4-kate.desktop", true, AppKind::Desktop)).await,
            Err(DbusError::Invalid { what: "desktop file ID", .. })
        ),
        "an ID that is not a well-known name"
    );
    // The fake records a call before it answers, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the app was never called");
}

#[tokio::test]
async fn an_app_that_is_not_on_the_bus_is_unavailable() {
    let bus = PrivateBus::start();
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.launch(&entry(APP_ID, true, AppKind::Desktop)).await,
        Err(DbusError::Unavailable { what: "app Activate call" }),
        "nothing owns the name and the bus cannot start it"
    );
}

#[tokio::test]
async fn an_app_that_exits_as_it_starts_is_unavailable() {
    let bus = PrivateBus::start();
    let (_service, _seen) = app(&bus, APP, APP_PATH, Behaviour::ExitAtStart).await;
    let opener = Opener::new(&bus.connect().await);
    assert_eq!(
        opener.launch(&entry(APP_ID, true, AppKind::Desktop)).await,
        Err(DbusError::Unavailable { what: "app Activate call" }),
        "org.freedesktop.DBus.Error.Spawn.ChildExited: the service is not there"
    );
}

#[tokio::test]
async fn a_hung_app_passes_the_method_timeout() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Hang).await;
    // A short real method timeout, not paused time: the call goes through the bus daemon,
    // another process, so paused time would fire the timeout before any reply could come. The
    // fake never answers, so the outcome does not depend on how quickly the bus routes the call.
    let opener = Opener::new(&bus.connect_within(Duration::from_millis(50)).await);
    assert_eq!(
        opener.launch(&entry(APP_ID, true, AppKind::Desktop)).await,
        Err(DbusError::DeadlineExceeded { call: "app Activate call" }),
        "Activate never answered"
    );
    assert_eq!(app_seen(&mut seen).await.method, "Activate", "the call reached the app");
}

#[tokio::test]
async fn a_file_opens_with_an_activatable_app_through_its_open() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    let scratch = Scratch::new();
    let file = scratch.file("Quarterly report #3.odt");
    let with = entry(APP_ID, true, AppKind::Desktop);
    assert_eq!(opener.open_path_with(&file, &with).await, Ok(()), "opened");
    let call = app_seen(&mut seen).await;
    assert_eq!(call.method, "Open", "started with a file");
    let expected = format!("file://{}/Quarterly%20report%20%233.odt", scratch.0.display());
    assert_eq!(call.uris, [expected], "the file's URI, escaped");
    assert!(call.platform_data.is_empty(), "no platform data");
}

#[tokio::test]
async fn files_open_over_d_bus_only_with_unpackaged_activatable_apps() {
    let bus = PrivateBus::start();
    let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Answer).await;
    let opener = Opener::new(&bus.connect().await);
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let unavailable = |what| Err(OpenError::Bus(DbusError::Unavailable { what }));
    assert_eq!(
        opener.open_path_with(&file, &entry(APP_ID, false, AppKind::Desktop)).await,
        unavailable("opening files with apps that are not D-Bus activatable"),
        "an Exec-only app"
    );
    assert_eq!(
        opener.open_path_with(&file, &entry(APP_ID, true, AppKind::Packaged)).await,
        unavailable("opening files with packaged apps over D-Bus"),
        "a sandboxed app needs the document portal"
    );
    let desktop = entry(APP_ID, true, AppKind::Desktop);
    let refused = |error| Err(OpenError::File(error));
    assert_eq!(
        opener.open_path_with(&scratch.0.join("gone.txt"), &desktop).await,
        refused(FileError::NotFound { what: "file to open" }),
        "a missing file"
    );
    assert_eq!(
        opener.open_path_with(Path::new("notes.txt"), &desktop).await,
        refused(FileError::Invalid { what: "path", detail: "not absolute".to_owned() }),
        "a relative path"
    );
    assert_eq!(
        opener.open_path_with(&scratch.fifo("pipe"), &desktop).await,
        refused(FileError::Invalid { what: "path", detail: "not a file or folder".to_owned() }),
        "a FIFO"
    );
    // The fake records a call before it answers, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the app was never called");
}

#[test]
fn a_file_check_that_stalls_passes_the_method_timeout() {
    // A real timer, as in a_file_check_that_stalls_passes_the_deadline.
    one_blocking_thread().block_on(async {
        let bus = PrivateBus::start();
        let (_service, mut seen) = app(&bus, APP, APP_PATH, Behaviour::Answer).await;
        let opener = Opener::new(&bus.connect_within(Duration::from_millis(50)).await);
        let scratch = Scratch::new();
        let file = scratch.file("notes.txt");
        let with = entry(APP_ID, true, AppKind::Desktop);
        let release = stall_file_checks();
        assert_eq!(
            opener.open_path_with(&file, &with).await,
            Err(OpenError::File(FileError::Io {
                what: "file to open",
                detail: "timed out".to_owned()
            })),
            "the check still waited behind the stall when the method timeout passed"
        );
        // open_path_with answered before any call to the app.
        assert!(seen.try_recv().is_err(), "the app was not called");
        drop(release);
        // A connection without a method timeout, whose checks take METHOD_TIMEOUT.
        let unstalled = Opener::new(&bus.connect().await);
        assert_eq!(
            unstalled.open_path_with(&file, &with).await,
            Ok(()),
            "the same file, unstalled"
        );
        assert_eq!(app_seen(&mut seen).await.method, "Open", "called once the check ran");
    });
}

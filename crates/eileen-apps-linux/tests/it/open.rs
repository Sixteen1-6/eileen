//! The opener against a fake `OpenURI` portal on the private bus. The fake records each call and
//! answers on the request handle its `handle_token` names, as the real portal does.

use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_apps_linux::{DbusError, FileError, OpenError, Opener};
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
enum Answer {
    /// Emits `Response` with this code before it replies, as a quick portal can.
    Now(u32),
    /// Replies and stays silent: the test answers, or lets the deadline pass.
    Later,
    /// Refuses, as the portal does when the desktop's lockdown turns handlers off.
    NotAllowed,
}

/// What the fake portal saw.
#[derive(Debug)]
enum Event {
    /// A call to the portal.
    Call(PortalCall),
    /// `Close` on a request.
    Closed(OwnedObjectPath),
}

/// One call to the portal.
#[derive(Debug)]
struct PortalCall {
    method: &'static str,
    parent_window: String,
    /// The URI, or the path the descriptor names.
    target: String,
    /// How the descriptor is open; `None` for a URI.
    mode: Option<Mode>,
    /// The options' names.
    options: Vec<String>,
    /// The `handle_token` option.
    token: Option<String>,
    /// The handle the fake answered.
    handle: OwnedObjectPath,
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
async fn portal(bus: &PrivateBus, answer: Answer) -> (zbus::Connection, mpsc::Receiver<Event>) {
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
async fn next(seen: &mut mpsc::Receiver<Event>) -> Event {
    seen.recv().await.expect("the fake portal is still on the bus")
}

/// The next call the fake portal saw.
async fn call_seen(seen: &mut mpsc::Receiver<Event>) -> PortalCall {
    match next(seen).await {
        Event::Call(call) => call,
        other @ Event::Closed(_) => panic!("not a call: {other:?}"),
    }
}

/// A fresh folder of the temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("eileen-open-{}-{n}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }

    /// A file in the folder.
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, "a fixture document").unwrap();
        path
    }

    /// A FIFO in the folder, which nothing writes to.
    fn fifo(&self, name: &str) -> PathBuf {
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

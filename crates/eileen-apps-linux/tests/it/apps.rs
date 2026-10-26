//! `LinuxApps`, the `Apps` trait over the desktop entries and the opener, through a bridge on the
//! private bus. The index is the fixture session's (`index`), and the fakes are the opener's own
//! (`open`): the `OpenURI` portal and a D-Bus-activatable Text Editor. They run on a runtime of
//! their own, whose workers serve them while the test's thread calls the trait and blocks, as its
//! caller does. What the index lists and how each target opens is pinned by the `index` and
//! `open` tests: these pin that the trait reaches them, and what it adds: the conversions, the
//! kept index, the caller's deadline for reading the index, a launch and an open, the portal's
//! wait inside the bridge's deadline, and the checks of paths off the bridge's blocking threads.
//! Calls that pass no deadline give [`DEFAULT_DEADLINE`], as a caller with no deadline of its
//! own would.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use eileen_apps_linux::{LinuxApps, index};
use eileen_contracts::ids::Personal;
use eileen_dbus_linux::testing::PrivateBus;
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};
use eileen_platform::{AppEntry, AppKind, Apps, Deadline, PlatformError};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::index::session;
use crate::open::{
    Answer, AppCall, Behaviour, Event, Scratch, app, app_seen, call_seen, next,
    one_blocking_thread, portal,
};

/// The fixture's D-Bus-activatable Text Editor: its well-known name and its object path.
const EDITOR: &str = "org.gnome.TextEditor";
const EDITOR_PATH: &str = "/org/gnome/TextEditor";

/// What the jobs that take the bridge's blocking threads name.
const TAKE: &str = "taking a blocking thread";

/// How late the slow portal replies: inside the opener's 5 s budget, but late enough that a
/// wait of half the method timeout begun after the reply would pass the bridge's 5.5 s deadline.
const SLOW_REPLY: Duration = Duration::from_millis(3200);

/// The caller's deadline in the tests that wait for one to pass while a read holds the index or
/// the portal never answers: a second past [`DEFAULT_DEADLINE`]. The waits are checked from
/// below only, which no slow machine can break, and a call bounded by the default instead of
/// its caller's deadline answers a second too early for them.
pub(crate) const PAST_THE_DEFAULT: Deadline =
    Deadline::after(DEFAULT_DEADLINE.budget().saturating_add(Duration::from_secs(1)));

/// The caller's deadline in the tests where an app holds the call: well inside the connection's
/// 5 s method timeout, so the bridge's wait for the caller's deadline ends first and names the
/// trait call. A call bounded by the 5.5 s default instead ends on the method timeout, which
/// names the D-Bus call.
const A_SECOND: Deadline = Deadline::after(Duration::from_secs(1));

/// Fake services on a multi-thread runtime of their own.
struct Fakes {
    runtime: tokio::runtime::Runtime,
    services: Vec<zbus::Connection>,
}

impl Fakes {
    fn start() -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        Self { runtime: runtime.unwrap(), services: Vec::new() }
    }

    /// Runs `work` on the fakes' runtime.
    fn run<T>(&self, work: impl Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }

    /// Serves the fake portal, which answers as `answer` says, and returns what it sees.
    fn portal(&mut self, bus: &PrivateBus, answer: Answer) -> mpsc::Receiver<Event> {
        let (service, seen) = self.run(portal(bus, answer));
        self.services.push(service);
        seen
    }

    /// Serves the fake Text Editor, which answers as `behaviour` says, and returns the calls it
    /// sees.
    fn editor(&mut self, bus: &PrivateBus, behaviour: Behaviour) -> mpsc::Receiver<AppCall> {
        let (service, seen) = self.run(app(bus, EDITOR, EDITOR_PATH, behaviour));
        self.services.push(service);
        seen
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        // The connections end inside their runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.services.clear();
    }
}

/// The apps of the fixture session on a new bridge to `bus`.
fn apps(bus: &PrivateBus) -> LinuxApps {
    let session = session("en_US.UTF-8", "ubuntu:GNOME", None);
    LinuxApps::new(Bridge::on_private_bus(bus).unwrap(), session)
}

/// The app the index lists as `name`.
fn listed(apps: &LinuxApps, name: &str) -> AppEntry {
    let mut listed = apps.index(DEFAULT_DEADLINE).unwrap().into_iter();
    listed.find(|app| app.name == name).unwrap_or_else(|| panic!("{name} is not listed"))
}

/// An app that no index lists.
fn unlisted() -> AppEntry {
    AppEntry {
        id: "org.eileen.Removed.desktop".to_owned(),
        name: "Removed".to_owned(),
        kind: AppKind::Desktop,
        path: None,
    }
}

#[test]
fn index_lists_the_menu_as_the_traits_entries() {
    let bus = PrivateBus::start();
    let listed = apps(&bus).index(DEFAULT_DEADLINE).unwrap();
    let names: Vec<&str> = listed.iter().map(|app| app.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Back up photos",
            "Firefox (work profile)",
            "Htop",
            "Kate",
            "Spotify",
            "Text Editor",
            "VLC media player"
        ],
        "the index's apps, in its order"
    );
    let entries = index(&session("en_US.UTF-8", "ubuntu:GNOME", None));
    assert_eq!(listed.len(), entries.len(), "one entry for each");
    for (app, entry) in listed.iter().zip(&entries) {
        assert_eq!(app.id, entry.id, "the desktop file ID");
        let path = Personal::new(entry.path.get().to_string_lossy().into_owned());
        assert_eq!(app.path, Some(path), "{}: its desktop entry file, still personal", app.id);
    }
    let kind = |id: &str| listed.iter().find(|app| app.id == id).map(|app| app.kind);
    assert_eq!(kind("org.videolan.VLC.desktop"), Some(AppKind::Packaged), "Flatpak");
    assert_eq!(kind("spotify_spotify.desktop"), Some(AppKind::Packaged), "Snap");
    assert_eq!(kind("htop.desktop"), Some(AppKind::Desktop), "a classic app");
}

#[test]
fn an_index_that_a_read_holds_passes_its_deadline() {
    // The index opens each entry without waiting and skips what is not a file, so no FIFO holds
    // its read, and no unprivileged test can make a read hang as a hung network mount does. The
    // adapter reads on a pool whose one thread the test holds instead, so its read waits behind
    // it. A real wait: the caller's deadline is real time, and the call waits all of it. The
    // time is checked from below only, which no slow machine can break: the wait for the read's
    // answer never ends before its deadline, a second past the bridge's default.
    let pool = one_blocking_thread();
    let bus = PrivateBus::start();
    let apps = apps(&bus).reading_files_on(pool.handle().clone());
    let release = hold_the_blocking_thread(&pool);
    let started = Instant::now();
    let held = apps.index(PAST_THE_DEFAULT);
    let waited = started.elapsed();
    assert_eq!(
        held,
        Err(PlatformError::DeadlineExceeded { call: "app index" }),
        "the read still waited for the pool's thread at the deadline, and was left to it"
    );
    assert!(
        waited >= PAST_THE_DEFAULT.budget(),
        "the call waited the caller's whole deadline, not the default's: {waited:?}"
    );
    drop(release);
    let listed = apps.index(DEFAULT_DEADLINE).map(|listed| listed.len());
    assert_eq!(listed, Ok(7), "once the thread is free, the pool reads the whole index again");
}

#[test]
fn a_launch_that_indexes_again_passes_its_deadline_while_a_read_holds() {
    // As for the index: a real wait for the caller's deadline, a second past the bridge's
    // default, checked from below only. An app missing from the kept index takes a new one, a
    // read that waits behind the held thread with all of the deadline, which leaves the bridge
    // call none: the app is never asked.
    let pool = one_blocking_thread();
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let editor = listed(&apps(&bus), "Text Editor");
    let apps = apps(&bus).reading_files_on(pool.handle().clone());
    let release = hold_the_blocking_thread(&pool);
    let started = Instant::now();
    let held = apps.launch(&editor, PAST_THE_DEFAULT);
    let waited = started.elapsed();
    assert_eq!(
        held,
        Err(PlatformError::DeadlineExceeded { call: "app index" }),
        "a launch before any index: its new index passed the caller's deadline"
    );
    assert!(waited >= PAST_THE_DEFAULT.budget(), "the caller's whole deadline: {waited:?}");
    drop(release);
    // The fake records a call before it answers, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the Text Editor was never asked to start");
}

#[test]
fn an_open_with_an_app_that_indexes_again_passes_its_deadline_while_a_read_holds() {
    // As for a launch that indexes again: the read gets all of the caller's deadline.
    let pool = one_blocking_thread();
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let editor = listed(&apps(&bus), "Text Editor");
    let apps = apps(&bus).reading_files_on(pool.handle().clone());
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let release = hold_the_blocking_thread(&pool);
    let started = Instant::now();
    let held = apps.open_path(file.to_str().unwrap(), Some(&editor), PAST_THE_DEFAULT);
    let waited = started.elapsed();
    assert_eq!(
        held,
        Err(PlatformError::DeadlineExceeded { call: "app index" }),
        "an open before any index: its new index passed the caller's deadline"
    );
    assert!(waited >= PAST_THE_DEFAULT.budget(), "the caller's whole deadline: {waited:?}");
    drop(release);
    assert!(seen.try_recv().is_err(), "the Text Editor was never asked to open the file");
}

/// Holds the one blocking thread of `pool`, from `one_blocking_thread`, until the returned sender
/// drops, as a read hung on a network mount would, so a read given that pool waits behind it.
/// It returns once the thread is held; a test that panics drops the sender as it unwinds.
pub(crate) fn hold_the_blocking_thread(pool: &tokio::runtime::Runtime) -> oneshot::Sender<()> {
    let (release, released) = oneshot::channel::<()>();
    let (holding, held) = oneshot::channel();
    let _held = pool.spawn_blocking(move || {
        let _ignored = holding.send(());
        released.blocking_recv()
    });
    assert_eq!(held.blocking_recv(), Ok(()), "the pool's thread held");
    release
}

#[test]
fn an_activatable_app_launches_over_d_bus_and_answers_its_process_id() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let indexed = apps(&bus);
    let editor = listed(&indexed, "Text Editor");
    assert_eq!(
        indexed.launch(&editor, DEFAULT_DEADLINE),
        Ok(std::process::id()),
        "the process of the connection that answered: the fake's, in the test's own process"
    );
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Activate", "not its Exec line");
    let fresh = apps(&bus).launch(&editor, DEFAULT_DEADLINE);
    assert_eq!(fresh, Ok(std::process::id()), "before any index");
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Activate", "from the index it made");
}

#[test]
fn a_launch_or_an_open_that_an_app_holds_passes_the_callers_deadline() {
    // Real waits for the caller's deadline, a second each, checked from below only. Each call
    // reaches the app, which never answers, and ends when the bridge's wait for that second
    // does: the error names the trait call, not the app's Activate or Open call, which the
    // connection's method timeout would end only after 5 s.
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus, Behaviour::Hang);
    let apps = apps(&bus);
    let editor = listed(&apps, "Text Editor");
    let started = Instant::now();
    assert_eq!(
        apps.launch(&editor, A_SECOND),
        Err(PlatformError::DeadlineExceeded { call: "app launch" }),
        "the launch passed the caller's deadline"
    );
    assert!(started.elapsed() >= A_SECOND.budget(), "the caller's whole deadline");
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Activate", "the launch reached the app");
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let started = Instant::now();
    assert_eq!(
        apps.open_path(file.to_str().unwrap(), Some(&editor), A_SECOND),
        Err(PlatformError::DeadlineExceeded { call: "opening a file or folder" }),
        "the open passed the caller's deadline"
    );
    assert!(started.elapsed() >= A_SECOND.budget(), "the caller's whole deadline");
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Open", "the open reached the app");
}

#[test]
fn an_app_that_is_not_activatable_or_not_indexed_is_refused_before_any_call() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let apps = apps(&bus);
    assert_eq!(
        apps.launch(&listed(&apps, "Htop"), DEFAULT_DEADLINE),
        Err(PlatformError::Unavailable { what: "launching apps that are not D-Bus activatable" }),
        "an app started from its Exec line waits for eileen_supervisor::user_launch"
    );
    assert_eq!(
        apps.launch(&unlisted(), DEFAULT_DEADLINE),
        Err(PlatformError::NotFound { what: "desktop entry" }),
        "in neither the kept index nor a new one"
    );
    // The fake records a call before it answers, so a call made would be here by now.
    assert!(seen.try_recv().is_err(), "the Text Editor was never called");
}

#[test]
fn main_window_is_unavailable() {
    let bus = PrivateBus::start();
    assert_eq!(
        apps(&bus).main_window(std::process::id(), Deadline::after(Duration::from_secs(1))),
        Err(PlatformError::Unavailable { what: "an app's main window" }),
        "until phase L2"
    );
}

#[test]
fn web_addresses_and_uris_open_through_the_portal() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.portal(&bus, Answer::Now(0));
    let apps = apps(&bus);
    let url = "https://www.youtube.com/watch?v=abc&t=200";
    assert_eq!(apps.open_url(url), Ok(()), "a web address");
    let call = fakes.run(call_seen(&mut seen));
    assert_eq!((call.method, call.target.as_str()), ("OpenURI", url), "as given");
    assert_eq!(apps.open_uri("mailto:sam@example.com"), Ok(()), "a URI");
    let call = fakes.run(call_seen(&mut seen));
    assert_eq!((call.method, call.target.as_str()), ("OpenURI", "mailto:sam@example.com"), "too");
    assert!(
        matches!(
            apps.open_url("ftp://example.com"),
            Err(PlatformError::Invalid { what: "web address", .. })
        ),
        "a DbusError the opener refuses with, as its PlatformError twin"
    );
    assert!(
        matches!(
            apps.open_uri("https://example.com"),
            Err(PlatformError::Invalid { what: "URI", .. })
        ),
        "a web address opens with open_url"
    );
}

#[test]
fn the_portals_answers_are_platform_errors() {
    let what = "OpenURI request";
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let _seen = fakes.portal(&bus, Answer::Now(1));
    assert_eq!(
        apps(&bus).open_url("https://example.com"),
        Err(PlatformError::Refused { what, reason: "cancelled by the user" }),
        "response code 1"
    );
    let without = PrivateBus::start();
    assert_eq!(
        apps(&without).open_uri("mailto:sam@example.com"),
        Err(PlatformError::Unavailable { what }),
        "no portal on the bus"
    );
}

#[test]
fn a_portal_that_never_answers_passes_its_wait_and_the_request_is_closed() {
    // A real wait, to the end of the request's budget: the bridge's deadline is real time too.
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.portal(&bus, Answer::Later);
    assert_eq!(
        apps(&bus).open_url("https://example.com"),
        Err(PlatformError::DeadlineExceeded { call: "portal response" }),
        "the opener's wait ended first, not the bridge's deadline for opening a web address"
    );
    let call = fakes.run(call_seen(&mut seen));
    match fakes.run(next(&mut seen)) {
        Event::Closed(path) => assert_eq!(path, call.handle, "closed, which dismisses its dialog"),
        other @ Event::Call(_) => panic!("not a Close: {other:?}"),
    }
}

#[test]
fn a_portal_slow_to_reply_still_has_its_request_closed_inside_the_deadline() {
    // A real delay of the fake's reply, and real waits after it: the bridge's deadline is real
    // time.
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.portal(&bus, Answer::Slow(SLOW_REPLY));
    assert_eq!(
        apps(&bus).open_uri("mailto:sam@example.com"),
        Err(PlatformError::DeadlineExceeded { call: "portal response" }),
        "the wait got what the late reply left of the budget, and ended first"
    );
    let call = fakes.run(call_seen(&mut seen));
    match fakes.run(next(&mut seen)) {
        Event::Closed(path) => assert_eq!(path, call.handle, "closed, which dismisses its dialog"),
        other @ Event::Call(_) => panic!("not a Close: {other:?}"),
    }
}

#[test]
fn a_path_the_portal_never_answers_has_its_request_closed_within_the_callers_deadline() {
    // A real wait to the end of the request's budget, the caller's deadline less what the
    // adapter keeps for the close, checked from below only. The deadline is a second past the
    // bridge's default: a budget taken from the default instead would end before the wait has
    // passed the default, and a bridge call bounded by the default would be cancelled with the
    // request still open, naming the open rather than the portal's response.
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.portal(&bus, Answer::Later);
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let started = Instant::now();
    assert_eq!(
        apps(&bus).open_path(file.to_str().unwrap(), None, PAST_THE_DEFAULT),
        Err(PlatformError::DeadlineExceeded { call: "portal response" }),
        "the opener's wait ended first, not the bridge's deadline for opening a file or folder"
    );
    let waited = started.elapsed();
    assert!(waited >= DEFAULT_DEADLINE.budget(), "past the default, as allowed: {waited:?}");
    let call = fakes.run(call_seen(&mut seen));
    match fakes.run(next(&mut seen)) {
        Event::Closed(path) => assert_eq!(path, call.handle, "closed, which dismisses its dialog"),
        other @ Event::Call(_) => panic!("not a Close: {other:?}"),
    }
}

#[test]
fn a_path_opens_through_the_portal_or_with_an_activatable_app() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut opened = fakes.portal(&bus, Answer::Now(0));
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let apps = apps(&bus);
    let scratch = Scratch::new();
    let file = scratch.file("Quarterly report #3.odt");
    let path = file.to_str().unwrap();
    assert_eq!(apps.open_path(path, None, DEFAULT_DEADLINE), Ok(()), "with its default app");
    let call = fakes.run(call_seen(&mut opened));
    assert_eq!((call.method, Path::new(&call.target)), ("OpenFile", file.as_path()), "the portal");
    let editor = listed(&apps, "Text Editor");
    assert_eq!(apps.open_path(path, Some(&editor), DEFAULT_DEADLINE), Ok(()), "with an app");
    let call = fakes.run(app_seen(&mut seen));
    assert_eq!(call.method, "Open", "the app's own Open");
    let uri = format!("file://{}", path.replace(' ', "%20").replace('#', "%23"));
    assert_eq!(call.uris, [uri], "the file's URI");
}

#[test]
fn paths_are_checked_while_the_bridges_blocking_threads_are_taken() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut opened = fakes.portal(&bus, Answer::Now(0));
    let mut seen = fakes.editor(&bus, Behaviour::Answer);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let apps = LinuxApps::new(bridge.clone(), session("en_US.UTF-8", "ubuntu:GNOME", None));
    // The session bus first: zbus connects on a blocking thread, so none could connect after.
    assert_eq!(apps.open_url("https://example.com"), Ok(()), "connected");
    assert_eq!(fakes.run(call_seen(&mut opened)).method, "OpenURI", "the web address");
    let editor = listed(&apps, "Text Editor");
    let _taken = take_blocking_threads(&bridge);
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let path = file.to_str().unwrap();
    let checked = apps.open_path(path, None, DEFAULT_DEADLINE);
    assert_eq!(checked, Ok(()), "checked off the bridge's blocking threads");
    assert_eq!(fakes.run(call_seen(&mut opened)).method, "OpenFile", "then opened by the portal");
    let with_app = apps.open_path(path, Some(&editor), DEFAULT_DEADLINE);
    assert_eq!(with_app, Ok(()), "and checked for an app");
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Open", "then opened by the app");
}

/// Takes the bridge's two blocking threads (`eileen-dbus-linux`'s `BLOCKING_THREADS`), as two
/// checks hung on a file system would, until the returned senders drop: each job starts a
/// blocking task that holds its thread until released, and answers once it runs.
fn take_blocking_threads(bridge: &Bridge) -> Vec<oneshot::Sender<()>> {
    let take = |_n| {
        let (release, released) = oneshot::channel::<()>();
        let held =
            bridge.call(TAKE, Deadline::after(Duration::from_secs(30)), move |_buses| async move {
                let (running, runs) = oneshot::channel();
                let _held = tokio::runtime::Handle::current().spawn_blocking(move || {
                    let _ignored = running.send(());
                    released.blocking_recv()
                });
                runs.await.map_err(|_ended| PlatformError::Io { what: TAKE, detail: String::new() })
            });
        assert_eq!(held, Ok(()), "a blocking thread taken");
        release
    };
    (0..2).map(take).collect()
}

#[test]
fn a_path_or_an_app_that_cannot_open_it_is_refused_before_any_call() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut opened = fakes.portal(&bus, Answer::Now(0));
    let apps = apps(&bus);
    let scratch = Scratch::new();
    let file = scratch.file("notes.txt");
    let path = file.to_str().unwrap();
    let open = |path: &str, app: Option<&AppEntry>| apps.open_path(path, app, DEFAULT_DEADLINE);
    assert_eq!(
        open(file.with_file_name("gone.txt").to_str().unwrap(), None),
        Err(PlatformError::NotFound { what: "file to open" }),
        "a missing file: a FileError in an OpenError, as its PlatformError twin"
    );
    assert_eq!(
        open("Documents/notes.txt", None),
        Err(PlatformError::Invalid { what: "path", detail: "not absolute".to_owned() }),
        "a relative path"
    );
    let exec_only = Err(PlatformError::Unavailable {
        what: "opening files with apps that are not D-Bus activatable",
    });
    let vlc = listed(&apps, "VLC media player");
    assert_eq!(open(path, Some(&vlc)), exec_only, "a Flatpak app's Exec line");
    assert_eq!(open(path, Some(&listed(&apps, "Htop"))), exec_only, "a classic one's");
    assert_eq!(
        open(path, Some(&unlisted())),
        Err(PlatformError::NotFound { what: "desktop entry" }),
        "an app that no index lists"
    );
    // The fake records a call before it replies, so a call made would be here by now.
    assert!(opened.try_recv().is_err(), "the portal was never asked");
}

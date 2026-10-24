//! `LinuxApps`, the `Apps` trait over the desktop entries and the opener, through a bridge on the
//! private bus. The index is the fixture session's (`index`), and the fakes are the opener's own
//! (`open`): the `OpenURI` portal and a D-Bus-activatable Text Editor. They run on a runtime of
//! their own, whose workers serve them while the test's thread calls the trait and blocks, as its
//! caller does. What the index lists and how each target opens is pinned by the `index` and
//! `open` tests: these pin that the trait reaches them, and what it adds, the conversions, the
//! kept index, and the portal's wait inside the bridge's deadline.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use eileen_apps_linux::{LinuxApps, index};
use eileen_contracts::ids::Personal;
use eileen_dbus_linux::Bridge;
use eileen_dbus_linux::testing::PrivateBus;
use eileen_platform::{AppEntry, AppKind, Apps, Deadline, PlatformError};
use tokio::sync::mpsc;

use crate::index::session;
use crate::open::{
    Answer, AppCall, Behaviour, Event, Scratch, app, app_seen, call_seen, next, portal,
};

/// The fixture's D-Bus-activatable Text Editor: its well-known name and its object path.
const EDITOR: &str = "org.gnome.TextEditor";
const EDITOR_PATH: &str = "/org/gnome/TextEditor";

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

    /// Serves the fake Text Editor, and returns the calls it sees.
    fn editor(&mut self, bus: &PrivateBus) -> mpsc::Receiver<AppCall> {
        let (service, seen) = self.run(app(bus, EDITOR, EDITOR_PATH, Behaviour::Answer));
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
    let mut listed = apps.index().unwrap().into_iter();
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
    let listed = apps(&bus).index().unwrap();
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
fn an_activatable_app_launches_over_d_bus_and_answers_its_process_id() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus);
    let indexed = apps(&bus);
    let editor = listed(&indexed, "Text Editor");
    assert_eq!(
        indexed.launch(&editor),
        Ok(std::process::id()),
        "the process of the connection that answered: the fake's, in the test's own process"
    );
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Activate", "not its Exec line");
    assert_eq!(apps(&bus).launch(&editor), Ok(std::process::id()), "before any index");
    assert_eq!(fakes.run(app_seen(&mut seen)).method, "Activate", "from the index it made");
}

#[test]
fn an_app_that_is_not_activatable_or_not_indexed_is_refused_before_any_call() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut seen = fakes.editor(&bus);
    let apps = apps(&bus);
    assert_eq!(
        apps.launch(&listed(&apps, "Htop")),
        Err(PlatformError::Unavailable { what: "launching apps that are not D-Bus activatable" }),
        "an app started from its Exec line waits for eileen_supervisor::user_launch"
    );
    assert_eq!(
        apps.launch(&unlisted()),
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
    // A real wait, of half the method timeout: the bridge's deadline is real time too.
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
fn a_path_opens_through_the_portal_or_with_an_activatable_app() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::start();
    let mut opened = fakes.portal(&bus, Answer::Now(0));
    let mut seen = fakes.editor(&bus);
    let apps = apps(&bus);
    let scratch = Scratch::new();
    let file = scratch.file("Quarterly report #3.odt");
    let path = file.to_str().unwrap();
    assert_eq!(apps.open_path(path, None), Ok(()), "with its default app");
    let call = fakes.run(call_seen(&mut opened));
    assert_eq!((call.method, Path::new(&call.target)), ("OpenFile", file.as_path()), "the portal");
    assert_eq!(apps.open_path(path, Some(&listed(&apps, "Text Editor"))), Ok(()), "with an app");
    let call = fakes.run(app_seen(&mut seen));
    assert_eq!(call.method, "Open", "the app's own Open");
    let uri = format!("file://{}", path.replace(' ', "%20").replace('#', "%23"));
    assert_eq!(call.uris, [uri], "the file's URI");
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
    assert_eq!(
        apps.open_path(file.with_file_name("gone.txt").to_str().unwrap(), None),
        Err(PlatformError::NotFound { what: "file to open" }),
        "a missing file: a FileError in an OpenError, as its PlatformError twin"
    );
    assert_eq!(
        apps.open_path("Documents/notes.txt", None),
        Err(PlatformError::Invalid { what: "path", detail: "not absolute".to_owned() }),
        "a relative path"
    );
    let exec_only = Err(PlatformError::Unavailable {
        what: "opening files with apps that are not D-Bus activatable",
    });
    let vlc = listed(&apps, "VLC media player");
    assert_eq!(apps.open_path(path, Some(&vlc)), exec_only, "a Flatpak app's Exec line");
    assert_eq!(apps.open_path(path, Some(&listed(&apps, "Htop"))), exec_only, "a classic one's");
    assert_eq!(
        apps.open_path(path, Some(&unlisted())),
        Err(PlatformError::NotFound { what: "desktop entry" }),
        "an app that no index lists"
    );
    // The fake records a call before it replies, so a call made would be here by now.
    assert!(opened.try_recv().is_err(), "the portal was never asked");
}

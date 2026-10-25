//! `Grounding` on Linux: `LinuxDesktop` through a bridge on a private session bus, whose fake
//! launcher names a second private bus as the accessibility bus, with a fake registry and fake
//! apps on it (`fake`). The test's thread calls the desktop as the desktop server does, blocking;
//! the fakes run on a runtime of their own, whose workers serve them meanwhile. Every fake app is
//! a connection of the test's own process, so each window's process is the test binary.

use std::time::Duration;

use eileen_contracts::desktop::{ElementInfo, Point, Rect, SelectionSource, WindowInfo, WindowRef};
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::testing::PrivateBus;
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};
use eileen_desktop_linux::{
    CALL_TIMEOUT, FOCUS_LIMITS, KnownWindow, LinuxDesktop, LinuxDesktopSettings, ObjectRef,
    window_id,
};
use eileen_platform::{Deadline, Grounding, PlatformError, TreeLimits};
use tokio::time::Instant;

use crate::fake::{
    ACTIVE, App, Calls, DEFUNCT, FOCUSED, ICONIFIED, Node, ROOT, Reference, SHOWING, Session,
    registry, words,
};

/// AT-SPI2's role numbers the tests use, written out.
const DIALOG: u32 = 16;
const FRAME: u32 = 23;
const LIST_ITEM: u32 = 32;
const PANEL: u32 = 39;
const PASSWORD_TEXT: u32 = 40;
const TEXT: u32 = 61;
const APPLICATION: u32 = 75;

/// The deadline of a call that cannot answer in time, its app being silent: it passes whatever
/// its length, and a short one keeps the test short.
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));

/// The fakes, on a multi-thread runtime of their own: the session bus with the launcher, and
/// the accessibility bus with the apps and, once a desktop is made, the registry.
struct World {
    runtime: tokio::runtime::Runtime,
    apps: Vec<App>,
    /// The roots the registry lists, in order: each app's, and those a test adds.
    roots: Vec<Reference>,
    registry: Option<zbus::Connection>,
    session: Option<Session>,
    a11y: PrivateBus,
}

impl World {
    fn new() -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        let runtime = runtime.unwrap();
        let a11y = PrivateBus::start();
        let session = Some(runtime.block_on(Session::start(a11y.address())));
        Self { runtime, apps: Vec::new(), roots: Vec::new(), registry: None, session, a11y }
    }

    /// Starts an app and serves the objects `nodes` gives once the app's name is known, its
    /// root among them; the app's index.
    fn app<'p>(&mut self, nodes: impl FnOnce(&App) -> Vec<(&'p str, Node)>) -> usize {
        let app = self.runtime.block_on(async {
            let app = App::start(&self.a11y).await;
            for (path, node) in nodes(&app) {
                app.add(path, node).await;
            }
            app
        });
        self.roots.push(app.reference(ROOT));
        self.apps.push(app);
        self.apps.len() - 1
    }

    /// Lists, after the roots so far, the root of an app that has gone since the registry
    /// listed it: a unique name nothing holds, which the bus's daemon never gives out.
    fn gone_app(&mut self) {
        self.roots.push((":9.9".to_owned(), ROOT.try_into().unwrap()));
    }

    /// App `app`'s object at `path`.
    fn object(&self, app: usize, path: &str) -> ObjectRef {
        self.apps[app].object(path)
    }

    /// The session bus, where the launcher is.
    fn session_bus(&self) -> &PrivateBus {
        &self.session.as_ref().unwrap().bus
    }

    /// A desktop with `settings` on a bridge to the session bus. The first one serves the
    /// registry, which lists the roots listed so far, in order.
    fn desktop(&mut self, settings: LinuxDesktopSettings) -> LinuxDesktop {
        if self.registry.is_none() {
            let roots = self.roots.clone();
            self.registry = Some(self.runtime.block_on(registry(&self.a11y, roots)));
        }
        LinuxDesktop::new(Bridge::on_private_bus(self.session_bus()).unwrap(), settings)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // The connections end inside their runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.apps.clear();
        self.registry = None;
        self.session = None;
    }
}

/// An app's root, which lists the app's windows at `windows`.
fn root(app: &App, windows: &[&str]) -> (&'static str, Node) {
    (ROOT, Node { role: APPLICATION, ..Node::showing("app", app.references(windows)) })
}

/// A showing top-level window of `role`, named `name`, in `states` too, with `children`, whose
/// box in its own coordinates sits at (12, 7), as a window with shadows does, and is 800 by 600.
fn window(role: u32, name: &str, states: &[u32], children: Vec<Reference>) -> Node {
    let states = words(&[&[SHOWING], states].concat());
    Node { role, states, extents: Some((12, 7, 800, 600)), ..Node::showing(name, children) }
}

/// The settings of the product, with `eileen-hud` as Eileen's own process.
fn settings() -> LinuxDesktopSettings {
    LinuxDesktopSettings {
        deadline: DEFAULT_DEADLINE,
        call_timeout: CALL_TIMEOUT,
        own_processes: vec!["eileen-hud".to_owned()],
        focus_limits: FOCUS_LIMITS,
    }
}

/// The file name of the test binary: every fake app's process.
fn this_process() -> String {
    let exe = std::env::current_exe().unwrap();
    exe.file_name().unwrap().to_str().unwrap().to_owned()
}

/// Each window as its id, class, z-order, foreground flag and frame.
fn summary(windows: &[WindowInfo]) -> Vec<(i64, &str, u32, bool, Rect)> {
    windows
        .iter()
        .map(|w| (w.hwnd, w.class.as_str(), w.z_order, w.is_foreground, w.frame_rect))
        .collect()
}

/// A world whose active window's focused text field holds `text` with `selections`, and the
/// field's calls. The field's `AccessibleId` is empty.
fn focused_text(text: String, selections: Vec<(i32, i32)>) -> (World, Calls) {
    let mut world = World::new();
    let (states, accessible_id) = (words(&[SHOWING, FOCUSED]), Some(String::new()));
    let text = Some(text);
    let field = Node { role: TEXT, states, text, selections, accessible_id, ..Node::default() };
    let calls = field.calls.clone();
    world.app(|app| {
        let editor = window(FRAME, "Editor", &[ACTIVE], app.references(&["/letter"]));
        vec![root(app, &["/editor"]), ("/editor", editor), ("/letter", field)]
    });
    (world, calls)
}

#[test]
fn windows_are_each_applications_top_level_objects() {
    let mut world = World::new();
    let mail = world.app(|app| {
        let inbox = window(FRAME, "Inbox \u{202E}(3)", &[], vec![]);
        let compose =
            Node { extents: Some((0, 0, 400, -5)), ..window(DIALOG, "Compose", &[], vec![]) };
        vec![root(app, &["/inbox", "/compose"]), ("/inbox", inbox), ("/compose", compose)]
    });
    let files = world.app(|app| {
        let plain = Node { extents: None, ..window(FRAME, "Files", &[ACTIVE], vec![]) };
        vec![root(app, &["/files"]), ("/files", plain)]
    });
    let desktop = world.desktop(settings());
    let windows = desktop.windows(false).unwrap();
    let inbox = world.object(mail, "/inbox");
    let expected = WindowInfo {
        hwnd: window_id(&inbox),
        pid: std::process::id(),
        process: this_process(),
        exe_version: None,
        title: Untrusted::new("Inbox (3)".to_owned()),
        class: "frame".to_owned(),
        monitor_ordinal: None,
        frame_rect: Rect { x: 0, y: 0, w: 800, h: 600 },
        z_order: 1,
        is_minimized: false,
        is_cloaked: false,
        is_foreground: false,
        is_eileen: false,
        is_private: false,
        url_host: None,
    };
    assert_eq!(
        windows.get(1),
        Some(&expected),
        "the inbox: the pid from the bus's daemon, the process from it, the title sanitized, the \
         class the role's name and the frame window-relative"
    );
    assert_eq!(
        summary(&windows),
        [
            (window_id(&world.object(files, "/files")), "frame", 0, true, Rect::default()),
            (window_id(&inbox), "frame", 1, false, Rect { x: 0, y: 0, w: 800, h: 600 }),
            (
                window_id(&world.object(mail, "/compose")),
                "dialog",
                2,
                false,
                Rect { w: 400, ..Rect::default() }
            ),
        ],
        "the active window first, then the registry's order; no Component, no box, and a \
         negative height is 0"
    );
    let known =
        KnownWindow { object: inbox.clone(), pid: std::process::id(), process: this_process() };
    assert_eq!(desktop.known_window(window_id(&inbox)), Some(known), "kept, to reach it again");
}

#[test]
fn minimized_windows_are_left_out_unless_asked() {
    let mut world = World::new();
    let app = world.app(|app| {
        let away = window(FRAME, "Away", &[ICONIFIED], vec![]);
        vec![
            root(app, &["/away", "/here"]),
            ("/away", away),
            ("/here", window(FRAME, "Here", &[], vec![])),
        ]
    });
    let desktop = world.desktop(settings());
    let (away, here) =
        (window_id(&world.object(app, "/away")), window_id(&world.object(app, "/here")));
    let shown = desktop.windows(false).unwrap();
    let shown: Vec<_> = shown.iter().map(|w| (w.hwnd, w.z_order)).collect();
    assert_eq!(shown, [(here, 1)], "the minimized window is left out, and counted in the z-order");
    assert!(desktop.known_window(away).is_some(), "but kept, to reach it again");
    let all = desktop.windows(true).unwrap();
    let all: Vec<_> = all.iter().map(|w| (w.hwnd, w.is_minimized)).collect();
    assert_eq!(all, [(away, true), (here, false)], "asked for, it is listed and says so");
}

#[test]
fn the_foreground_is_the_active_window_and_none_without_one() {
    let mut world = World::new();
    let first = world.app(|app| {
        let gone = window(FRAME, "Gone", &[ACTIVE, DEFUNCT], vec![]);
        let front = window(FRAME, "Front", &[ACTIVE], vec![]);
        let idle = window(FRAME, "Idle", &[], vec![]);
        vec![
            root(app, &["/idle", "/gone", "/front"]),
            ("/idle", idle),
            ("/gone", gone),
            ("/front", front),
        ]
    });
    world.app(|app| {
        vec![root(app, &["/stale"]), ("/stale", window(FRAME, "Stale", &[ACTIVE], vec![]))]
    });
    world.gone_app();
    let desktop = world.desktop(settings());
    let front = window_id(&world.object(first, "/front"));
    assert_eq!(
        desktop.foreground().unwrap(),
        Some(WindowRef { hwnd: front, pid: std::process::id() }),
        "the first active window that is not defunct, a gone app leaving it known"
    );
    let windows = desktop.windows(true).unwrap();
    let in_front: Vec<_> = windows.iter().filter(|w| w.is_foreground).map(|w| w.hwnd).collect();
    assert_eq!(
        in_front,
        [front],
        "a second active window, as a stale app leaves one, is not in front"
    );
    let gone = window_id(&world.object(first, "/gone"));
    assert!(windows.iter().all(|w| w.hwnd != gone), "a defunct window is not listed");
    let mut idle = World::new();
    idle.app(|app| {
        let field = Node { states: words(&[SHOWING, FOCUSED]), ..Node::default() };
        let idle = window(FRAME, "Idle", &[], app.references(&["/field"]));
        vec![root(app, &["/idle"]), ("/idle", idle), ("/field", field)]
    });
    let desktop = idle.desktop(settings());
    assert_eq!(desktop.foreground().unwrap(), None, "no window is active");
    assert_eq!(desktop.focused_element(Deadline::UIA).unwrap(), None, "so nothing is focused");
}

/// The app in front, busy past its call timeout: its root answers `NoReply`, so its active
/// sign-in dialog, whose password field has the focus, is never read.
fn busy_sign_in(app: &App) -> Vec<(&'static str, Node)> {
    let (path, root) = root(app, &["/login"]);
    let states = words(&[SHOWING, FOCUSED]);
    let password = Node { role: PASSWORD_TEXT, states, ..Node::default() };
    let login = window(DIALOG, "Sign in", &[ACTIVE], app.references(&["/password"]));
    vec![(path, Node { no_reply: true, ..root }), ("/login", login), ("/password", password)]
}

#[test]
fn while_an_app_does_not_answer_the_front_window_is_unknown() {
    let unknown = |call| PlatformError::DeadlineExceeded { call };
    let mut world = World::new();
    world.app(busy_sign_in);
    let idle =
        world.app(|app| vec![root(app, &["/idle"]), ("/idle", window(FRAME, "Idle", &[], vec![]))]);
    let desktop = world.desktop(settings());
    let listed: Vec<_> = desktop.windows(true).unwrap().iter().map(|w| w.hwnd).collect();
    assert_eq!(listed, [window_id(&world.object(idle, "/idle"))], "the busy app's are left out");
    let foreground = desktop.foreground();
    assert_eq!(
        foreground.unwrap_err(),
        unknown("foreground window"),
        "the busy app may be in front"
    );
    let focus = desktop.focused_element(Deadline::UIA);
    assert_eq!(focus.unwrap_err(), unknown("focused element"), "so the focus is unknown");
    let selection = desktop.selection(false, Deadline::UIA);
    assert_eq!(selection.unwrap_err(), unknown("focused element"), "and with it the selection");
    let mut world = World::new();
    world.app(busy_sign_in);
    let stale = world.app(|app| {
        let field = Node { role: TEXT, states: words(&[SHOWING, FOCUSED]), ..Node::default() };
        let editor = window(FRAME, "Editor", &[ACTIVE], app.references(&["/field"]));
        vec![root(app, &["/editor"]), ("/editor", editor), ("/field", field)]
    });
    let desktop = world.desktop(settings());
    let windows = desktop.windows(true).unwrap();
    let listed: Vec<_> = windows.iter().map(|w| (w.hwnd, w.is_foreground)).collect();
    assert_eq!(
        listed,
        [(window_id(&world.object(stale, "/editor")), false)],
        "a window of another app that still says it is active is not marked in front"
    );
    assert_eq!(desktop.foreground().unwrap_err(), unknown("foreground window"), "nor answered so");
    assert_eq!(
        desktop.focused_element(Deadline::UIA).unwrap_err(),
        unknown("focused element"),
        "nor its field as the focus, while the keyboard may be in the password field"
    );
}

#[test]
fn the_focused_element_is_found_in_the_foreground_window() {
    let mut world = World::new();
    world.app(|app| {
        // A focused field in a window that is not active is never the answer.
        let field = Node { states: words(&[SHOWING, FOCUSED]), ..Node::default() };
        let back = window(FRAME, "Back", &[], app.references(&["/field"]));
        vec![root(app, &["/back"]), ("/back", back), ("/field", field)]
    });
    let front = world.app(|app| {
        let focused = words(&[SHOWING, FOCUSED]);
        let list = Node {
            states: focused.clone(),
            ..Node::showing("list", app.references(&["/a", "/b"]))
        };
        let row = Node {
            role: LIST_ITEM,
            states: focused,
            interfaces: vec!["org.a11y.atspi.Action".to_owned()],
            extents: Some((5, -3, -1, 20)),
            accessible_id: Some("row-\u{202E}2".to_owned()),
            ..Node::showing("Inbox \u{200F}row", vec![])
        };
        let panel = Node { role: PANEL, ..Node::showing("panel", app.references(&["/list"])) };
        vec![
            root(app, &["/front"]),
            ("/front", window(FRAME, "Front", &[ACTIVE], app.references(&["/panel"]))),
            ("/panel", panel),
            ("/list", list),
            ("/a", Node::showing("a", vec![])),
            ("/b", row),
        ]
    });
    let desktop = world.desktop(settings());
    let expected = ElementInfo {
        control_type: "list item".to_owned(),
        name: Untrusted::new("Inbox row".to_owned()),
        automation_id: Some(Untrusted::new("row-2".to_owned())),
        rect: Rect { x: 5, y: -3, w: 0, h: 20 },
        patterns: vec!["Action".to_owned(), "Component".to_owned()],
        is_password: false,
        window: WindowRef {
            hwnd: window_id(&world.object(front, "/front")),
            pid: std::process::id(),
        },
    };
    assert_eq!(
        desktop.focused_element(Deadline::UIA).unwrap(),
        Some(expected),
        "the focused row inside the focused list, named and identified as sanitized, its box in \
         its window with a negative width as 0, and its interfaces' short names"
    );
    assert_eq!(desktop.selection(false, Deadline::UIA).unwrap(), None, "a row without Text");
}

#[test]
fn a_focus_search_cut_short_is_unknown_not_none() {
    let mut world = World::new();
    world.app(|app| {
        let paths = ["/a", "/b", "/c", "/d"];
        let front = window(FRAME, "Front", &[ACTIVE], app.references(&paths));
        let mut nodes = vec![root(app, &["/front"]), ("/front", front)];
        nodes.extend(paths.map(|path| (path, Node::showing("plain", vec![]))));
        nodes
    });
    let small = LinuxDesktopSettings {
        focus_limits: TreeLimits { max_depth: 24, max_nodes: 3 },
        ..settings()
    };
    assert_eq!(
        world.desktop(small).focused_element(Deadline::UIA),
        Err(PlatformError::DeadlineExceeded { call: "focused element" }),
        "three objects read and none focused: the focus is unknown"
    );
    assert_eq!(
        world.desktop(settings()).focused_element(Deadline::UIA),
        Ok(None),
        "the whole window read and nothing focused: there is no focus"
    );
}

#[test]
fn a_focused_password_field_says_so_and_its_selection_is_never_read() {
    let mut world = World::new();
    let field = Node {
        role: PASSWORD_TEXT,
        states: words(&[SHOWING, FOCUSED]),
        text: Some("hunter2".to_owned()),
        selections: vec![(0, 7)],
        ..Node::default()
    };
    let calls = field.calls.clone();
    world.app(|app| {
        let login = window(DIALOG, "Sign in", &[ACTIVE], app.references(&["/password"]));
        vec![root(app, &["/login"]), ("/login", login), ("/password", field)]
    });
    let desktop = world.desktop(settings());
    let element = desktop.focused_element(Deadline::UIA).unwrap().unwrap();
    assert_eq!(
        (element.control_type.as_str(), element.is_password, element.automation_id),
        ("password text", true, None),
        "a password field, which has no automation id"
    );
    assert_eq!(desktop.selection(true, Deadline::UIA).unwrap(), None, "the clipboard allowed");
    assert_eq!(*calls.text_reads.lock().unwrap(), [], "its text is never asked for");
}

#[test]
fn the_selection_is_the_first_range_of_the_focused_text() {
    let letter = format!("Dear \u{202E}Ada,\u{2066} {}", "x".repeat(9_000));
    let (mut world, calls) = focused_text(letter, vec![(10, 9_000), (0, 4)]);
    let selection = world.desktop(settings()).selection(false, Deadline::UIA).unwrap().unwrap();
    assert_eq!(selection.source, SelectionSource::TextPattern, "read from the Text interface");
    let text = selection.text.get();
    assert!(text.starts_with("Dear Ada, xx"), "sanitized: {:?}", text.get(..20));
    assert_eq!(text.chars().count(), 4_000, "kept to 4,000 characters");
    assert_eq!(
        *calls.text_reads.lock().unwrap(),
        [(10, 4_010)],
        "the first range, to 4,000 at most"
    );
    let (mut world, calls) = focused_text("Dear Ada".to_owned(), vec![]);
    let desktop = world.desktop(settings());
    assert_eq!(desktop.selection(false, Deadline::UIA).unwrap(), None, "nothing selected");
    assert_eq!(*calls.text_reads.lock().unwrap(), [], "and nothing read");
    let field = desktop.focused_element(Deadline::UIA).unwrap().unwrap();
    assert_eq!(field.automation_id, None, "an empty AccessibleId is none");
}

#[test]
fn a_focus_search_stops_at_four_fifths_of_its_deadline() {
    let mut world = World::new();
    world.app(|app| {
        // The walk enters `/stuck`, whose listing never answers, before `/panel`, which holds the
        // focused field.
        let hour = Some(Duration::from_secs(3_600));
        let stuck = Node { slow_listing: hour, ..Node::showing("stuck", app.references(&["/x"])) };
        let field = Node { states: words(&[SHOWING, FOCUSED]), ..Node::default() };
        let front = window(FRAME, "Front", &[ACTIVE], app.references(&["/stuck", "/panel"]));
        let panel = Node::showing("panel", app.references(&["/field"]));
        vec![
            root(app, &["/front"]),
            ("/front", front),
            ("/stuck", stuck),
            ("/panel", panel),
            ("/field", field),
        ]
    });
    // The listing costs the 250 ms call timeout, past four fifths of 300 ms: the search stops
    // there, where one that read on would find the field at about 255 ms.
    let quick = LinuxDesktopSettings { call_timeout: Duration::from_millis(250), ..settings() };
    let deadline = Deadline::after(Duration::from_millis(300));
    assert_eq!(
        world.desktop(quick).focused_element(deadline),
        Err(PlatformError::DeadlineExceeded { call: "focused element" }),
        "the search leaves the last fifth of its deadline unread"
    );
}

#[test]
fn an_app_that_does_not_answer_leaves_out_only_its_windows() {
    let mut world = World::new();
    world.app(|app| {
        let (path, root) = root(app, &["/w"]);
        vec![(path, Node { no_reply: true, ..root }), ("/w", window(FRAME, "Hung", &[], vec![]))]
    });
    world.app(|app| {
        let stuck = Node { no_reply: true, ..window(FRAME, "Stuck", &[], vec![]) };
        vec![
            root(app, &["/fine", "/stuck"]),
            ("/fine", window(FRAME, "Fine", &[], vec![])),
            ("/stuck", stuck),
        ]
    });
    let closing = world.app(|app| {
        // `/closed` is listed but no longer served, as a window that closed since.
        vec![root(app, &["/closed", "/open"]), ("/open", window(FRAME, "Open", &[], vec![]))]
    });
    let desktop = world.desktop(settings());
    let listed: Vec<_> = desktop.windows(true).unwrap().iter().map(|w| w.hwnd).collect();
    assert_eq!(
        listed,
        [window_id(&world.object(closing, "/open"))],
        "an app whose root or one of whose windows does not answer is left out whole, and a \
         window gone since its app's listing leaves out only itself"
    );
}

#[test]
fn an_app_has_at_most_64_windows_listed() {
    let mut world = World::new();
    let paths: Vec<String> = (0..65).map(|n| format!("/w{n}")).collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    world.app(|app| {
        let mut nodes = vec![root(app, &paths)];
        nodes.extend(paths.iter().map(|&path| (path, window(FRAME, path, &[], vec![]))));
        nodes
    });
    let windows = world.desktop(settings()).windows(true).unwrap();
    assert_eq!(windows.len(), 64, "the first 64 of an app's 65 windows");
}

#[test]
fn eileens_own_windows_are_marked_by_process() {
    let mut world = World::new();
    world.app(|app| vec![root(app, &["/hud"]), ("/hud", window(FRAME, "Eileen", &[], vec![]))]);
    let others = world.desktop(settings()).windows(false).unwrap();
    assert!(!others[0].is_eileen, "a process the settings do not name, whatever its title");
    let own = LinuxDesktopSettings { own_processes: vec![this_process()], ..settings() };
    let mine = world.desktop(own).windows(false).unwrap();
    assert!(mine[0].is_eileen, "the process the settings name");
}

#[test]
fn what_wayland_withholds_is_unavailable() {
    let bus = PrivateBus::start();
    let desktop = LinuxDesktop::new(Bridge::on_private_bus(&bus).unwrap(), settings());
    let unavailable = |what| PlatformError::Unavailable { what };
    let point = Point { x: 10, y: 10 };
    assert_eq!(desktop.monitors(), Err(unavailable("monitors")), "no monitor layout");
    assert_eq!(
        desktop.window_from_point(point),
        Err(unavailable("window under a point")),
        "no screen coordinates"
    );
    assert_eq!(
        desktop.element_from_point(point, Deadline::UIA),
        Err(unavailable("element under a point")),
        "for an element neither"
    );
    assert_eq!(desktop.presentation_state(), Err(unavailable("presentation state")), "no state");
    assert_eq!(
        desktop.mic_in_use_elsewhere(),
        Err(unavailable("microphone use elsewhere")),
        "PipeWire's, in another crate"
    );
    let changes = desktop.subscribe(Box::new(|_change| {}));
    assert_eq!(changes.unwrap_err(), unavailable("desktop changes"), "AT-SPI's events come later");
}

#[test]
fn without_an_accessibility_bus_every_call_is_unavailable() {
    let bus = PrivateBus::start();
    let desktop = LinuxDesktop::new(Bridge::on_private_bus(&bus).unwrap(), settings());
    let no_bus = PlatformError::Unavailable { what: "accessibility bus" };
    assert_eq!(desktop.windows(true), Err(no_bus.clone()), "no launcher on the session bus");
    assert_eq!(desktop.foreground(), Err(no_bus.clone()), "for the foreground neither");
    assert_eq!(desktop.focused_element(Deadline::UIA), Err(no_bus.clone()), "nor the focus");
    assert_eq!(desktop.selection(true, Deadline::UIA), Err(no_bus), "nor the selection");
    let world = World::new();
    let desktop =
        LinuxDesktop::new(Bridge::on_private_bus(world.session_bus()).unwrap(), settings());
    assert_eq!(
        desktop.windows(true),
        Err(PlatformError::Unavailable { what: "accessibility registry" }),
        "an accessibility bus with no registry"
    );
}

#[test]
fn a_silent_app_answers_deadline_exceeded_within_the_deadline() {
    let mut world = World::new();
    world.app(|app| {
        let (path, root) = root(app, &[]);
        vec![(path, Node { silent: true, ..root })]
    });
    let desktop = world.desktop(LinuxDesktopSettings { deadline: SHORT, ..settings() });
    let started = Instant::now();
    assert_eq!(
        desktop.windows(true),
        Err(PlatformError::DeadlineExceeded { call: "desktop windows" }),
        "the app's root never answers GetRole"
    );
    let waited = started.elapsed();
    assert!(
        waited >= SHORT.budget() && waited < CALL_TIMEOUT,
        "the settings' deadline, not the app's call timeout: {waited:?}"
    );
    // The settings' deadline is the default here, so only the call's own can answer in time.
    let desktop = world.desktop(settings());
    let started = Instant::now();
    assert_eq!(
        desktop.focused_element(SHORT),
        Err(PlatformError::DeadlineExceeded { call: "focused element" }),
        "a call that takes a deadline keeps its own"
    );
    let waited = started.elapsed();
    assert!(waited >= SHORT.budget() && waited < CALL_TIMEOUT, "within it: {waited:?}");
    // With a call timeout shorter than the deadline, the silent app costs its call timeout.
    let quick = LinuxDesktopSettings { call_timeout: SHORT.budget(), ..settings() };
    let started = Instant::now();
    assert_eq!(world.desktop(quick).windows(true), Ok(Vec::new()), "the silent app left out");
    let waited = started.elapsed();
    assert!(waited >= SHORT.budget() && waited < CALL_TIMEOUT, "its call timeout: {waited:?}");
}

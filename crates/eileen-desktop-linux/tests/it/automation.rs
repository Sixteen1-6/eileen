//! `Automation` on Linux: `LinuxDesktop` through a bridge, against the fakes `grounding`'s
//! [`World`] serves, called from the test's thread as the desktop server calls it.

use std::time::Duration;

use eileen_contracts::desktop::{ElementInfo, Rect, Selector, SelectorStep, WindowRef};
use eileen_contracts::ids::Untrusted;
use eileen_desktop_linux::{LinuxDesktop, LinuxDesktopSettings, ObjectRef, window_id};
use eileen_platform::{Automation, Deadline, ElementHandle, Grounding, PlatformError, TreeLimits};
use serde_json::json;
use tokio::time::Instant;

use crate::fake::{ACTIVE, Calls, EDITABLE, FOCUSED, Node, READ_ONLY, SHOWING, count, words};
use crate::grounding::{SHORT, World, root, settings, this_process, window};

/// AT-SPI2's role numbers the tests use, written out.
const FRAME: u32 = 23;
const PANEL: u32 = 39;
const PASSWORD_TEXT: u32 = 40;
const PUSH_BUTTON: u32 = 43;
const SLIDER: u32 = 51;
const TEXT: u32 = 61;

/// The handle of `object` in window `hwnd`, packed as the crate's doc says: a version word, the
/// two lengths, then the bus name's and the path's bytes, four to a word.
fn handle(hwnd: i64, object: &ObjectRef) -> ElementHandle {
    let (bus, path) = (object.bus.as_bytes(), object.path.as_bytes());
    let length = |bytes: &[u8]| i32::try_from(bytes.len()).unwrap();
    let mut runtime_id = vec![1, length(bus), length(path)];
    let bytes = [bus, path].concat();
    for chunk in bytes.chunks(4) {
        let mut word = [0_u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        runtime_id.push(i32::from_le_bytes(word));
    }
    ElementHandle { hwnd, runtime_id }
}

/// A world with one app whose active window "Main" holds a toolbar with a Save button, a Save
/// As button and a hidden menu with an item, and the window's id. `nodes` adds more objects
/// under the toolbar, after the two buttons.
fn editor(world: &mut World, nodes: Vec<(&'static str, Node)>) -> (usize, i64) {
    let app = world.app(|app| {
        let mut toolbar_children = vec!["/save", "/save_as", "/menu"];
        toolbar_children.extend(nodes.iter().map(|(path, _)| *path));
        let toolbar =
            Node { role: PANEL, ..Node::showing("Toolbar", app.references(&toolbar_children)) };
        let save = Node {
            role: PUSH_BUTTON,
            extents: Some((10, 20, 80, 24)),
            accessible_id: Some("save".to_owned()),
            actions: vec!["focus".to_owned(), "Click".to_owned()],
            states_after_action: Some(words(&[SHOWING, FOCUSED])),
            ..Node::showing("Save", vec![])
        };
        let save_as = Node { role: PUSH_BUTTON, ..Node::showing("Save As", vec![]) };
        // Not showing: read, and not entered.
        let menu = Node { states: Vec::new(), ..Node::showing("Menu", app.references(&["/item"])) };
        let mut objects = vec![
            root(app, &["/main"]),
            ("/main", window(FRAME, "Main", &[ACTIVE], app.references(&["/toolbar"]))),
            ("/toolbar", toolbar),
            ("/save", save),
            ("/save_as", save_as),
            ("/menu", menu),
            ("/item", Node::showing("Item", vec![])),
        ];
        objects.extend(nodes);
        objects
    });
    let hwnd = window_id(&world.object(app, "/main"));
    (app, hwnd)
}

/// The desktop of the product's settings, which has listed the windows once.
fn listed(world: &mut World) -> LinuxDesktop {
    let desktop = world.desktop(settings());
    assert!(!desktop.windows(true).unwrap().is_empty(), "the windows are listed");
    desktop
}

/// `node`, and the calls it will receive.
fn with_calls(node: Node) -> (Node, Calls) {
    let calls = node.calls.clone();
    (node, calls)
}

/// A selector of this test's process, with nothing else.
fn selector() -> Selector {
    Selector {
        process: this_process(),
        window_class: None,
        automation_id: None,
        control_type: None,
        name: None,
        ancestors: Vec::new(),
    }
}

#[test]
fn a_tree_lists_a_windows_elements_parents_first_within_the_limits() {
    let mut world = World::new();
    // It says it has a box, but serves no Component.
    let boxless = Node {
        interfaces: vec!["org.a11y.atspi.Component".to_owned()],
        ..Node::showing("Boxless", vec![])
    };
    let (app, hwnd) = editor(&mut world, vec![("/boxless", boxless)]);
    let desktop = listed(&mut world);
    let tree = desktop.tree(hwnd, TreeLimits::default(), Deadline::UIA).unwrap();
    let rows: Vec<_> =
        tree.nodes.iter().map(|node| (node.info.name.get().as_str(), node.parent)).collect();
    assert_eq!(
        rows,
        [
            ("Main", None),
            ("Toolbar", Some(0)),
            ("Save", Some(1)),
            ("Save As", Some(1)),
            ("Menu", Some(1)),
            ("Boxless", Some(1)),
        ],
        "breadth first, parents before children; the hidden menu's item is not entered"
    );
    let window = WindowRef { hwnd, pid: std::process::id() };
    let save = ElementInfo {
        control_type: "push button".to_owned(),
        name: Untrusted::new("Save".to_owned()),
        automation_id: Some(Untrusted::new("save".to_owned())),
        rect: Rect { x: 10, y: 20, w: 80, h: 24 },
        patterns: vec!["Action".to_owned(), "Component".to_owned()],
        is_password: false,
        window,
    };
    assert_eq!(tree.nodes[2].info, save, "the button as Grounding gives an element");
    assert_eq!(
        tree.nodes[2].handle,
        handle(hwnd, &world.object(app, "/save")),
        "its handle packs its object"
    );
    assert_eq!(tree.nodes[5].info.rect, Rect::default(), "a box that cannot be read is zero");
    assert!(tree.truncated, "and leaves the tree truncated");
    let again = desktop.tree(hwnd, TreeLimits::default(), Deadline::UIA).unwrap();
    assert_eq!(again.nodes[2].handle, tree.nodes[2].handle, "the same handle on every read");
    let small = desktop.tree(hwnd, TreeLimits { max_depth: 1, max_nodes: 500 }, Deadline::UIA);
    let small = small.unwrap();
    assert_eq!(small.nodes.len(), 2, "the window and the toolbar, at depth 1");
    assert!(small.truncated, "cut short by the depth");
    let narrow = desktop.tree(hwnd, TreeLimits { max_depth: 12, max_nodes: 3 }, Deadline::UIA);
    assert_eq!(narrow.unwrap().nodes.len(), 3, "at most 3 nodes");
}

#[test]
fn a_window_no_listing_has_seen_is_found_by_its_id() {
    let mut world = World::new();
    let (_app, hwnd) = editor(&mut world, Vec::new());
    let desktop = world.desktop(settings());
    assert_eq!(desktop.known_window(hwnd), None, "nothing listed yet");
    let tree = desktop.tree(hwnd, TreeLimits::default(), Deadline::UIA).unwrap();
    assert_eq!(tree.nodes[0].info.name.get(), "Main", "found by a listing in the call");
    assert_eq!(tree.nodes[0].info.window, WindowRef { hwnd, pid: std::process::id() }, "its pid");
    assert_eq!(
        desktop.tree(hwnd ^ 1, TreeLimits::default(), Deadline::UIA),
        Err(PlatformError::NotFound { what: "window" }),
        "an id no window has"
    );
    let found = desktop.find(
        hwnd,
        &Selector { name: Some(Untrusted::new("Save".to_owned())), ..selector() },
        Deadline::UIA,
    );
    assert_eq!(found.unwrap().len(), 2, "find lists the windows first, on the caller's thread");
}

#[test]
fn a_closed_window_is_not_found() {
    let mut world = World::new();
    let (app, hwnd) = editor(&mut world, Vec::new());
    let desktop = listed(&mut world);
    let save = handle(hwnd, &world.object(app, "/save"));
    world.close(app);
    assert!(desktop.known_window(hwnd).is_some(), "the last listing still knows it");
    let not_found = |what| PlatformError::NotFound { what };
    let tree = desktop.tree(hwnd, TreeLimits::default(), Deadline::UIA);
    assert_eq!(tree.unwrap_err(), not_found("window"), "its tree");
    let found = desktop.find(hwnd, &selector_named("Save"), Deadline::UIA);
    assert_eq!(found.unwrap_err(), not_found("window"), "a search in it");
    let invoked = desktop.invoke(&save, Deadline::UIA);
    assert_eq!(invoked.unwrap_err(), not_found("element"), "nor its elements");
    let set = desktop.set_value(&save, "x", Deadline::UIA);
    assert_eq!(set.unwrap_err(), not_found("element"), "for a write neither");
}

/// A selector of this test's process for elements named `name`.
fn selector_named(name: &str) -> Selector {
    Selector { name: Some(Untrusted::new(name.to_owned())), ..selector() }
}

#[test]
fn find_ranks_the_matches_best_first() {
    let mut world = World::new();
    let (app, hwnd) = editor(&mut world, Vec::new());
    let desktop = listed(&mut world);
    let found = desktop.find(hwnd, &selector_named("save"), Deadline::UIA).unwrap();
    let ranked: Vec<_> =
        found.iter().map(|found| (found.info.name.get().as_str(), found.score)).collect();
    assert_eq!(
        ranked,
        [("Save", 3), ("Save As", 1)],
        "a name in another case, then one holding it"
    );
    assert_eq!(found[0].handle, handle(hwnd, &world.object(app, "/save")), "the tree's handle");
    assert_eq!(found[0].info.rect, Rect { x: 10, y: 20, w: 80, h: 24 }, "with its box");
    let id = Some(Untrusted::new("save".to_owned()));
    assert_eq!(found[0].info.automation_id, id, "and its id, which the selector did not name");
    let toolbar = SelectorStep {
        control_type: Some("panel".to_owned()),
        name: Some(Untrusted::new("Toolbar".to_owned())),
        automation_id: None,
    };
    let exact = Selector {
        automation_id: Some(Untrusted::new("save".to_owned())),
        control_type: Some("push button".to_owned()),
        ancestors: vec![toolbar],
        ..selector_named("Save")
    };
    let found = desktop.find(hwnd, &exact, Deadline::UIA).unwrap();
    assert_eq!(found.len(), 1, "the automation id is required");
    assert_eq!(found[0].score, 8 + 2 + 4 + 1, "id, type, exact name and the toolbar step");
    assert_eq!(
        found[0].info.automation_id,
        Some(Untrusted::new("save".to_owned())),
        "read for the selector"
    );
    let framed = Selector { window_class: Some("frame".to_owned()), ..selector_named("Save") };
    assert_eq!(desktop.find(hwnd, &framed, Deadline::UIA).unwrap().len(), 2, "the class matches");
    let dialog = Selector { window_class: Some("dialog".to_owned()), ..selector_named("Save") };
    assert_eq!(desktop.find(hwnd, &dialog, Deadline::UIA), Ok(Vec::new()), "another class");
}

#[test]
fn find_in_another_process_finds_nothing() {
    let mut world = World::new();
    let mut calls = None;
    let app = world.app(|app| {
        let main = window(FRAME, "Main", &[ACTIVE], app.references(&["/child"]));
        calls = Some(main.calls.clone());
        vec![root(app, &["/main"]), ("/main", main), ("/child", Node::showing("Child", vec![]))]
    });
    let calls = calls.unwrap();
    let hwnd = window_id(&world.object(app, "/main"));
    let desktop = listed(&mut world);
    let elsewhere = Selector { process: "firefox".to_owned(), ..selector_named("Main") };
    assert_eq!(desktop.find(hwnd, &elsewhere, Deadline::UIA), Ok(Vec::new()), "another process");
    let unnamed = Selector { process: String::new(), ..selector_named("Main") };
    assert_eq!(desktop.find(hwnd, &unnamed, Deadline::UIA), Ok(Vec::new()), "no process");
    assert_eq!(count(&calls.listings), 0, "and the window's tree is never read");
    assert_eq!(desktop.find(hwnd, &selector_named("Main"), Deadline::UIA).unwrap().len(), 1);
}

#[test]
fn invoke_runs_the_click_action_and_reports_the_change() {
    let mut world = World::new();
    let refusing = Node {
        role: PUSH_BUTTON,
        actions: vec!["press".to_owned()],
        refuses_actions: true,
        ..Node::showing("Stuck", vec![])
    };
    let (refusing, refusing_calls) = with_calls(refusing);
    let (app, hwnd) = editor(&mut world, vec![("/stuck", refusing)]);
    let desktop = listed(&mut world);
    let tree = desktop.tree(hwnd, TreeLimits::default(), Deadline::UIA).unwrap();
    let save = tree.nodes[2].handle.clone();
    let verification = desktop.invoke(&save, Deadline::UIA).unwrap();
    assert_eq!(verification.kind, "action", "an action's check");
    assert_eq!(
        verification.observed.get(),
        &json!({ "action": "Click", "states_changed": true, "gone": false }),
        "the click, not the first action, and the focus it gave"
    );
    let stuck = handle(hwnd, &world.object(app, "/stuck"));
    assert_eq!(
        desktop.invoke(&stuck, Deadline::UIA),
        Err(PlatformError::Refused { what: "invoke", reason: "the app refused the action" }),
        "an action the app did not run"
    );
    assert_eq!(*refusing_calls.actions_done.lock().unwrap(), [0], "its press, its only action");
}

#[test]
fn invoke_without_an_action_is_refused() {
    let mut world = World::new();
    let named_only = Node {
        interfaces: vec!["org.a11y.atspi.Action".to_owned()],
        ..Node::showing("Odd", vec![])
    };
    let (app, hwnd) = editor(&mut world, vec![("/odd", named_only)]);
    let other = world.app(|app| vec![root(app, &["/w"]), ("/w", window(FRAME, "W", &[], vec![]))]);
    let desktop = listed(&mut world);
    let no_action =
        Err(PlatformError::Refused { what: "invoke", reason: "the element has no action" });
    let save_as = handle(hwnd, &world.object(app, "/save_as"));
    assert_eq!(desktop.invoke(&save_as, Deadline::UIA), no_action, "no Action");
    let odd = handle(hwnd, &world.object(app, "/odd"));
    assert_eq!(desktop.invoke(&odd, Deadline::UIA), no_action, "an Action with no action in it");
    let foreign = handle(hwnd, &world.object(other, "/w"));
    assert!(
        matches!(
            desktop.invoke(&foreign, Deadline::UIA),
            Err(PlatformError::Invalid { what: "element handle", .. })
        ),
        "an element of another app than its window's"
    );
    let forged = ElementHandle { hwnd, runtime_id: vec![1, 2, 3] };
    assert!(
        matches!(
            desktop.set_value(&forged, "x", Deadline::UIA),
            Err(PlatformError::Invalid { what: "element handle", .. })
        ),
        "a handle no read made"
    );
}

/// A text field holding `text`, in `states` besides showing, editable as `editable` says.
fn field(role: u32, states: &[u32], editable: bool) -> Node {
    Node {
        role,
        states: words(&[&[SHOWING], states].concat()),
        text: Some("old".to_owned()),
        editable,
        ..Node::default()
    }
}

#[test]
fn set_value_replaces_editable_text_and_reads_it_back() {
    let mut world = World::new();
    let (entry, calls) = with_calls(field(TEXT, &[EDITABLE], true));
    let refusing = Node { refuses_text: true, ..field(TEXT, &[EDITABLE], true) };
    let (app, hwnd) = editor(&mut world, vec![("/entry", entry), ("/refusing", refusing)]);
    let desktop = listed(&mut world);
    let entry = handle(hwnd, &world.object(app, "/entry"));
    let verification = desktop.set_value(&entry, "Dear \u{202E}Sam", Deadline::UIA).unwrap();
    assert_eq!(verification.kind, "value_read_back", "a read-back");
    assert_eq!(verification.observed.get(), &json!("Dear Sam"), "the new text, sanitized");
    assert_eq!(count(&calls.text_writes), 1, "one write");
    assert_eq!(calls.text_reads.lock().unwrap().last(), Some(&(0, 4_000)), "4,000 characters back");
    let refusing = handle(hwnd, &world.object(app, "/refusing"));
    assert_eq!(
        desktop.set_value(&refusing, "new", Deadline::UIA),
        Err(PlatformError::Refused { what: "set value", reason: "the app refused the text" }),
    );
}

#[test]
fn set_value_sets_a_ranges_number() {
    let mut world = World::new();
    let slider =
        Node { role: SLIDER, number: Some((5.0, 0.0, 10.0)), ..Node::showing("Volume", vec![]) };
    let (slider, calls) = with_calls(slider);
    let (app, hwnd) = editor(&mut world, vec![("/slider", slider)]);
    let desktop = listed(&mut world);
    let slider = handle(hwnd, &world.object(app, "/slider"));
    let set = |value| desktop.set_value(&slider, value, Deadline::UIA);
    assert_eq!(set(" 7.5 ").unwrap().observed.get(), &json!(7.5), "the number set and read back");
    assert_eq!(set("42").unwrap().observed.get(), &json!(10.0), "the number the app kept");
    for value in ["loud", "NaN", "inf"] {
        assert_eq!(
            set(value),
            Err(PlatformError::Invalid { what: "value", detail: "not a number".to_owned() }),
            "{value:?}"
        );
    }
    assert_eq!(count(&calls.value_writes), 2, "only numbers are written");
    let button = handle(hwnd, &world.object(app, "/save_as"));
    assert_eq!(
        desktop.set_value(&button, "1", Deadline::UIA),
        Err(PlatformError::Refused { what: "set value", reason: "the element takes no value" }),
    );
}

#[test]
fn set_value_refuses_password_and_read_only_fields_without_writing() {
    let mut world = World::new();
    let (password, password_calls) = with_calls(field(PASSWORD_TEXT, &[EDITABLE], true));
    let (locked, locked_calls) = with_calls(field(TEXT, &[EDITABLE, READ_ONLY], true));
    let (label, label_calls) = with_calls(field(TEXT, &[], true));
    let fixed = Node {
        role: SLIDER,
        states: words(&[SHOWING, READ_ONLY]),
        number: Some((1.0, 0.0, 10.0)),
        ..Node::default()
    };
    let (fixed, fixed_calls) = with_calls(fixed);
    let nodes =
        vec![("/password", password), ("/locked", locked), ("/label", label), ("/fixed", fixed)];
    let (app, hwnd) = editor(&mut world, nodes);
    let desktop = listed(&mut world);
    let refused = |reason| Err(PlatformError::Refused { what: "set value", reason });
    let set = |path| desktop.set_value(&handle(hwnd, &world.object(app, path)), "5", Deadline::UIA);
    assert_eq!(set("/password"), refused("a password field"), "a password field");
    assert_eq!(set("/locked"), refused("read-only"), "a read-only field");
    assert_eq!(set("/label"), refused("read-only"), "text that is not editable");
    assert_eq!(set("/fixed"), refused("read-only"), "a read-only number");
    for calls in [&password_calls, &locked_calls, &label_calls] {
        assert_eq!(count(&calls.text_writes), 0, "no text written");
        assert_eq!(count(&calls.text_uses), 0, "nor read");
    }
    assert_eq!(count(&fixed_calls.value_writes), 0, "no number written");
}

#[test]
fn a_silent_app_answers_deadline_exceeded_within_the_calls_deadline() {
    let mut world = World::new();
    let silent = Node { silent: true, ..Node::showing("Silent", vec![]) };
    let (app, hwnd) = editor(&mut world, vec![("/silent", silent)]);
    let desktop = listed(&mut world);
    let silent = handle(hwnd, &world.object(app, "/silent"));
    let within = |call: &'static str, result: Result<(), PlatformError>, started: Instant| {
        let waited = started.elapsed();
        assert_eq!(result, Err(PlatformError::DeadlineExceeded { call }), "{call}");
        assert!(
            waited >= SHORT.budget() && waited < eileen_desktop_linux::CALL_TIMEOUT,
            "{call} within its deadline, not the app's call timeout: {waited:?}"
        );
    };
    let started = Instant::now();
    let tree = desktop.tree(hwnd, TreeLimits::default(), SHORT).map(drop);
    within("element tree", tree, started);
    let started = Instant::now();
    within("find elements", desktop.find(hwnd, &selector_named("Save"), SHORT).map(drop), started);
    let started = Instant::now();
    within("invoke", desktop.invoke(&silent, SHORT).map(drop), started);
    let started = Instant::now();
    within("set value", desktop.set_value(&silent, "x", SHORT).map(drop), started);
    let quick = LinuxDesktopSettings { call_timeout: Duration::from_millis(50), ..settings() };
    let tree = world.desktop(quick).tree(hwnd, TreeLimits::default(), Deadline::UIA).unwrap();
    assert!(tree.truncated, "with a short call timeout, the silent element is left out");
}

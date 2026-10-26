//! `eileen-platform`'s `Automation` for [`LinuxDesktop`]: AT-SPI2's routes to what UI Automation
//! gives on Windows, each call one bridge job as `src/adapter.rs` says.
//!
//! - `tree` and `find`: a walk of the window's tree; an element's handle packs its object
//!   (`src/handle.rs`), and `find` ranks the walk (`src/find.rs`).
//! - `invoke`: the element's `Action`.
//! - `set_value`: its `EditableText`, else its `Value`, read back after.
//!
//! A window is the one the last listing found under its id, else the one a listing made in the
//! call finds, since a window's id is the same in every process. An element must be of its
//! window's app: a handle whose object is another app's is refused before any call to it, so a
//! rule that reads the window's process holds for the element. AT-SPI's actions and writes reach
//! an app in the background: no call takes the foreground.

use eileen_contracts::core_io::Verification;
use eileen_contracts::desktop::{Selector, WindowRef};
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::DbusError;
use eileen_platform::{
    Automation, Deadline, ElementCandidate, ElementHandle, ElementNode, ElementTree, PlatformError,
    TreeLimits,
};
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::LinuxDesktop;
use crate::atspi::{Accessible, Atspi, Interfaces, ObjectRef, Role, State, Walk};
use crate::element::{info_of, rect_of, role_name, search_budget};
use crate::find::{needs_ids, rank};
use crate::handle::{handle_of, object_of};
use crate::toplevel::{KnownWindow, toplevels, window_id};

/// What the calls' errors name.
const TREE: &str = "element tree";
const INVOKE: &str = "invoke";
const SET_VALUE: &str = "set value";
const FIND: &str = "find elements";
const WINDOW: &str = "window";
const ELEMENT: &str = "element";
const VALUE: &str = "value";
/// The most of an element's actions whose names are read.
const MOST_ACTIONS: u32 = 16;
/// The names of the actions that invoke an element, in any case: GTK's `click` and
/// `activate`, Qt's `Press` and `Toggle`, a link's `jump`, Chromium's and Firefox's `press`,
/// `open` and `select`. An element with none of them runs its first action.
const INVOKING: [&str; 7] = ["click", "press", "activate", "jump", "toggle", "open", "select"];
/// The most characters of a text read back.
const TEXT_CHARS: usize = 4_000;

impl Automation for LinuxDesktop {
    /// The window's tree, read as `Atspi::walk` reads it within `limits` for four fifths of
    /// `deadline`, parents first. Each element's handle packs its object, its info is
    /// `Grounding`'s (role name, name, automation id, window-relative box, interfaces), and a box
    /// that cannot be read is all zero and leaves the tree `truncated`. A window no listing finds,
    /// or one whose app has gone, is `NotFound { what: "window" }`.
    fn tree(
        &self,
        hwnd: i64,
        limits: TreeLimits,
        deadline: Deadline,
    ) -> Result<ElementTree, PlatformError> {
        let known = self.known_window(hwnd);
        self.on_bus(TREE, deadline, move |atspi| async move {
            let (window, at) = locate(&atspi, known, hwnd, TREE).await?;
            let until = Instant::now() + search_budget(deadline.budget());
            let walk = atspi.walk(&window, limits, until).await.map_err(gone(WINDOW))?;
            let mut tree = ElementTree { nodes: Vec::new(), truncated: walk.truncated };
            for node in walk.nodes {
                let element = &node.accessible;
                let (rect, id) =
                    tokio::join!(rect_of(&atspi, element), atspi.accessible_id(&element.object));
                tree.truncated |= rect.is_err();
                let info = info_of(element, rect.unwrap_or_default(), id.ok().flatten(), at);
                let handle = handle_of(hwnd, &element.object);
                tree.nodes.push(ElementNode { handle, parent: node.parent, info });
            }
            Ok(tree)
        })
    }

    /// Runs the element's action that invokes it: among its first 16 actions, the first named
    /// as [`INVOKING`] lists, else its first. It answers `Verification { kind: "action" }`,
    /// whose `observed` names the action (as the app named it, sanitized), whether the element's
    /// states changed and whether it has gone; `null` where its read after the action failed for
    /// another reason. An element without `Action`, or with no action, is `Refused`, as is an
    /// action the app says it did not run.
    fn invoke(
        &self,
        element: &ElementHandle,
        deadline: Deadline,
    ) -> Result<Verification, PlatformError> {
        let (hwnd, object) = (element.hwnd, object_of(element)?);
        let known = self.known_window(hwnd);
        self.on_bus(INVOKE, deadline, move |atspi| async move {
            let refused = |reason| PlatformError::Refused { what: INVOKE, reason };
            let before = facts(&atspi, known, hwnd, &object, INVOKE).await?;
            if !before.interfaces.contains(Interfaces::ACTION) {
                return Err(refused("the element has no action"));
            }
            let names = atspi.actions(&object, MOST_ACTIONS).await.map_err(gone(ELEMENT))?;
            let Some((index, action)) = invoking(&names) else {
                return Err(refused("the element has no action"));
            };
            if !atspi.do_action(&object, index).await.map_err(gone(ELEMENT))? {
                return Err(refused("the app refused the action"));
            }
            let observed = seen(action, &before, atspi.accessible(&object).await);
            Ok(Verification { kind: "action".to_owned(), observed: Untrusted::new(observed) })
        })
    }

    /// Sets the element's value and reads it back, `Verification { kind: "value_read_back" }`.
    ///
    /// - A password field is `Refused` before any write, and so is a read-only one: `READ_ONLY`,
    ///   or text that is not `EDITABLE`.
    /// - With `EditableText`, `SetTextContents` replaces its whole text, and the first 4,000
    ///   characters of the text are read back.
    /// - Else with `Value`, `value` is its number (`Invalid` when it is not a finite one), set as
    ///   `CurrentValue`; the number the app kept is read back, as it may keep it in its range.
    /// - Else `Refused`: the element takes no value.
    fn set_value(
        &self,
        element: &ElementHandle,
        value: &str,
        deadline: Deadline,
    ) -> Result<Verification, PlatformError> {
        let (hwnd, object) = (element.hwnd, object_of(element)?);
        let (known, value) = (self.known_window(hwnd), value.to_owned());
        self.on_bus(SET_VALUE, deadline, move |atspi| async move {
            let refused = |reason| PlatformError::Refused { what: SET_VALUE, reason };
            let facts = facts(&atspi, known, hwnd, &object, SET_VALUE).await?;
            if facts.role == Role::PASSWORD_TEXT {
                return Err(refused("a password field"));
            }
            let text = facts.interfaces.contains(Interfaces::TEXT)
                || facts.interfaces.contains(Interfaces::EDITABLE_TEXT);
            let editable = facts.states.contains(State::EDITABLE);
            if facts.states.contains(State::READ_ONLY) || (text && !editable) {
                return Err(refused("read-only"));
            }
            let read_back = |observed| Verification {
                kind: "value_read_back".to_owned(),
                observed: Untrusted::new(observed),
            };
            if facts.interfaces.contains(Interfaces::EDITABLE_TEXT) {
                if !atspi.set_text(&object, &value).await.map_err(gone(ELEMENT))? {
                    return Err(refused("the app refused the text"));
                }
                let read = atspi.text(&object, TEXT_CHARS).await.map_err(gone(ELEMENT))?;
                return Ok(read_back(Value::String(read.into_inner())));
            }
            if facts.interfaces.contains(Interfaces::VALUE) {
                let number = value.trim().parse::<f64>().ok().filter(|number| number.is_finite());
                let not_a_number =
                    || PlatformError::Invalid { what: VALUE, detail: "not a number".to_owned() };
                atspi
                    .set_number(&object, number.ok_or_else(not_a_number)?)
                    .await
                    .map_err(gone(ELEMENT))?;
                let kept = atspi.number(&object).await.map_err(gone(ELEMENT))?;
                return Ok(read_back(json!(kept)));
            }
            Err(refused("the element takes no value"))
        })
    }

    /// The window's elements `selector` matches, best first, at most 16, scored as
    /// `src/find.rs` says, within the settings' find limits and four fifths of what is left of
    /// `deadline`. None when the window's process is not `selector.process` (an unknown process
    /// matches no selector) or, when given, its class is not `selector.window_class`. A window
    /// the last listing did not find is looked for by a listing within `deadline` first, on the
    /// caller's thread, which names the processes.
    fn find(
        &self,
        hwnd: i64,
        selector: &Selector,
        deadline: Deadline,
    ) -> Result<Vec<ElementCandidate>, PlatformError> {
        let started = Instant::now();
        let known = if let Some(known) = self.known_window(hwnd) {
            known
        } else {
            let listed = self.list(FIND, deadline)?;
            match self.known_window(hwnd) {
                Some(known) => known,
                None if listed.unread => {
                    return Err(PlatformError::DeadlineExceeded { call: FIND });
                }
                None => return Err(PlatformError::NotFound { what: WINDOW }),
            }
        };
        if known.process.is_empty() || known.process != selector.process {
            return Ok(Vec::new());
        }
        let left = Deadline::after(deadline.budget().saturating_sub(started.elapsed()));
        let (limits, selector) = (self.settings().find_limits, selector.clone());
        self.on_bus(FIND, left, move |atspi| async move {
            let until = Instant::now() + search_budget(left.budget());
            let walk = atspi.walk(&known.object, limits, until).await.map_err(gone(WINDOW))?;
            let class = walk.nodes.first().map(|window| role_name(window.accessible.role));
            if selector.window_class.as_deref().is_some_and(|wanted| class != Some(wanted)) {
                return Ok(Vec::new());
            }
            let ids =
                if needs_ids(&selector) { ids_of(&atspi, &walk, until).await } else { Vec::new() };
            let at = WindowRef { hwnd, pid: known.pid };
            let mut candidates = Vec::new();
            for (index, score) in rank(&walk, &ids, &selector) {
                let Some(node) = walk.nodes.get(index) else { continue };
                let rect = rect_of(&atspi, &node.accessible).await.unwrap_or_default();
                let id = ids.get(index).cloned().flatten();
                let info = info_of(&node.accessible, rect, id, at);
                let handle = handle_of(hwnd, &node.accessible.object);
                candidates.push(ElementCandidate { handle, info, score });
            }
            Ok(candidates)
        })
    }
}

/// The window `hwnd` names, its object and its reference: `known`, which the last listing
/// found, else the window a listing finds now. Not found, it is `NotFound`, or
/// `DeadlineExceeded` naming `call` while an app was left unread, as the window may be its.
async fn locate(
    atspi: &Atspi,
    known: Option<KnownWindow>,
    hwnd: i64,
    call: &'static str,
) -> Result<(ObjectRef, WindowRef), PlatformError> {
    if let Some(known) = known {
        return Ok((known.object, WindowRef { hwnd, pid: known.pid }));
    }
    let found = toplevels(atspi).await?;
    let window = found.windows.into_iter().find(|window| window_id(&window.window.object) == hwnd);
    match window {
        Some(window) => Ok((window.window.object, WindowRef { hwnd, pid: window.pid })),
        None if found.unread => Err(PlatformError::DeadlineExceeded { call }),
        None => Err(PlatformError::NotFound { what: WINDOW }),
    }
}

/// What `object`, an element of window `hwnd`, is, once it is shown to be of the window's app;
/// one of another app is `Invalid`, before any call to it.
async fn facts(
    atspi: &Atspi,
    known: Option<KnownWindow>,
    hwnd: i64,
    object: &ObjectRef,
    call: &'static str,
) -> Result<Accessible, PlatformError> {
    let (window, _at) = locate(atspi, known, hwnd, call).await?;
    if window.bus != object.bus {
        let detail = "an element of another app than its window's".to_owned();
        return Err(PlatformError::Invalid { what: "element handle", detail });
    }
    atspi.accessible(object).await.map_err(gone(ELEMENT))
}

/// A failure, with `NotFound` naming `what`: the window or the element, gone or never served.
fn gone(what: &'static str) -> impl Fn(DbusError) -> PlatformError {
    move |error| match error {
        DbusError::NotFound { .. } => PlatformError::NotFound { what },
        error @ (DbusError::Unavailable { .. }
        | DbusError::DeadlineExceeded { .. }
        | DbusError::Refused { .. }
        | DbusError::Invalid { .. }
        | DbusError::Io { .. }) => error.into(),
    }
}

/// The index and name of the action among `names` that invokes an element: the first named as
/// [`INVOKING`] lists, in any case, else the first; `None` without actions.
fn invoking(names: &[Untrusted<String>]) -> Option<(i32, &Untrusted<String>)> {
    let invokes = |name: &Untrusted<String>| {
        INVOKING.iter().any(|invoking| name.get().eq_ignore_ascii_case(invoking))
    };
    let index = names.iter().position(invokes).unwrap_or(0);
    Some((i32::try_from(index).ok()?, names.get(index)?))
}

/// What an action's verification observed: its name, and from the element's read `after` it,
/// whether its states changed since `before` and whether it has gone.
fn seen(
    action: &Untrusted<String>,
    before: &Accessible,
    after: Result<Accessible, DbusError>,
) -> Value {
    let (changed, gone) = match after {
        Ok(after) => (
            Value::Bool(after.states != before.states),
            Value::Bool(after.states.contains(State::DEFUNCT)),
        ),
        Err(DbusError::NotFound { .. }) => (Value::Null, Value::Bool(true)),
        Err(_) => (Value::Null, Value::Null),
    };
    json!({ "action": action.get(), "states_changed": changed, "gone": gone })
}

/// The automation id of each node of `walk`, by index, read one after another until `until`:
/// a node not read by then, or whose id cannot be read, has none.
async fn ids_of(atspi: &Atspi, walk: &Walk, until: Instant) -> Vec<Option<Untrusted<String>>> {
    let mut ids = Vec::new();
    for node in &walk.nodes {
        if Instant::now() >= until {
            break;
        }
        ids.push(atspi.accessible_id(&node.accessible.object).await.ok().flatten());
    }
    ids
}

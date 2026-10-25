//! The tray through `LinuxShell::set_tray` on a bridge to a private bus: the item and its menu as
//! a host reads them, with a connection of the test's playing the host, and their registration
//! with a fake watcher that reports each one. The rig's items are `pub(super)` for the shell's
//! tests. The test's thread calls the shell as the core does, blocking; the fakes and the host run
//! on a runtime of their own.

use std::collections::HashMap;
use std::sync::Arc;

use eileen_dbus_linux::testing::{PrivateBus, nothing_ready, round_trip, serve};
use eileen_dbus_linux::{
    Bridge, DEFAULT_DEADLINE, Names, Signals, name_owner, owner_changes, subscribe,
};
use eileen_platform::ShellHost;
use eileen_platform::{TrayItem, TrayState};
use eileen_shell_linux::{LinuxShell, LinuxShellEvent, LinuxShellSettings, TraySettings};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{MatchRule, Message};

use crate::shell_host::{DESKTOP_ENTRY, settings};

/// The watcher's name, which is also its interface's, and its object.
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// The item's object and interface, and the menu's.
pub(super) const ITEM_PATH: &str = "/StatusNotifierItem";
pub(super) const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const MENU: &str = "com.canonical.dbusmenu";
/// The name of the error an unknown menu id is answered with.
pub(super) const INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";

/// An icon as pixels, `a(iiay)`.
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
/// A node as `GetLayout` sends it, `(ia{sv}av)`.
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
/// One registration: the service the item gave, and the unique name it came from.
pub(super) type Registration = (String, String);

/// A watcher that reports each `RegisterStatusNotifierItem`.
#[derive(Debug, Clone)]
struct FakeWatcher(mpsc::Sender<Registration>);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl FakeWatcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
    ) {
        let sender = header.sender().unwrap().to_string();
        let _ignored = self.0.send((service.to_owned(), sender)).await;
    }
}

/// A node of the menu's layout as the host reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) id: i32,
    /// Its `label`, if it has one.
    pub(super) label: Option<String>,
    /// Its `toggle-state`, which only a `checkmark` toggle has.
    pub(super) check: Option<i32>,
    pub(super) children: Vec<Self>,
}

impl Node {
    /// The node `value` holds, `(ia{sv}av)`.
    fn of(value: OwnedValue) -> Self {
        let (id, properties, children) = Layout::try_from(value).unwrap();
        let text = |name: &str| properties.get(name).map(|v| String::try_from(v.clone()).unwrap());
        let check = properties.get("toggle-state").map(|v| i32::try_from(v.clone()).unwrap());
        let toggle = text("toggle-type");
        assert_eq!(
            toggle.is_some(),
            check.is_some(),
            "a state comes with its toggle: {properties:?}"
        );
        assert!(toggle.is_none_or(|toggle| toggle == "checkmark"), "a checkmark toggle");
        let children = children.into_iter().map(Self::of).collect();
        Self { id, label: text("label"), check, children }
    }

    /// The labels of the node's children.
    pub(super) fn labels(&self) -> Vec<&str> {
        self.children.iter().map(|child| child.label.as_deref().unwrap_or_default()).collect()
    }
}

/// A tray test's world: a private bus, a runtime for the fakes and the host, the host's
/// connection, and the watchers being served.
pub(super) struct Rig {
    pub(super) runtime: Runtime,
    host: Option<zbus::Connection>,
    watchers: Vec<zbus::Connection>,
    pub(super) bus: PrivateBus,
}

impl Drop for Rig {
    fn drop(&mut self) {
        // The connections end inside their runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.watchers.clear();
        self.host = None;
    }
}

impl Rig {
    /// A private bus with nothing on it yet but the host's connection.
    pub(super) fn new() -> Self {
        let mut runtime = tokio::runtime::Builder::new_multi_thread();
        let runtime = runtime.worker_threads(2).enable_all().build().unwrap();
        let bus = PrivateBus::start();
        let host = runtime.block_on(bus.connect());
        Self { runtime, host: Some(host), watchers: Vec::new(), bus }
    }

    /// The host's connection.
    pub(super) fn host(&self) -> &zbus::Connection {
        self.host.as_ref().unwrap()
    }

    /// Starts a watcher, and answers what it reports.
    pub(super) fn watcher(&mut self) -> mpsc::Receiver<Registration> {
        let (registrations, reported) = mpsc::channel(8);
        let watcher = FakeWatcher(registrations);
        self.watchers.push(self.runtime.block_on(serve(&self.bus, WATCHER, WATCHER_PATH, watcher)));
        reported
    }

    /// Takes every watcher off the bus.
    pub(super) fn stop_watchers(&mut self) {
        let _context = self.runtime.enter();
        self.watchers.clear();
    }

    /// A shell on a bridge to the bus with `settings`, whose events reach the answered channel.
    pub(super) fn shell_with(
        &self,
        settings: LinuxShellSettings,
    ) -> (LinuxShell, mpsc::Receiver<LinuxShellEvent>) {
        shell_on(Bridge::on_private_bus(&self.bus).unwrap(), settings)
    }

    /// A shell on a bridge to the bus with the tests' settings and the default icons.
    pub(super) fn shell(&self) -> (LinuxShell, mpsc::Receiver<LinuxShellEvent>) {
        self.shell_with(settings(DEFAULT_DEADLINE, None))
    }

    /// The next registration `registrations` reports.
    pub(super) fn registered(
        &self,
        registrations: &mut mpsc::Receiver<Registration>,
    ) -> Registration {
        self.runtime.block_on(registrations.recv()).unwrap()
    }

    /// The next event `events` brings.
    pub(super) fn event(&self, events: &mut mpsc::Receiver<LinuxShellEvent>) -> LinuxShellEvent {
        self.runtime.block_on(events.recv()).unwrap()
    }

    /// The property `name` of `interface` at `path` of the service `item`.
    pub(super) fn property(
        &self,
        item: &str,
        path: &str,
        interface: &str,
        name: &str,
    ) -> OwnedValue {
        let properties = Some("org.freedesktop.DBus.Properties");
        let asked = (interface, name);
        let get = self.host().call_method(Some(item), path, properties, "Get", &asked);
        let reply = self.runtime.block_on(get).unwrap();
        reply.body().deserialize().unwrap()
    }

    /// A method's reply from the menu of `item`.
    pub(super) fn menu_call<B>(&self, item: &str, method: &str, body: &B) -> zbus::Result<Message>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.runtime.block_on(self.host().call_method(
            Some(item),
            MENU_PATH,
            Some(MENU),
            method,
            body,
        ))
    }

    /// The menu's revision and the node `parent` of its layout, `depth` levels of children down.
    pub(super) fn layout(&self, item: &str, parent: i32, depth: i32) -> (u32, Node) {
        let reply = self.menu_call(item, "GetLayout", &(parent, depth, Vec::<String>::new()));
        let (revision, node): (u32, Layout) = reply.unwrap().body().deserialize().unwrap();
        let value = zbus::zvariant::Value::from(node);
        (revision, Node::of(value.try_into_owned().unwrap()))
    }

    /// Sends the menu of `item` the event `event` on the node `id`.
    pub(super) fn menu_event(&self, item: &str, id: i32, event: &str) -> zbus::Result<()> {
        let data = OwnedValue::from(0_i32);
        self.menu_call(item, "Event", &(id, event, data, 0_u32)).map(drop)
    }

    /// The signals `member` from `interface` at `path` of the service `item`, from now on.
    pub(super) fn signals(&self, item: &str, path: &str, interface: &str, member: &str) -> Signals {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(item)
            .and_then(|rule| rule.path(path))
            .and_then(|rule| rule.interface(interface))
            .and_then(|rule| rule.member(member))
            .map(zbus::match_rule::Builder::build);
        self.runtime.block_on(subscribe(self.host(), rule, "test signals")).unwrap()
    }

    /// Whether `signals` has nothing more from `item` after a round trip to it.
    pub(super) fn nothing_more(&self, signals: &mut Signals, item: &str) -> bool {
        self.runtime.block_on(round_trip(self.host(), Some(item)));
        nothing_ready(signals)
    }
}

/// A shell on `bridge` with `settings`, whose events reach the answered channel.
fn shell_on(
    bridge: Bridge,
    settings: LinuxShellSettings,
) -> (LinuxShell, mpsc::Receiver<LinuxShellEvent>) {
    let (events, received) = mpsc::channel(64);
    let sink = move |event| {
        let _ignored = events.try_send(event);
    };
    (LinuxShell::new(bridge, settings).with_events(Arc::new(sink)), received)
}

/// The name of the error `result` holds; `None` for an answer.
pub(super) fn error_name<T>(result: zbus::Result<T>) -> Option<String> {
    match result {
        Ok(_answer) => None,
        Err(zbus::Error::MethodError(name, _, _)) => Some(name.to_string()),
        Err(error) => panic!("an error reply: {error}"),
    }
}

/// An item with this id and label, and `checked`.
pub(super) fn tray_item(id: &str, label: &str, checked: Option<bool>) -> TrayItem {
    TrayItem { id: id.to_owned(), label: label.to_owned(), checked }
}

/// The tests' tray: unmuted, a tooltip, and a mute toggle and a quit item.
pub(super) fn state() -> TrayState {
    TrayState {
        muted: false,
        tooltip: "Listening".to_owned(),
        menu: vec![tray_item("mute", "Mute", Some(false)), tray_item("quit", "Quit", None)],
    }
}

/// The default icon while the microphone is on, and while it is muted.
fn icons() -> (String, String) {
    let TraySettings { icon, muted_icon } = TraySettings::default();
    (icon, muted_icon)
}

#[test]
fn the_item_shows_the_state_and_registers_with_the_watcher() {
    let mut rig = Rig::new();
    let mut registrations = rig.watcher();
    let (shell, _events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, sender) = rig.registered(&mut registrations);
    let prefix = format!("org.kde.StatusNotifierItem-{}-", std::process::id());
    let n = item.strip_prefix(&prefix).unwrap_or_else(|| panic!("the item's name: {item}"));
    assert!(n.parse::<u32>().is_ok(), "a number after the process's ID: {item}");
    let owner = rig.runtime.block_on(name_owner(rig.host(), &item)).unwrap().unwrap();
    assert_eq!(owner.as_str(), sender, "the name is the registering connection's");
    let text = |name: &str| String::try_from(rig.property(&item, ITEM_PATH, ITEM, name)).unwrap();
    assert_eq!(text("Category"), "ApplicationStatus", "an application's status");
    assert_eq!(text("Id"), DESKTOP_ENTRY, "the settings' desktop entry");
    assert_eq!(text("Title"), "Eileen", "Eileen");
    assert_eq!(text("Status"), "Active", "shown");
    let (icon, _muted) = icons();
    assert_eq!(text("IconName"), icon, "the microphone on");
    for empty in ["IconThemePath", "OverlayIconName", "AttentionIconName", "AttentionMovieName"] {
        assert_eq!(text(empty), "", "{empty}");
    }
    let pixmaps = Pixmaps::try_from(rig.property(&item, ITEM_PATH, ITEM, "IconPixmap")).unwrap();
    assert!(pixmaps.is_empty(), "no pixmap");
    let tooltip = rig.property(&item, ITEM_PATH, ITEM, "ToolTip");
    let tooltip = <(String, Pixmaps, String, String)>::try_from(tooltip).unwrap();
    assert_eq!(tooltip, (icon, Vec::new(), "Eileen".to_owned(), "Listening".to_owned()), "tooltip");
    let menu = OwnedObjectPath::try_from(rig.property(&item, ITEM_PATH, ITEM, "Menu")).unwrap();
    assert_eq!(menu.as_str(), MENU_PATH, "the menu's object");
    let is_menu = bool::try_from(rig.property(&item, ITEM_PATH, ITEM, "ItemIsMenu")).unwrap();
    assert!(is_menu, "a click opens the menu");
    let version = u32::try_from(rig.property(&item, MENU_PATH, MENU, "Version")).unwrap();
    assert_eq!(version, 3, "dbusmenu's version");
    let (_revision, root) = rig.layout(&item, 0, -1);
    assert_eq!(root.labels(), ["Mute", "Quit"], "the menu, top to bottom");
    assert_eq!(root.children[0].check, Some(0), "the toggle, off");
    assert_eq!(root.children[1].check, None, "a plain item");
    for (method, args) in
        [("Activate", (0, 0)), ("SecondaryActivate", (0, 0)), ("ContextMenu", (0, 0))]
    {
        let call =
            rig.host().call_method(Some(item.as_str()), ITEM_PATH, Some(ITEM), method, &args);
        rig.runtime.block_on(call).unwrap_or_else(|error| panic!("{method}: {error}"));
    }
}

#[test]
fn a_clicked_item_reaches_the_sink_by_its_id_and_a_stale_one_is_an_id_error() {
    let mut rig = Rig::new();
    let mut registrations = rig.watcher();
    let (shell, mut events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, _sender) = rig.registered(&mut registrations);
    let (_revision, root) = rig.layout(&item, 0, -1);
    let [mute, quit] = [root.children[0].id, root.children[1].id];
    rig.menu_event(&item, quit, "clicked").unwrap();
    let choice = |id: &str| LinuxShellEvent::TrayChoice(id.to_owned());
    assert_eq!(rig.event(&mut events), choice("quit"), "chosen, by its TrayItem id");
    let menu = vec![tray_item("mute", "Mute", Some(true)), tray_item("about", "About", None)];
    shell.set_tray(TrayState { menu, ..state() }).unwrap();
    let stale = rig.menu_event(&item, quit, "clicked");
    assert_eq!(error_name(stale).as_deref(), Some(INVALID_ARGS), "an item shown no more");
    let click = |id: i32| (id, "clicked", OwnedValue::from(0_i32), 0_u32);
    let group = rig.menu_call(&item, "EventGroup", &(vec![click(mute), click(quit)],));
    let errors: Vec<i32> = group.unwrap().body().deserialize().unwrap();
    assert_eq!(errors, [quit], "the stale id, as an id error");
    assert_eq!(rig.event(&mut events), choice("mute"), "the known one, chosen");
    let none_known = rig.menu_call(&item, "EventGroup", &(vec![click(quit)],)).map(drop);
    assert_eq!(error_name(none_known).as_deref(), Some(INVALID_ARGS), "no known id in the group");
    rig.menu_event(&item, 0, "opened").unwrap();
    rig.menu_event(&item, mute, "hovered").unwrap();
    let (_revision, root) = rig.layout(&item, 0, -1);
    rig.menu_event(&item, root.children[1].id, "clicked").unwrap();
    assert_eq!(rig.event(&mut events), choice("about"), "only a click is a choice");
}

#[test]
fn the_menu_answers_for_its_items_and_never_asks_to_update() {
    let mut rig = Rig::new();
    let mut registrations = rig.watcher();
    let (shell, _events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, _sender) = rig.registered(&mut registrations);
    let (_revision, root) = rig.layout(&item, 0, -1);
    let [mute, quit] = [root.children[0].id, root.children[1].id];
    let label = vec!["label".to_owned()];
    let labels = |ids: Vec<i32>| -> Vec<(i32, Option<String>)> {
        let reply = rig.menu_call(&item, "GetGroupProperties", &(ids, &label)).unwrap();
        let groups: Vec<(i32, HashMap<String, OwnedValue>)> = reply.body().deserialize().unwrap();
        let text = |value: &OwnedValue| String::try_from(value.clone()).unwrap();
        groups.into_iter().map(|(id, found)| (id, found.get("label").map(text))).collect()
    };
    assert_eq!(labels(vec![quit, 99]), [(quit, Some("Quit".to_owned()))], "known ids alone");
    let all = [(0, None), (mute, Some("Mute".to_owned())), (quit, Some("Quit".to_owned()))];
    assert_eq!(labels(Vec::new()), all, "no ids: the root and every item");
    let property = |id: i32, name: &str| {
        let reply = rig.menu_call(&item, "GetProperty", &(id, name))?;
        Ok(String::try_from(reply.body().deserialize::<OwnedValue>()?)?)
    };
    assert_eq!(property(quit, "label").unwrap(), "Quit", "one property");
    assert_eq!(error_name(property(99, "label")).as_deref(), Some(INVALID_ARGS), "unknown item");
    let missing = property(quit, "no-such-property");
    assert_eq!(error_name(missing).as_deref(), Some(INVALID_ARGS), "unknown property");
    let reply = rig.menu_call(&item, "AboutToShow", &(0_i32,)).unwrap();
    assert!(!reply.body().deserialize::<bool>().unwrap(), "no update needed");
    let reply = rig.menu_call(&item, "AboutToShowGroup", &(vec![0_i32, quit, 99],)).unwrap();
    let (updates, errors): (Vec<i32>, Vec<i32>) = reply.body().deserialize().unwrap();
    assert_eq!((updates, errors), (Vec::new(), vec![99]), "no update, and the unknown id");
}

#[test]
fn a_menu_change_signals_a_new_revision() {
    let mut rig = Rig::new();
    // The test's streams drop inside the runtime's context, a panicking test's included.
    let runtime = rig.runtime.handle().clone();
    let _context = runtime.enter();
    let mut registrations = rig.watcher();
    let (shell, _events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, _sender) = rig.registered(&mut registrations);
    let (before, _root) = rig.layout(&item, 0, -1);
    let mut updates = rig.signals(&item, MENU_PATH, MENU, "LayoutUpdated");
    let menu = vec![tray_item("mute", "Unmute", Some(true)), tray_item("about", "About", None)];
    shell.set_tray(TrayState { menu, ..state() }).unwrap();
    let update = rig.runtime.block_on(updates.next()).unwrap();
    let (revision, parent): (u32, i32) = update.body().deserialize().unwrap();
    assert_eq!((revision, parent), (before + 1, 0), "a new revision, from the root");
    let (now, root) = rig.layout(&item, 0, -1);
    assert_eq!(now, revision, "the layout's revision");
    assert_eq!(root.labels(), ["Unmute", "About"], "the new labels");
    assert_eq!(root.children.iter().map(|node| node.check).collect::<Vec<_>>(), [Some(1), None]);
    let (_now, alone) = rig.layout(&item, 0, 0);
    assert!(alone.children.is_empty(), "depth 0: the root alone");
    let menu = vec![tray_item("mute", "Unmute", Some(true)), tray_item("about", "About", None)];
    shell.set_tray(TrayState { menu, tooltip: "Muted".to_owned(), ..state() }).unwrap();
    assert!(rig.nothing_more(&mut updates, &item), "the same menu, no update");
}

#[test]
fn a_new_icon_or_tooltip_is_signalled() {
    let mut rig = Rig::new();
    // The test's streams drop inside the runtime's context, a panicking test's included.
    let runtime = rig.runtime.handle().clone();
    let _context = runtime.enter();
    let mut registrations = rig.watcher();
    let (shell, _events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, _sender) = rig.registered(&mut registrations);
    let mut icons_now = rig.signals(&item, ITEM_PATH, ITEM, "NewIcon");
    let mut tooltips = rig.signals(&item, ITEM_PATH, ITEM, "NewToolTip");
    shell.set_tray(TrayState { muted: true, ..state() }).unwrap();
    let text = |name: &str| String::try_from(rig.property(&item, ITEM_PATH, ITEM, name)).unwrap();
    rig.runtime.block_on(icons_now.next()).unwrap();
    rig.runtime.block_on(tooltips.next()).unwrap();
    let (_icon, muted) = icons();
    assert_eq!(text("IconName"), muted, "muted");
    shell.set_tray(TrayState { muted: true, tooltip: "Muted".to_owned(), ..state() }).unwrap();
    rig.runtime.block_on(tooltips.next()).unwrap();
    assert!(rig.nothing_more(&mut icons_now, &item), "the same icon, no NewIcon");
    shell.set_tray(TrayState { muted: true, tooltip: "Muted".to_owned(), ..state() }).unwrap();
    assert!(rig.nothing_more(&mut tooltips, &item), "the same tooltip, no NewToolTip");
}

#[test]
fn a_watcher_that_restarts_registers_the_item_again() {
    let mut rig = Rig::new();
    let mut first = rig.watcher();
    let (shell, _events) = rig.shell();
    shell.set_tray(state()).unwrap();
    let (item, sender) = rig.registered(&mut first);
    rig.stop_watchers();
    let mut second = rig.watcher();
    assert_eq!(rig.registered(&mut second), (item, sender), "the same item, registered again");
    assert_eq!(shell.set_tray(state()), Ok(()), "a watcher has it");
}

#[test]
fn the_item_leaves_the_bus_when_the_shell_drops() {
    let mut rig = Rig::new();
    // The test's streams drop inside the runtime's context, a panicking test's included.
    let runtime = rig.runtime.handle().clone();
    let _context = runtime.enter();
    let mut registrations = rig.watcher();
    // A clone of the bridge outlives the shell, as another adapter's does in the core, so the
    // holder ends on its own, not with the bridge's thread.
    let bridge = Bridge::on_private_bus(&rig.bus).unwrap();
    let (shell, _events) = shell_on(bridge.clone(), settings(DEFAULT_DEADLINE, None));
    shell.set_tray(state()).unwrap();
    let (item, sender) = rig.registered(&mut registrations);
    let mut changes = rig.runtime.block_on(owner_changes(rig.host(), Names::Name(&item))).unwrap();
    let owner = rig.runtime.block_on(name_owner(rig.host(), &item)).unwrap();
    assert_eq!(owner.map(|owner| owner.to_string()), Some(sender), "on the bus");
    drop(shell);
    let change = rig.runtime.block_on(changes.next()).unwrap();
    assert_eq!((change.name.as_str(), change.new), (item.as_str(), None), "the name has gone");
    drop(bridge);
}

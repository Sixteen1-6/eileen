//! The tray on Linux: [`StatusNotifier`], Eileen's item of the `StatusNotifierItem`
//! specification ([freedesktop's draft][sni]), with its menu over dbusmenu
//! (`com.canonical.dbusmenu`, version 3), for `ShellHost::set_tray` (BUILD.md section 3.4;
//! docs/LINUX.md sections 3 and 4), whose doc in `host` says how it is held.
//!
//! [sni]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/
//!
//! On a session connection of its caller's own, [`StatusNotifier::serve`] serves two objects with
//! `#[zbus::interface]`, then takes a well-known name, `org.kde.StatusNotifierItem-<pid>-<n>`,
//! which [`StatusNotifier::register`] gives the watcher (`org.kde.StatusNotifierWatcher`). The
//! tray's host, such as GNOME's `AppIndicator` extension or KDE's panel, reads the item from there.
//!
//! - `org.kde.StatusNotifierItem` at `/StatusNotifierItem`: `Category` `ApplicationStatus`,
//!   `Id` the settings' desktop entry, `Title` `Eileen`, `Status` `Active`, `IconName` by the
//!   mute state ([`TraySettings`]), `ToolTip` (that icon, no pixmap, `Eileen`, the state's
//!   tooltip), `Menu` `/MenuBar` and `ItemIsMenu` true, so a click on the icon opens the menu.
//!   The pixmaps, overlay and attention icons and `IconThemePath` are empty; `WindowId` is left
//!   out, since copies of the interface give it different types, and hosts go without it.
//!   `Activate`, `SecondaryActivate`, `ContextMenu` and `Scroll` do nothing. A change of icon or
//!   tooltip sends `NewIcon` or `NewToolTip`, after which a host reads the item again.
//! - `com.canonical.dbusmenu` at `/MenuBar`: the `menu` module's layout and properties, with an
//!   unknown id answered `InvalidArgs`. A `clicked` event on an item passes its `TrayItem::id`
//!   to the caller's channel; any other event, and one on the root (a host's `opened` and
//!   `closed`), passes nothing. `EventGroup` answers the ids it does not know, and
//!   `InvalidArgs` when it knows none. `AboutToShow` never asks for an update. A change of the
//!   menu sends `LayoutUpdated(revision, 0)`, after which a host fetches the layout again.
//!
//! Traps:
//!
//! - Nothing says that a click came from a person: any program on the session bus may send the
//!   menu an `Event`, as `LinuxShellEvent::TrayChoice` says.
//! - The choice channel is bounded and a full one drops the click, so that the object server,
//!   which serves the whole connection, never waits on the caller.
//! - The watcher keeps the item only while the name has its owner, and a watcher that restarts
//!   forgets it: the caller follows the watcher's owner and registers again ([`super::host`]).
//! - A watcher may have no host (KDE's without a panel): the item is registered, and unseen.

mod menu;

use std::sync::atomic::{AtomicU32, Ordering};

use eileen_platform::{TrayItem, TrayState};
use tokio::sync::mpsc;
use zbus::Connection;
use zbus::fdo;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::DbusError;
use menu::{Layout, Menu, Properties, ROOT};

/// The watcher's name, which is also its interface's, and its object.
pub(crate) const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// The item's object and the menu's.
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
/// The start of the item's well-known name.
const ITEM_NAME: &str = "org.kde.StatusNotifierItem";
/// What the errors name, each a bare noun.
const SERVE_WHAT: &str = "tray objects";
const NAME_WHAT: &str = "tray item name";
const REGISTER_WHAT: &str = "tray registration";
/// The event a host sends for a chosen item.
const CLICKED: &str = "clicked";
/// What the item and the tooltip call Eileen.
const TITLE: &str = "Eileen";

/// The number in the next item's name, after the process's ID.
static NEXT_ITEM: AtomicU32 = AtomicU32::new(1);

/// The tray's icons, by name in the desktop's icon theme. Eileen installs no icon of its own
/// before phase L3's packages, so the defaults are the Adwaita theme's symbolic microphones, which
/// GNOME and most themes have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraySettings {
    /// While the microphone is on: `audio-input-microphone-symbolic` by default.
    pub icon: String,
    /// While it is muted: `microphone-sensitivity-muted-symbolic` by default.
    pub muted_icon: String,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            icon: "audio-input-microphone-symbolic".to_owned(),
            muted_icon: "microphone-sensitivity-muted-symbolic".to_owned(),
        }
    }
}

/// Eileen's tray item and its menu, served on its caller's connection; the module's doc says
/// what each shows. Dropping it leaves the objects and the name to the connection, which drops
/// them as it closes.
pub(crate) struct StatusNotifier {
    conn: Connection,
    /// The item's well-known name.
    name: String,
    item: InterfaceRef<TrayItemServer>,
    menu: InterfaceRef<MenuServer>,
}

impl StatusNotifier {
    /// Serves the item, as the application `id` (its desktop entry) with `settings`' icons, and
    /// its empty menu on `conn`, then takes the item's name. Each chosen item's `TrayItem::id`
    /// goes to `choices`, dropped when it is full.
    pub(crate) async fn serve(
        conn: &Connection,
        id: &str,
        settings: TraySettings,
        choices: mpsc::Sender<String>,
    ) -> Result<Self, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(SERVE_WHAT, &error);
        let server = conn.object_server();
        server.at(ITEM_PATH, TrayItemServer::new(id, settings)).await.map_err(failed)?;
        server.at(MENU_PATH, MenuServer { menu: Menu::new(), choices }).await.map_err(failed)?;
        let item = server.interface(ITEM_PATH).await.map_err(failed)?;
        let menu = server.interface(MENU_PATH).await.map_err(failed)?;
        let n = NEXT_ITEM.fetch_add(1, Ordering::Relaxed);
        let name = format!("{ITEM_NAME}-{}-{n}", std::process::id());
        conn.request_name(name.as_str())
            .await
            .map_err(|error| DbusError::from_zbus(NAME_WHAT, &error))?;
        Ok(Self { conn: conn.clone(), name, item, menu })
    }

    /// Shows `state`, and signals what changed: `NewIcon` and `NewToolTip` for the item,
    /// `LayoutUpdated` for the menu.
    pub(crate) async fn show(&self, state: &TrayState) {
        let (icon, tooltip) = self.item.get_mut().await.show(state.muted, &state.tooltip);
        // A signal fails only on a connection that has closed, which takes the item with it.
        let emitter = self.item.signal_emitter();
        if icon {
            let _ignored = TrayItemServer::new_icon(emitter).await;
        }
        if tooltip {
            let _ignored = TrayItemServer::new_tool_tip(emitter).await;
        }
        let revision = self.menu.get_mut().await.show(&state.menu);
        if let Some(revision) = revision {
            let emitter = self.menu.signal_emitter();
            let _ignored = MenuServer::layout_updated(emitter, revision, ROOT).await;
        }
    }

    /// Gives the watcher the item's name (`RegisterStatusNotifierItem`).
    pub(crate) async fn register(&self) -> Result<(), DbusError> {
        let name = (self.name.as_str(),);
        self.conn
            .call_method(
                Some(WATCHER),
                WATCHER_PATH,
                Some(WATCHER),
                "RegisterStatusNotifierItem",
                &name,
            )
            .await
            .map(drop)
            .map_err(|error| DbusError::from_zbus(REGISTER_WHAT, &error))
    }
}

/// `org.kde.StatusNotifierItem`: what the icon shows.
#[derive(Debug)]
struct TrayItemServer {
    id: String,
    settings: TraySettings,
    /// The icon's name now.
    icon: String,
    /// The tooltip's text now.
    tooltip: String,
}

impl TrayItemServer {
    /// The item of the application `id`, its microphone on and no tooltip.
    fn new(id: &str, settings: TraySettings) -> Self {
        let icon = settings.icon.replace('\0', "");
        Self { id: id.replace('\0', ""), settings, icon, tooltip: String::new() }
    }

    /// Shows the mute state and the tooltip; whether the icon changed, and whether the tooltip,
    /// which shows the icon too, did.
    fn show(&mut self, muted: bool, tooltip: &str) -> (bool, bool) {
        let icon = if muted { &self.settings.muted_icon } else { &self.settings.icon };
        let icon = icon.replace('\0', "");
        let tooltip = tooltip.replace('\0', "");
        let new_icon = icon != self.icon;
        let new_tooltip = new_icon || tooltip != self.tooltip;
        self.icon = icon;
        self.tooltip = tooltip;
        (new_icon, new_tooltip)
    }
}

/// An icon as pixels, `a(iiay)`: none here.
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
#[expect(
    clippy::unused_self,
    clippy::missing_const_for_fn,
    clippy::unnecessary_literal_bound,
    unused_variables,
    reason = "zbus serves each property and method from a method on `&self`, whatever it reads, \
              and passes every argument, the unused ones too"
)]
impl TrayItemServer {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        &self.id
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        TITLE
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        &self.icon
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        Pixmaps::new()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        Pixmaps::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Pixmaps::new()
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        (self.icon.clone(), Pixmaps::new(), TITLE.to_owned(), self.tooltip.clone())
    }

    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(MENU_PATH)
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        true
    }

    /// Nothing: the item is a menu, which the host opens.
    fn activate(&self, x: i32, y: i32) {}

    /// Nothing, as `Activate`.
    fn secondary_activate(&self, x: i32, y: i32) {}

    /// Nothing: the host shows the menu itself.
    fn context_menu(&self, x: i32, y: i32) {}

    /// Nothing: the tray has no scroll action.
    fn scroll(&self, delta: i32, orientation: &str) {}

    #[zbus(signal)]
    async fn new_icon(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn new_tool_tip(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// `com.canonical.dbusmenu`: the tray's menu, and where its choices go.
#[derive(Debug)]
struct MenuServer {
    menu: Menu,
    choices: mpsc::Sender<String>,
}

impl MenuServer {
    /// Shows `items`; the new revision when that changed the menu.
    fn show(&mut self, items: &[TrayItem]) -> Option<u32> {
        self.menu.set(items).then(|| self.menu.revision())
    }

    /// Takes the event `event` on the node `id`: a click on an item passes its `TrayItem::id`
    /// on. Whether the menu knows `id`.
    fn take(&self, id: i32, event: &str) -> bool {
        if event == CLICKED
            && let Some(item) = self.menu.item(id)
        {
            // Full, the queue drops the click rather than hold the object server.
            let _ignored = self.choices.try_send(item.id.clone());
        }
        self.menu.knows(id)
    }
}

/// The error for the unknown id `id`.
fn unknown(id: i32) -> fdo::Error {
    fdo::Error::InvalidArgs(format!("no menu item {id}"))
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
#[expect(
    clippy::unused_self,
    clippy::missing_const_for_fn,
    clippy::unnecessary_literal_bound,
    unused_variables,
    clippy::needless_pass_by_value,
    reason = "zbus serves each property and method from a method on `&self`, whatever it reads, \
              and passes every argument as an owned value, the unused ones too"
)]
impl MenuServer {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    /// The revision, and the node `parent_id` with its children `recursion_depth` levels down.
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> fdo::Result<(u32, Layout)> {
        let layout = self.menu.layout(parent_id, recursion_depth, &property_names);
        Ok((self.menu.revision(), layout.ok_or_else(|| unknown(parent_id))?))
    }

    /// The properties of each known node of `ids`, or of every node when it lists none.
    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, Properties)> {
        let ids = if ids.is_empty() { self.menu.ids().collect() } else { ids };
        let of = |id: i32| Some((id, self.menu.properties(id, &property_names)?));
        ids.into_iter().filter_map(of).collect()
    }

    /// One property of the node `id`.
    fn get_property(&self, id: i32, name: String) -> fdo::Result<Value<'static>> {
        let mut properties = self.menu.properties(id, &[]).ok_or_else(|| unknown(id))?;
        properties
            .remove(&name)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no property {name} on item {id}")))
    }

    /// An event of the host's, such as a click.
    fn event(&self, id: i32, event_id: &str, data: OwnedValue, timestamp: u32) -> fdo::Result<()> {
        if self.take(id, event_id) { Ok(()) } else { Err(unknown(id)) }
    }

    /// Several events: the ids the menu does not know, or `InvalidArgs` when it knows none.
    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> fdo::Result<Vec<i32>> {
        let taken = |(id, event, _, _): &(i32, String, OwnedValue, u32)| self.take(*id, event);
        let errors: Vec<i32> = events.iter().filter(|event| !taken(event)).map(|e| e.0).collect();
        if errors.len() == events.len() && !events.is_empty() {
            return Err(fdo::Error::InvalidArgs("no such menu items".to_owned()));
        }
        Ok(errors)
    }

    /// Whether the node `id` needs an update before it is shown: never.
    fn about_to_show(&self, id: i32) -> bool {
        false
    }

    /// The nodes of `ids` that need an update, none, and those the menu does not know.
    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), ids.into_iter().filter(|id| !self.menu.knows(*id)).collect())
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

//! The D-Bus interfaces the client calls, as zbus proxies, with the members it uses and no
//! others: the session bus's `org.a11y.Bus` and `org.a11y.Status`, which at-spi-bus-launcher
//! serves at `/org/a11y/bus`, and each accessible object's `org.a11y.atspi.Accessible`,
//! `org.a11y.atspi.Text`, `org.a11y.atspi.Component`, `org.a11y.atspi.Action`,
//! `org.a11y.atspi.EditableText` and `org.a11y.atspi.Value` (at-spi2-core 2.52's
//! introspection). The bus daemon's own `GetConnectionUnixProcessID` goes through zbus's
//! `fdo::DBusProxy`. Only `Value.CurrentValue` has a setter, so `IsEnabled` cannot be written
//! through them.
//!
//! Every proxy is built with `CacheProperties::No`, so building one makes no call and none
//! subscribes to a property's changes or its service's owner (test
//! `no_proxy_fills_a_property_cache`); an object's proxy names the app's unique name, never a
//! well-known one.

use zbus::proxy::{CacheProperties, Defaults};
use zbus::zvariant::OwnedObjectPath;

use super::ObjectRef;

/// A reference to an object as AT-SPI2 sends one, `(so)`: the bus name of the app's connection
/// and the object's path.
pub(super) type Reference = (String, OwnedObjectPath);

/// The launcher's `org.a11y.Bus`.
#[zbus::proxy(
    interface = "org.a11y.Bus",
    default_service = "org.a11y.Bus",
    default_path = "/org/a11y/bus",
    gen_blocking = false
)]
pub(super) trait Bus {
    /// The accessibility bus's address.
    fn get_address(&self) -> zbus::Result<String>;
}

/// The launcher's `org.a11y.Status`.
#[zbus::proxy(
    interface = "org.a11y.Status",
    default_service = "org.a11y.Bus",
    default_path = "/org/a11y/bus",
    gen_blocking = false
)]
pub(super) trait Status {
    /// Whether toolkits are asked to put their trees on the accessibility bus.
    #[zbus(property)]
    fn is_enabled(&self) -> zbus::Result<bool>;
}

/// An object's `org.a11y.atspi.Accessible`.
#[zbus::proxy(interface = "org.a11y.atspi.Accessible", gen_blocking = false)]
pub(super) trait Accessible {
    /// Its name.
    #[zbus(property)]
    fn name(&self) -> zbus::Result<String>;
    /// How many children it says it has.
    #[zbus(property)]
    fn child_count(&self) -> zbus::Result<i32>;
    /// The id its app gives it for automation, empty when it has none.
    #[zbus(property)]
    fn accessible_id(&self) -> zbus::Result<String>;
    /// Its role's number.
    fn get_role(&self) -> zbus::Result<u32>;
    /// Its states, in two words.
    fn get_state(&self) -> zbus::Result<Vec<u32>>;
    /// The names of the interfaces it implements.
    fn get_interfaces(&self) -> zbus::Result<Vec<String>>;
    /// All its children.
    fn get_children(&self) -> zbus::Result<Vec<Reference>>;
    /// Its child at `index`.
    fn get_child_at_index(&self, index: i32) -> zbus::Result<Reference>;
}

/// An object's `org.a11y.atspi.Text`.
#[zbus::proxy(interface = "org.a11y.atspi.Text", gen_blocking = false)]
pub(super) trait Text {
    /// The text from `start_offset` to `end_offset`, in characters.
    fn get_text(&self, start_offset: i32, end_offset: i32) -> zbus::Result<String>;
    /// How many ranges of the text are selected.
    fn get_n_selections(&self) -> zbus::Result<i32>;
    /// Where selected range `selection_num` starts, and the character after its last.
    fn get_selection(&self, selection_num: i32) -> zbus::Result<(i32, i32)>;
}

/// An object's `org.a11y.atspi.Component`.
#[zbus::proxy(interface = "org.a11y.atspi.Component", gen_blocking = false)]
pub(super) trait Component {
    /// Its box, `(x, y, width, height)`, in the coordinates `coord_type` names.
    fn get_extents(&self, coord_type: u32) -> zbus::Result<(i32, i32, i32, i32)>;
}

/// An object's `org.a11y.atspi.Action`.
#[zbus::proxy(interface = "org.a11y.atspi.Action", gen_blocking = false)]
pub(super) trait Action {
    /// How many actions it has.
    #[zbus(property)]
    fn n_actions(&self) -> zbus::Result<i32>;
    /// The name of its action at `index`, such as `click`.
    fn get_name(&self, index: i32) -> zbus::Result<String>;
    /// Runs its action at `index`: whether the app ran it.
    fn do_action(&self, index: i32) -> zbus::Result<bool>;
}

/// An object's `org.a11y.atspi.EditableText`.
#[zbus::proxy(interface = "org.a11y.atspi.EditableText", gen_blocking = false)]
pub(super) trait EditableText {
    /// Replaces its whole text with `new_contents`: whether the app took it.
    fn set_text_contents(&self, new_contents: &str) -> zbus::Result<bool>;
}

/// An object's `org.a11y.atspi.Value`.
#[zbus::proxy(interface = "org.a11y.atspi.Value", gen_blocking = false)]
pub(super) trait Value {
    /// Its number.
    #[zbus(property)]
    fn current_value(&self) -> zbus::Result<f64>;
    /// Sets its number, through `org.freedesktop.DBus.Properties.Set`.
    #[zbus(property)]
    fn set_current_value(&self, value: f64) -> zbus::Result<()>;
}

/// The proxy `P` of `object`, at its app's unique name and its path.
pub(super) async fn of<'a, P>(conn: &zbus::Connection, object: &'a ObjectRef) -> zbus::Result<P>
where
    P: Defaults + From<zbus::Proxy<'a>>,
{
    zbus::proxy::Builder::<P>::new(conn)
        .destination(&object.bus)?
        .path(&object.path)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

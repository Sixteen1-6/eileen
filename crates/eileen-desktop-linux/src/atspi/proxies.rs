//! The D-Bus interfaces the client calls, as zbus proxies, with the members it uses and no
//! others: the session bus's `org.a11y.Bus` and `org.a11y.Status`, which at-spi-bus-launcher
//! serves at `/org/a11y/bus`, and each accessible object's `org.a11y.atspi.Accessible` and
//! `org.a11y.atspi.Text` (at-spi2-core 2.52's introspection). No property has a setter, so
//! `IsEnabled` cannot be written through them.
//!
//! Every proxy is built with `CacheProperties::No`, so building one makes no call and none
//! subscribes to a property's changes or its service's owner; an object's proxy names the app's
//! unique name, never a well-known one.

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

//! The AT-SPI2 client: what the desktop's apps put on the accessibility bus, as UI Automation
//! gives it on Windows (docs/LINUX.md sections 3 and 4). The interfaces are at-spi2-core 2.52's,
//! as the introspection data of its launcher and registry and of the ATK bridge give them.
//!
//! - The bus: [`accessibility_bus`] asks the session bus's `org.a11y.Bus` for its address and
//!   connects with a call timeout, [`CALL_TIMEOUT`] in the product.
//! - The applications: the children of the registry's root, `org.a11y.atspi.Registry` at
//!   `/org/a11y/atspi/accessible/root`, each an app's root object ([`Atspi::applications`]).
//! - Each object, reached at its app's unique name and its path ([`ObjectRef`]): its name, role,
//!   states and interfaces ([`Atspi::accessible`]), its children ([`Atspi::children`]), its text
//!   ([`Atspi::text`]) and a bounded walk of its tree ([`Atspi::walk`]).
//!
//! An app's text is untrusted: it is sanitized and bounded before it leaves this module, and
//! errors never carry it. Every call waits at most the connection's call timeout; past it, the
//! call is [`DbusError::DeadlineExceeded`].

mod bus;
mod kinds;
mod proxies;
mod walk;

use std::time::Duration;

use eileen_contracts::desktop::sanitize;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::DbusError;
use tokio::time::Instant;
use zbus::Connection;
use zbus::names::OwnedUniqueName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

pub use self::bus::{accessibility_bus, toolkit_accessibility};
pub use self::kinds::{Interfaces, Role, State, StateSet};
use self::proxies::{AccessibleProxy, Reference, TextProxy};
pub use self::walk::{Walk, WalkNode};

/// The call timeout of the accessibility bus's connection: 800 ms, libatspi 2.52's own default
/// for each call (its `method_call_timeout`, read from the library's data). It is shorter than
/// the session bus's 5 s, so one app that hangs cannot stall a listing.
pub const CALL_TIMEOUT: Duration = Duration::from_millis(800);

/// The registry's bus name.
const REGISTRY: &str = "org.a11y.atspi.Registry";
/// The registry's root object, whose children are the apps' roots.
const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The null reference's path, which names no object.
const NULL: &str = "/org/a11y/atspi/null";
/// The most characters of a name kept.
const NAME_CHARS: usize = 1_000;

/// What errors of the registry's listing name.
const REGISTRY_CALL: &str = "accessibility registry";
/// What errors of an object's reads name.
const OBJECT: &str = "accessible object";
/// What errors of a text read name.
const TEXT: &str = "accessible text";

/// An accessible object: the unique name of its app's connection on the accessibility bus and
/// its path there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectRef {
    /// The app's connection, such as `:1.42`.
    pub bus: OwnedUniqueName,
    /// The object's path in the app.
    pub path: OwnedObjectPath,
}

impl ObjectRef {
    /// The object `bus` and `path` name, as a reference an app sent gives them; `None` when
    /// `bus` is not a unique name (an app is reached at its connection, never at a well-known
    /// name another process could take) or `path` is the null reference's.
    #[must_use]
    pub fn new(bus: &str, path: &ObjectPath<'_>) -> Option<Self> {
        if !bus.starts_with(':') || path.as_str() == NULL {
            return None;
        }
        let bus = OwnedUniqueName::try_from(bus).ok()?;
        Some(Self { bus, path: path.clone().into() })
    }
}

/// What one object is, as [`Atspi::accessible`] read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accessible {
    /// The object.
    pub object: ObjectRef,
    /// Its name, sanitized and kept to 1,000 characters.
    pub name: Untrusted<String>,
    /// Its role.
    pub role: Role,
    /// Its states.
    pub states: StateSet,
    /// The interfaces it implements, among those [`Interfaces`] knows.
    pub interfaces: Interfaces,
    /// How many children it says it has (`ChildCount`, 0 when negative), which can be wrong.
    pub child_count: u32,
}

/// The client of one accessibility bus. It takes the bus's connection from its caller:
/// [`accessibility_bus`]'s in the product, a private bus in tests. Every call waits at most that
/// connection's method timeout; on a connection made without one, an app that never answers
/// holds a call for good.
#[derive(Debug, Clone)]
pub struct Atspi {
    conn: Connection,
}

/// Some of an object's children, and whether it has more than were read.
struct Listing {
    refs: Vec<ObjectRef>,
    cut: bool,
}

impl Atspi {
    /// The client of the accessibility bus `conn` is connected to.
    #[must_use]
    pub fn new(conn: &Connection) -> Self {
        Self { conn: conn.clone() }
    }

    /// The apps the registry lists: its root's children, each an app's root object. A
    /// reference [`ObjectRef::new`] refuses is left out.
    pub async fn applications(&self) -> Result<Vec<ObjectRef>, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(REGISTRY_CALL, &error);
        let registry = AccessibleProxy::builder(&self.conn)
            .destination(REGISTRY)
            .and_then(|builder| builder.path(ROOT))
            .map_err(failed)?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(failed)?;
        Ok(references(&registry.get_children().await.map_err(failed)?))
    }

    /// What `object` is: its `Name`, `GetRole`, `GetState`, `GetInterfaces` and `ChildCount`,
    /// asked at once. The name is sanitized and kept to 1,000 characters, and interface names
    /// outside [`Interfaces`]' table are dropped. Any of the five that fails fails the read.
    pub async fn accessible(&self, object: &ObjectRef) -> Result<Accessible, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(OBJECT, &error);
        let proxy: AccessibleProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        let (name, role, states, interfaces, count) = tokio::join!(
            proxy.name(),
            proxy.get_role(),
            proxy.get_state(),
            proxy.get_interfaces(),
            proxy.child_count(),
        );
        Ok(Accessible {
            object: object.clone(),
            name: sanitize(&name.map_err(failed)?, NAME_CHARS),
            role: Role(role.map_err(failed)?),
            states: StateSet::from_words(&states.map_err(failed)?),
            interfaces: Interfaces::from_names(&interfaces.map_err(failed)?),
            child_count: u32::try_from(count.map_err(failed)?).unwrap_or(0),
        })
    }

    /// At most `at_most` of `parent`'s children, in order. `GetChildren` reads them at once
    /// when the parent says it has no more than that and does not manage its descendants;
    /// otherwise, or when the app has no `GetChildren`, `GetChildAtIndex` reads them one by one
    /// from index 0, so a big table never builds every child. A reference [`ObjectRef::new`]
    /// refuses is left out, and a failed index read fails the listing.
    pub async fn children(
        &self,
        parent: &Accessible,
        at_most: u32,
    ) -> Result<Vec<ObjectRef>, DbusError> {
        self.list(parent, at_most, None).await.map(|listing| listing.refs)
    }

    /// The text of `object`, at most `max_chars` characters of it, sanitized. It reads the
    /// object's role first and refuses a password field before any `Text` call, as Windows
    /// refuses an element whose `IsPassword` is set (BUILD.md section 2.6).
    pub async fn text(
        &self,
        object: &ObjectRef,
        max_chars: usize,
    ) -> Result<Untrusted<String>, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(TEXT, &error);
        let accessible: AccessibleProxy<'_> =
            proxies::of(&self.conn, object).await.map_err(failed)?;
        if Role(accessible.get_role().await.map_err(failed)?) == Role::PASSWORD_TEXT {
            return Err(DbusError::Refused { what: TEXT, reason: "a password field" });
        }
        let text: TextProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        let end = i32::try_from(max_chars).unwrap_or(i32::MAX);
        Ok(sanitize(&text.get_text(0, end).await.map_err(failed)?, max_chars))
    }

    /// [`Self::children`], with `cut` set when `parent` has more children than were read. With
    /// `until`, no index is read once it has passed, and the listing is cut there.
    async fn list(
        &self,
        parent: &Accessible,
        at_most: u32,
        until: Option<Instant>,
    ) -> Result<Listing, DbusError> {
        let failed = |error: zbus::Error| DbusError::from_zbus(OBJECT, &error);
        let proxy: AccessibleProxy<'_> =
            proxies::of(&self.conn, &parent.object).await.map_err(failed)?;
        let room = usize::try_from(at_most).unwrap_or(usize::MAX);
        let crowded =
            parent.child_count > at_most || parent.states.contains(State::MANAGES_DESCENDANTS);
        if !crowded {
            match proxy.get_children().await.map_err(failed) {
                Ok(children) => {
                    let mut refs = references(&children);
                    let cut = refs.len() > room;
                    refs.truncate(room);
                    return Ok(Listing { refs, cut });
                }
                // The app has no GetChildren: read by index.
                Err(DbusError::NotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        let mut refs = Vec::new();
        for index in 0..parent.child_count.min(at_most) {
            if until.is_some_and(|until| Instant::now() >= until) {
                return Ok(Listing { refs, cut: true });
            }
            let Ok(index) = i32::try_from(index) else { break };
            let (bus, path) = proxy.get_child_at_index(index).await.map_err(failed)?;
            refs.extend(ObjectRef::new(&bus, &path));
        }
        Ok(Listing { refs, cut: parent.child_count > at_most })
    }
}

/// The references in `children` that name an object.
fn references(children: &[Reference]) -> Vec<ObjectRef> {
    children.iter().filter_map(|(bus, path)| ObjectRef::new(bus, path)).collect()
}

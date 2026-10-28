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
//!   and selected text ([`Atspi::text`], [`Atspi::selection`]), its box in its window
//!   ([`Atspi::extents`]), its automation id ([`Atspi::accessible_id`]) and a bounded walk of its
//!   tree ([`Atspi::walk`]).
//! - Each app's process, as the bus's daemon knows it ([`Atspi::pid`]).
//!
//! An app's text is untrusted: it is sanitized and bounded before it leaves this module, and
//! errors never carry it, not even an error name the app chose. Every call waits at most the
//! connection's call timeout; past it, the call is [`DbusError::DeadlineExceeded`].

mod bus;
mod kinds;
mod proxies;
mod walk;

use std::time::Duration;

use eileen_contracts::desktop::{Rect, sanitize};
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::DbusError;
use tokio::time::Instant;
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::OwnedUniqueName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

pub use self::bus::{accessibility_bus, toolkit_accessibility};
pub use self::kinds::{Interfaces, Role, State, StateSet};
use self::proxies::{AccessibleProxy, ComponentProxy, Reference, TextProxy};
pub use self::walk::{Walk, WalkNode};

/// The call timeout of the accessibility bus's connection: 800 ms, libatspi 2.52's
/// `method_call_timeout`, read from the library's data. It is shorter than the session bus's
/// 5 s, so one app that hangs cannot stall a listing.
///
/// libatspi uses it as a floor, not a flat bound: a call to an app it first saw less than its
/// `app_startup_time` (15 s) ago may take what is left of those 15 s. This client keeps 800 ms
/// from the start, so a read right after an app starts can pass its deadline where libatspi,
/// and so a screen reader, would wait.
pub const CALL_TIMEOUT: Duration = Duration::from_millis(800);

/// The registry's bus name.
const REGISTRY: &str = "org.a11y.atspi.Registry";
/// The registry's root object, whose children are the apps' roots.
const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// The null reference's path, which names no object.
const NULL: &str = "/org/a11y/atspi/null";
/// The error an object answers for a method it does not have.
const UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
/// The most characters of a name or an automation id kept.
const NAME_CHARS: usize = 1_000;
/// `ATSPI_COORD_TYPE_WINDOW`: coordinates from the top left of the object's window, the one
/// kind Wayland gives a meaning to.
const WINDOW_COORDS: u32 = 1;

/// What errors of the registry's listing name.
const REGISTRY_CALL: &str = "accessibility registry";
/// What errors of an object's reads name.
const OBJECT: &str = "accessible object";
/// What errors of a text read name.
const TEXT: &str = "accessible text";
/// What errors of a box's read name.
const EXTENTS: &str = "accessible extents";
/// What errors of a process id's read name.
const PROCESS: &str = "app process";
/// The detail of an error the callee answered by a name this client does not know, which is not
/// kept.
const APPS_OWN_ERROR: &str = "an error name the app chose";

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
    /// reference [`ObjectRef::new`] refuses is left out. No registry on the bus is
    /// [`DbusError::Unavailable`]: nothing serves the desktop's tree.
    pub async fn applications(&self) -> Result<Vec<ObjectRef>, DbusError> {
        let failed = |error: zbus::Error| failure(REGISTRY_CALL, &error);
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
    /// outside [`Interfaces`]' table are dropped. Any of the five that fails fails the read. An
    /// object whose app has gone, or that its app does not serve, is [`DbusError::NotFound`].
    pub async fn accessible(&self, object: &ObjectRef) -> Result<Accessible, DbusError> {
        let failed = |error: zbus::Error| object_failure(OBJECT, &error);
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
    /// refuses is left out. `ChildCount` can say too many, and a child can go between the
    /// reads, so an index read that fails after the first ends the listing with the children
    /// read before it; a failed first read fails the listing. A parent whose app has gone is
    /// [`DbusError::NotFound`].
    pub async fn children(
        &self,
        parent: &Accessible,
        at_most: u32,
    ) -> Result<Vec<ObjectRef>, DbusError> {
        self.list(parent, at_most, None).await.map(|listing| listing.refs)
    }

    /// The text of `object`, at most `max_chars` characters of it, sanitized. It reads the
    /// object's role first and refuses a password field before any `Text` call, as Windows
    /// refuses an element whose `IsPassword` is set (BUILD.md section 2.6). An object without
    /// `Text`, or whose app has gone, is [`DbusError::NotFound`].
    pub async fn text(
        &self,
        object: &ObjectRef,
        max_chars: usize,
    ) -> Result<Untrusted<String>, DbusError> {
        let failed = |error: zbus::Error| object_failure(TEXT, &error);
        let text = self.text_of(object).await?;
        let end = i32::try_from(max_chars).unwrap_or(i32::MAX);
        Ok(sanitize(&text.get_text(0, end).await.map_err(failed)?, max_chars))
    }

    /// The selected text of `object`: its first selected range (`GetNSelections`,
    /// `GetSelection(0)`), at most `max_chars` characters of it from its start, sanitized.
    /// `None` when nothing is selected. A password field is refused as [`Self::text`] refuses
    /// it, before any `Text` call.
    pub async fn selection(
        &self,
        object: &ObjectRef,
        max_chars: usize,
    ) -> Result<Option<Untrusted<String>>, DbusError> {
        let failed = |error: zbus::Error| object_failure(TEXT, &error);
        let text = self.text_of(object).await?;
        if text.get_n_selections().await.map_err(failed)? < 1 {
            return Ok(None);
        }
        let (start, end) = text.get_selection(0).await.map_err(failed)?;
        if start < 0 || end <= start {
            return Ok(None);
        }
        let end = end.min(start.saturating_add(i32::try_from(max_chars).unwrap_or(i32::MAX)));
        Ok(Some(sanitize(&text.get_text(start, end).await.map_err(failed)?, max_chars)))
    }

    /// Where `object` is in its window: `Component.GetExtents` in window coordinates, a negative
    /// width or height read as 0. An object without `Component`, or whose app has gone, is
    /// [`DbusError::NotFound`].
    pub async fn extents(&self, object: &ObjectRef) -> Result<Rect, DbusError> {
        let failed = |error: zbus::Error| object_failure(EXTENTS, &error);
        let component: ComponentProxy<'_> =
            proxies::of(&self.conn, object).await.map_err(failed)?;
        let (x, y, w, h) = component.get_extents(WINDOW_COORDS).await.map_err(failed)?;
        let size = |length: i32| u32::try_from(length).unwrap_or(0);
        Ok(Rect { x, y, w: size(w), h: size(h) })
    }

    /// The id `object`'s app gives it for automation (`AccessibleId`), sanitized and kept to
    /// 1,000 characters; `None` when it is empty. In a browser it is the page's own `id`, as
    /// UI Automation's automation id is, so it stays untrusted.
    pub async fn accessible_id(
        &self,
        object: &ObjectRef,
    ) -> Result<Option<Untrusted<String>>, DbusError> {
        let failed = |error: zbus::Error| object_failure(OBJECT, &error);
        let proxy: AccessibleProxy<'_> = proxies::of(&self.conn, object).await.map_err(failed)?;
        let id = sanitize(&proxy.accessible_id().await.map_err(failed)?, NAME_CHARS);
        Ok((!id.get().is_empty()).then_some(id))
    }

    /// The process of the app at `bus`: `GetConnectionUnixProcessID` of the accessibility bus's
    /// daemon, which reads it from the connection's socket, never what the app says of itself.
    /// An app whose connection has gone is [`DbusError::NotFound`].
    pub async fn pid(&self, bus: &OwnedUniqueName) -> Result<u32, DbusError> {
        let failed = |error: zbus::Error| object_failure(PROCESS, &error);
        let daemon = DBusProxy::builder(&self.conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(failed)?;
        let pid = daemon.get_connection_unix_process_id(bus.as_ref().into()).await;
        pid.map_err(|error| failed(error.into()))
    }

    /// `object`'s `Text`, once its role has shown it is no password field.
    async fn text_of<'a>(&self, object: &'a ObjectRef) -> Result<TextProxy<'a>, DbusError> {
        let failed = |error: zbus::Error| object_failure(TEXT, &error);
        let accessible: AccessibleProxy<'_> =
            proxies::of(&self.conn, object).await.map_err(failed)?;
        if Role(accessible.get_role().await.map_err(failed)?) == Role::PASSWORD_TEXT {
            return Err(DbusError::Refused { what: TEXT, reason: "a password field" });
        }
        proxies::of(&self.conn, object).await.map_err(failed)
    }

    /// [`Self::children`], with `cut` set when `parent` has more children than were read, or an
    /// index read failed after the first. With `until`, no index is read once it has passed, and
    /// the listing is cut there.
    async fn list(
        &self,
        parent: &Accessible,
        at_most: u32,
        until: Option<Instant>,
    ) -> Result<Listing, DbusError> {
        let failed = |error: zbus::Error| object_failure(OBJECT, &error);
        let proxy: AccessibleProxy<'_> =
            proxies::of(&self.conn, &parent.object).await.map_err(failed)?;
        let room = usize::try_from(at_most).unwrap_or(usize::MAX);
        let crowded =
            parent.child_count > at_most || parent.states.contains(State::MANAGES_DESCENDANTS);
        if !crowded {
            match proxy.get_children().await {
                Ok(children) => {
                    let mut refs = references(&children);
                    let cut = refs.len() > room;
                    refs.truncate(room);
                    return Ok(Listing { refs, cut });
                }
                // The app has no GetChildren: read by index. Any other failure, an object or
                // an app gone since its read included, is the listing's.
                Err(error) if unknown_method(&error) => {}
                Err(error) => return Err(failed(error)),
            }
        }
        let mut refs = Vec::new();
        for index in 0..parent.child_count.min(at_most) {
            if until.is_some_and(|until| Instant::now() >= until) {
                return Ok(Listing { refs, cut: true });
            }
            let Ok(index) = i32::try_from(index) else { break };
            match proxy.get_child_at_index(index).await {
                Ok((bus, path)) => refs.extend(ObjectRef::new(&bus, &path)),
                // A count that says too many, or a child gone since: what was read is kept.
                Err(_) if index > 0 => return Ok(Listing { refs, cut: true }),
                Err(error) => return Err(failed(error)),
            }
        }
        Ok(Listing { refs, cut: parent.child_count > at_most })
    }
}

/// The references in `children` that name an object.
fn references(children: &[Reference]) -> Vec<ObjectRef> {
    children.iter().filter_map(|(bus, path)| ObjectRef::new(bus, path)).collect()
}

/// Whether `error` is an object's answer for a method it does not have.
fn unknown_method(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, ..) if name.as_str() == UNKNOWN_METHOD)
}

/// What a failed call on the accessibility bus means: [`DbusError::from_zbus`]'s answer, without
/// an error name the callee chose. Any process on the bus can answer with any name, and one
/// [`DbusError::from_error_name`] does not know would be [`DbusError::Invalid`]'s detail, that
/// process's own text; the detail says only where it came from.
fn failure(what: &'static str, error: &zbus::Error) -> DbusError {
    let failure = DbusError::from_zbus(what, error);
    let named_by_callee = matches!(error, zbus::Error::MethodError(..));
    if named_by_callee && matches!(failure, DbusError::Invalid { .. }) {
        return DbusError::Invalid { what, detail: APPS_OWN_ERROR.to_owned() };
    }
    failure
}

/// What a failed call at an app's object means, as [`failure`] says. The bus answers a call to a
/// unique name that nothing holds with `ServiceUnknown`, which [`DbusError::from_zbus`] makes
/// [`DbusError::Unavailable`]; here that is the app gone, so the object is
/// [`DbusError::NotFound`], as one the app does not serve is. `Unavailable` is kept for what is
/// not there at all: no launcher, no accessibility bus, no registry.
fn object_failure(what: &'static str, error: &zbus::Error) -> DbusError {
    let failure = failure(what, error);
    if let DbusError::Unavailable { what } = failure {
        return DbusError::NotFound { what };
    }
    failure
}

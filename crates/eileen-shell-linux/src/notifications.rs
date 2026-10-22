//! Toasts on Linux: [`Notifier`], a client of the desktop's notification server, the
//! `org.freedesktop.Notifications` interface at `/org/freedesktop/Notifications` on the session
//! bus. The [Desktop Notifications Specification][spec] (version 1.3) defines it, and GNOME
//! Shell, KDE Plasma, dunst, mako and xfce4-notifyd serve it.
//!
//! [spec]: https://specifications.freedesktop.org/notification/latest/
//!
//! What each part serves (BUILD.md section 3.4; docs/LINUX.md section 4):
//!
//! - [`Notifier::notify`] serves `ShellHost::toast`, whose adapter drops the id it returns.
//! - [`Notifier::close`] and [`Notifier::events`] serve no trait method yet. They let a toast be
//!   withdrawn once what it asked about is settled, and tell when the user dismissed it.
//! - [`Notifier::capabilities`] and [`Notifier::server_information`] serve none either: they
//!   are for diagnostics.
//!
//! The rules:
//!
//! - Every toast is one `Notify` call: the app name `Eileen`, `replaces_id` 0, the settings'
//!   icon, the toast's title as the summary and its body as the body, no actions, the hints
//!   `desktop-entry` (the settings' desktop file ID) and `urgency` (the byte 1, normal), and
//!   `expire_timeout` -1, the server's default. `Toast::job` stays in Eileen.
//! - Eileen's text is plain. A server that lists the `body-markup` capability reads the body as
//!   markup, so for it `&`, `<` and `>` become `&amp;`, `&lt;` and `&gt;`. Any other server
//!   shows the body as written and gets it unchanged. The summary is never markup.
//! - A server is the program that owns the name, known by the unique name the bus gave its
//!   connection ([`NotificationServer`], such as `:1.42`). A server that restarts is a new one
//!   and may number its notifications from 1 again, so a [`NotificationId`] is the server and its
//!   number together, and never matches another server's notification.
//! - `notify` asks the bus which server owns the name ([`name_owner`]) and sends the toast to
//!   that server by its unique name, escaped for that server's capabilities. They are read the
//!   first time a server's are needed and kept with it, and read again once another server owns
//!   the name; a failed read is not kept. While no server owns the name, they are asked of the
//!   name itself, so that the bus can start a server; one it cannot start is
//!   [`DbusError::Unavailable`].
//! - `close` goes to the server that showed the notification, by its unique name, never to the
//!   name's owner of the moment. Closing a notification that is already gone succeeds. The
//!   specification's answer for it is an error with no set name, so `close` takes any error
//!   outside the bus's own `org.freedesktop.DBus.Error` names to mean that; and the bus's
//!   `ServiceUnknown` or `NameHasNoOwner` means the server itself is gone, and its
//!   notifications with it.
//! - No D-Bus string may hold U+0000, and the bus drops a connection that sends one, so it is
//!   removed from every string first.
//! - A notification's id is never zero: a server that answers zero gets
//!   [`DbusError::Invalid`].
//! - Every call is bounded by the connection's method timeout, which `eileen-dbus-linux`'s
//!   buses set, and one that passes it is [`DbusError::DeadlineExceeded`]: the client keeps no
//!   timer of its own.
//! - Errors carry the call and the error's name, never the title, the body or the server's
//!   message ([`DbusError::from_zbus`]).
//! - [`NotificationEvents`] yields the server's `NotificationClosed` and `ActionInvoked` signals
//!   and the name's owner changes, in the order the connection received them; the doc of its
//!   module, `events`, gives its rules and traps.
//!
//! Traps:
//!
//! - The toast goes to the owner the bus named. A server that exits between that answer and
//!   the `Notify` makes `notify` fail as [`DbusError::Unavailable`], and one that only lost the
//!   name in that time still gets the toast.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;
use std::sync::{Mutex, MutexGuard, PoisonError};

use eileen_contracts::core_io::Toast;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{DbusError, name_owner};
use zbus::names::{OwnedUniqueName, UniqueName};
use zbus::zvariant::{DynamicDeserialize, Value};

mod events;

pub use events::{NotificationCloseReason, NotificationEvent, NotificationEvents};

/// The server's well-known name.
const NAME: &str = "org.freedesktop.Notifications";
/// Its object.
const PATH: &str = "/org/freedesktop/Notifications";
/// Its interface.
const INTERFACE: &str = "org.freedesktop.Notifications";
/// How the bus's own error names begin: they keep their meaning for `close`.
const BUS_ERRORS: &str = "org.freedesktop.DBus.Error.";
/// The bus's answers for a name no program has: a well-known name nothing owns, or the unique
/// name of a connection that is gone.
const NO_SUCH_NAME: [&str; 2] =
    ["org.freedesktop.DBus.Error.ServiceUnknown", "org.freedesktop.DBus.Error.NameHasNoOwner"];

/// Every notification's `app_name`.
const APP_NAME: &str = "Eileen";
/// The `urgency` hint's normal level (0 is low, 2 critical).
const NORMAL_URGENCY: u8 = 1;
/// The `expire_timeout` that leaves the time to the server.
const SERVER_DEFAULT_TIMEOUT: i32 = -1;
/// The capability under which the server reads bodies as markup.
const BODY_MARKUP: &str = "body-markup";

/// A method this client calls: its member name, and the name errors give it.
#[derive(Debug, Clone, Copy)]
struct Method {
    member: &'static str,
    what: &'static str,
}

/// Shows a notification.
const NOTIFY: Method = Method { member: "Notify", what: "org.freedesktop.Notifications.Notify" };
/// Closes one.
const CLOSE: Method =
    Method { member: "CloseNotification", what: "org.freedesktop.Notifications.CloseNotification" };
/// Lists what the server can do.
const GET_CAPABILITIES: Method =
    Method { member: "GetCapabilities", what: "org.freedesktop.Notifications.GetCapabilities" };
/// Names the server.
const GET_SERVER_INFORMATION: Method = Method {
    member: "GetServerInformation",
    what: "org.freedesktop.Notifications.GetServerInformation",
};

/// How every notification names Eileen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifierSettings {
    /// The desktop file ID of Eileen's desktop entry, without `.desktop` (a trailing `.desktop`
    /// is dropped): the `desktop-entry` hint, by which the server can find the app's name, its
    /// icon and the user's notification settings for it. `eileen` by default.
    pub desktop_entry: String,
    /// The `app_icon`: an icon name from the icon theme, or a `file://` URI. Empty, the
    /// default, leaves the icon to the server.
    pub app_icon: String,
}

impl Default for NotifierSettings {
    fn default() -> Self {
        Self { desktop_entry: "eileen".to_owned(), app_icon: String::new() }
    }
}

/// A notification server: the unique name the bus gave the connection of the program that owns
/// the server's name, such as `:1.42`. A server that restarts, or another that takes the name
/// over, has a new one, while the well-known name stays.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationServer(OwnedUniqueName);

impl NotificationServer {
    /// The server a unique bus name names, such as `:1.42`; `None` for any other text, a
    /// well-known name or the empty text included.
    #[must_use]
    pub fn new(unique_name: &str) -> Option<Self> {
        if !unique_name.starts_with(':') {
            return None;
        }
        OwnedUniqueName::try_from(unique_name).ok().map(Self)
    }

    /// The unique name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The server a message's `sender` names.
    fn of(sender: &UniqueName<'_>) -> Self {
        Self(OwnedUniqueName::from(sender.to_owned()))
    }
}

/// A notification's id: the server that showed it and the number that server gave it, never
/// zero. The number alone names no notification, since a restarted server numbers afresh.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationId {
    server: NotificationServer,
    number: NonZeroU32,
}

impl NotificationId {
    /// The notification `server` numbered `number`; `None` for zero, which names no
    /// notification.
    #[must_use]
    pub fn new(server: NotificationServer, number: u32) -> Option<Self> {
        NonZeroU32::new(number).map(|number| Self { server, number })
    }

    /// The server's number for it.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.number.get()
    }

    /// The server that showed it.
    #[must_use]
    pub const fn server(&self) -> &NotificationServer {
        &self.server
    }
}

/// What a notification server can do, as `GetCapabilities` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCapabilities {
    names: BTreeSet<String>,
}

impl NotificationCapabilities {
    /// Whether the server lists `name`, such as `actions`, `body-markup` or `persistence`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Whether the server reads bodies as markup (`body-markup`), so that a plain body's `&`,
    /// `<` and `>` must be escaped.
    #[must_use]
    pub fn body_markup(&self) -> bool {
        self.contains(BODY_MARKUP)
    }
}

/// Which notification server this is, as `GetServerInformation` names it. Every field is the
/// server's own text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationServerInfo {
    /// The product, such as `gnome-shell` or `dunst`.
    pub name: Untrusted<String>,
    /// Its vendor.
    pub vendor: Untrusted<String>,
    /// Its version.
    pub version: Untrusted<String>,
    /// The version of the specification it follows, such as `1.2`.
    pub spec_version: Untrusted<String>,
}

/// A client of the desktop's notification server, on a connection its caller gives it: the
/// session bus's ([`eileen_dbus_linux::session_bus`]), or a private bus in tests.
#[derive(Debug)]
pub struct Notifier {
    connection: zbus::Connection,
    /// The `desktop-entry` hint.
    desktop_entry: String,
    /// The `app_icon`.
    app_icon: String,
    /// The capabilities last read, with the server they are of.
    kept: Mutex<Option<Kept>>,
}

/// A server's capabilities, kept with the server.
#[derive(Debug, Clone)]
struct Kept {
    server: NotificationServer,
    capabilities: NotificationCapabilities,
}

impl Notifier {
    /// A client on `connection`, which it clones, that names Eileen as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub fn new(connection: &zbus::Connection, settings: &NotifierSettings) -> Self {
        let desktop_entry = wire_text(&settings.desktop_entry);
        let desktop_entry = desktop_entry.strip_suffix(".desktop").unwrap_or(&desktop_entry);
        Self {
            connection: connection.clone(),
            desktop_entry: desktop_entry.to_owned(),
            app_icon: wire_text(&settings.app_icon).into_owned(),
            kept: Mutex::new(None),
        }
    }

    /// Shows `toast` (`Notify`, what `ShellHost::toast` asks) on the server that owns the name
    /// now, and returns the id that server gave it. A call that must read the server's
    /// capabilities first may fail with an error that names `GetCapabilities`, and one that
    /// cannot ask the bus who owns the name with one that names the name owner.
    pub async fn notify(&self, toast: &Toast) -> Result<NotificationId, DbusError> {
        let Kept { server, capabilities } = self.current().await?;
        let summary = wire_text(&toast.title);
        let body = body_text(&toast.body, capabilities.body_markup());
        let actions: &[&str] = &[];
        let hints = BTreeMap::from([
            ("desktop-entry", Value::from(self.desktop_entry.as_str())),
            ("urgency", Value::U8(NORMAL_URGENCY)),
        ]);
        let arguments = (
            APP_NAME,
            0_u32,
            self.app_icon.as_str(),
            summary.as_ref(),
            body.as_ref(),
            actions,
            hints,
            SERVER_DEFAULT_TIMEOUT,
        );
        let reply = self
            .connection
            .call_method(Some(server.as_str()), PATH, Some(INTERFACE), NOTIFY.member, &arguments)
            .await;
        let (shown_by, number) = answer::<u32>(NOTIFY, reply)?;
        NotificationId::new(shown_by, number).ok_or_else(|| DbusError::Invalid {
            what: NOTIFY.what,
            detail: "the server answered notification id 0".to_owned(),
        })
    }

    /// Closes the notification `id` (`CloseNotification`) on the server that showed it, which
    /// then sends `NotificationClosed` with [`NotificationCloseReason::Closed`]. A notification
    /// that is already gone, its server included, closes without error.
    pub async fn close(&self, id: &NotificationId) -> Result<(), DbusError> {
        let reply = self
            .connection
            .call_method(Some(id.server.as_str()), PATH, Some(INTERFACE), CLOSE.member, &id.get())
            .await;
        match reply {
            Ok(_) => Ok(()),
            Err(error) if already_gone(&error) => Ok(()),
            Err(error) => Err(DbusError::from_zbus(CLOSE.what, &error)),
        }
    }

    /// What the server that owns the name now can do (`GetCapabilities`): read the first time
    /// its capabilities are needed and kept with it, and read again once another server owns
    /// the name. A failed read is not kept, so the next call asks again; two calls racing to
    /// read may both ask.
    pub async fn capabilities(&self) -> Result<NotificationCapabilities, DbusError> {
        Ok(self.current().await?.capabilities)
    }

    /// Which server this is (`GetServerInformation`), asked anew each time.
    pub async fn server_information(&self) -> Result<NotificationServerInfo, DbusError> {
        let reply = self
            .connection
            .call_method(Some(NAME), PATH, Some(INTERFACE), GET_SERVER_INFORMATION.member, &())
            .await;
        let (_server, (name, vendor, version, spec_version)) =
            answer::<(String, String, String, String)>(GET_SERVER_INFORMATION, reply)?;
        Ok(NotificationServerInfo {
            name: Untrusted::new(name),
            vendor: Untrusted::new(vendor),
            version: Untrusted::new(version),
            spec_version: Untrusted::new(spec_version),
        })
    }

    /// The server that owns the name now, with its capabilities: the kept ones when they are
    /// its own, read and kept otherwise.
    async fn current(&self) -> Result<Kept, DbusError> {
        let owner = name_owner(&self.connection, NAME).await?.map(NotificationServer);
        let kept = self.kept().clone().filter(|kept| owner.as_ref() == Some(&kept.server));
        if let Some(kept) = kept {
            return Ok(kept);
        }
        // With no owner, the name itself is asked, so that the bus can start a server.
        let destination = owner.as_ref().map_or(NAME, NotificationServer::as_str);
        let reply = self
            .connection
            .call_method(Some(destination), PATH, Some(INTERFACE), GET_CAPABILITIES.member, &())
            .await;
        let (server, names) = answer::<Vec<String>>(GET_CAPABILITIES, reply)?;
        let capabilities = NotificationCapabilities { names: names.into_iter().collect() };
        let read = Kept { server, capabilities };
        *self.kept() = Some(read.clone());
        Ok(read)
    }

    /// The kept capabilities, locked. Nothing panics while they are locked, so a poisoned lock
    /// still holds a whole value.
    fn kept(&self) -> MutexGuard<'_, Option<Kept>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The program that sent `reply`, and its body as a `T`; or what the failed call to `method`
/// means.
fn answer<T>(
    method: Method,
    reply: zbus::Result<zbus::Message>,
) -> Result<(NotificationServer, T), DbusError>
where
    T: for<'d> DynamicDeserialize<'d>,
{
    let failed = |error: zbus::Error| DbusError::from_zbus(method.what, &error);
    let reply = reply.map_err(failed)?;
    let body = reply.body().deserialize().map_err(failed)?;
    let server = reply.header().sender().map(NotificationServer::of).ok_or_else(|| {
        DbusError::Invalid { what: method.what, detail: "the reply named no sender".to_owned() }
    })?;
    Ok((server, body))
}

/// Whether `error` says the notification no longer exists: an error reply whose name is not
/// one of the bus's own, which is the server's answer, or the bus's answer that the server
/// itself is gone.
fn already_gone(error: &zbus::Error) -> bool {
    let zbus::Error::MethodError(name, _, _) = error else {
        return false;
    };
    NO_SUCH_NAME.contains(&name.as_str()) || !name.as_str().starts_with(BUS_ERRORS)
}

/// `text` as a D-Bus string can carry it: without U+0000.
fn wire_text(text: &str) -> Cow<'_, str> {
    if text.contains('\0') { Cow::Owned(text.replace('\0', "")) } else { Cow::Borrowed(text) }
}

/// The body as the server should read it: plain text, escaped for a server that reads markup.
fn body_text(body: &str, markup: bool) -> Cow<'_, str> {
    let body = wire_text(body);
    if markup && body.contains(['&', '<', '>']) { Cow::Owned(escape_markup(&body)) } else { body }
}

/// `text` with `&`, `<` and `>` written as the markup's entities.
fn escape_markup(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{body_text, wire_text};

    #[test]
    fn a_body_is_escaped_for_a_server_that_reads_markup_and_only_then() {
        let body = "Fish & chips <b>now</b> > later; &amp; stays text";
        assert_eq!(
            body_text(body, true),
            "Fish &amp; chips &lt;b&gt;now&lt;/b&gt; &gt; later; &amp;amp; stays text",
            "every &, < and >, an entity's & included, since the text is plain"
        );
        assert_eq!(body_text(body, false), body, "a server without markup shows it as written");
        assert_eq!(body_text("no markup here", true), "no markup here", "nothing to escape");
        assert_eq!(body_text("", true), "", "empty");
    }

    #[test]
    fn nul_never_reaches_the_bus() {
        assert_eq!(wire_text("Disk\0 full\0"), "Disk full", "every U+0000 removed");
        assert_eq!(wire_text("plain"), "plain", "the rest untouched");
        assert_eq!(body_text("a\0<b>", true), "a&lt;b&gt;", "removed before escaping");
    }
}

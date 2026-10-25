//! Global shortcuts on Linux: [`ShortcutsSession`], a client of the `GlobalShortcuts` portal
//! (`org.freedesktop.portal.GlobalShortcuts`, [its documentation][portal]), which serves
//! `ShellHost::register_hotkey` and the dictation key of `ShellHost::watch_input` (BUILD.md
//! section 3.4; docs/LINUX.md sections 3 and 4). Its triggers are the `trigger` module's.
//!
//! [portal]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html
//!
//! The order a caller follows, on a session connection of its own:
//!
//! 1. [`ShortcutsSession::open`] registers the app with the portal's host registry
//!    (`org.freedesktop.host.portal.Registry`), then asks for a session (`CreateSession`) through
//!    `eileen-dbus-linux`'s `Request`. The registry takes an app only before the connection's
//!    first portal call, and only an app ID whose desktop file is installed; portals before
//!    1.19.4 lack it. Its error is passed over, which leaves the portal to find the app's ID
//!    itself, and from 1.21.0 it refuses a session to an app with none.
//! 2. [`ShortcutsSession::events`] subscribes to the session's signals, before any bind.
//! 3. [`ShortcutsSession::bind`] sends the whole set in one `BindShortcuts`, typically answered
//!    once the user has seen the desktop's dialog. A session takes one bind, and GNOME answers a
//!    second with 2, so a set that changes is bound again in a new session.
//! 4. [`ShortcutsSession::close`] ends the session; the connection's close ends it too.
//!
//! The rules:
//!
//! - The `Response` codes are the Request documentation's: 1 is a refusal, "cancelled by the
//!   user", 2 one ended another way, "refused or cancelled", and any other
//!   [`DbusError::Invalid`]. GNOME answers a dialog the user dismissed with 2, since its settings
//!   refuse the bind; it gives 1 only to a request the caller has closed, which the frontend no
//!   longer reports.
//! - A portal without the interface, as on GNOME 46, answers `UnknownMethod` or
//!   `UnknownInterface`, [`DbusError::NotFound`]; no portal at all is
//!   [`DbusError::Unavailable`].
//! - The session's handle comes as a string, an erratum the documentation keeps for
//!   compatibility, and xdg-desktop-portal sends it so.
//! - Only the connection that answered `CreateSession` speaks for the portal, and only a signal
//!   that names the session's handle is the session's: any other, and a malformed one, is
//!   passed over. The stream ends after the session's `Closed`. The bindings' later changes
//!   (`ShortcutsChanged`) are not followed yet.
//! - A trigger's description is the desktop's text, so it is `Untrusted`. No D-Bus string may
//!   hold U+0000, and the bus drops a connection that sends one, so the app ID and each
//!   description lose any first.
//!
//! Traps:
//!
//! - Every signal stream's traps hold (`eileen-dbus-linux`'s crate doc): poll [`ShortcutEvents`]
//!   steadily or drop it, inside the tokio runtime's context. It takes every signal the portal
//!   sends under its object, other portals' broadcasts among them, which keeps the session's
//!   `Closed` in order with its shortcuts' signals.
//! - A portal that restarts loses the session without a word: the stream sees no `Closed`.

mod trigger;

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{Request, Signals, Stream, drain, subscribe};
use zbus::match_rule::Builder;
use zbus::names::OwnedUniqueName;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message};

use crate::DbusError;

pub use trigger::{trigger_of_chord, trigger_of_key};

/// The portal's service and object, and the interfaces this client calls there.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const SESSION: &str = "org.freedesktop.portal.Session";
const REGISTRY: &str = "org.freedesktop.host.portal.Registry";

/// What the errors name, each a bare noun.
const SESSION_WHAT: &str = "GlobalShortcuts session";
const BIND_WHAT: &str = "GlobalShortcuts binding";
const SIGNALS_WHAT: &str = "GlobalShortcuts signals";
const RESPONSE_WHAT: &str = "portal response";

/// A shortcut to bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// Its ID, which the desktop keeps the user's trigger under: keep it the same across runs.
    pub id: String,
    /// What it does, as the desktop's dialog shows it.
    pub description: String,
    /// The trigger to offer the user, in the shortcuts specification's form (`CTRL+ALT+m`, from
    /// [`trigger_of_chord`] or [`trigger_of_key`]); `None` leaves the choice to the user.
    pub trigger: Option<String>,
}

/// A shortcut the desktop bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundShortcut {
    /// Its ID, as the caller gave it.
    pub id: String,
    /// How to trigger it, in the desktop's words (GNOME's "Press Ctrl+Alt+M"); empty when the
    /// desktop gave none, as for a shortcut the user left without one.
    pub trigger: Untrusted<String>,
}

/// What the portal says of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutEvent {
    /// The shortcut with this ID went down (`Activated`).
    Activated(String),
    /// It came back up (`Deactivated`).
    Deactivated(String),
    /// The portal closed the session (the session's `Closed`), and the stream ends.
    Closed,
}

/// A session of the `GlobalShortcuts` portal on its caller's connection. Clones share the
/// session. The module's doc gives the order of calls and the rules.
#[derive(Debug, Clone)]
pub struct ShortcutsSession {
    conn: Connection,
    handle: OwnedObjectPath,
    /// The unique name of the portal's connection, which answered `CreateSession`.
    portal: OwnedUniqueName,
}

impl ShortcutsSession {
    /// A session on `conn`, a connection of the caller's own that has made no portal call yet,
    /// for the app `app_id`: the basename of its desktop file, which the portal's host registry
    /// asks for. The portal's answer is waited for within `deadline`.
    pub async fn open(
        conn: &Connection,
        app_id: &str,
        deadline: Duration,
    ) -> Result<Self, DbusError> {
        let no_options = HashMap::<&str, Value<'_>>::new();
        let app_id = app_id.replace('\0', "");
        let registered = (app_id.as_str(), &no_options);
        // Passed over: a portal without the registry, or one that refuses the ID, still serves.
        let _ignored = conn
            .call_method(Some(PORTAL), PORTAL_PATH, Some(REGISTRY), "Register", &registered)
            .await;
        let mut request = Request::new(conn)?;
        request.responses().await?;
        let reply = {
            // The request's token names the session too: its handle has a prefix of its own.
            let token = request.token();
            let options = HashMap::from([
                ("handle_token", Value::from(token)),
                ("session_handle_token", Value::from(token)),
            ]);
            conn.call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SHORTCUTS),
                "CreateSession",
                &(options,),
            )
            .await
            .map_err(failed(SESSION_WHAT))?
        };
        let portal = request.follow(&reply).await?;
        let response = request.answer(&portal, deadline).await?;
        confirmed(SESSION_WHAT, response.code)?;
        let handle =
            response.results.get("session_handle").and_then(|text| <&str>::try_from(text).ok());
        let handle =
            handle.and_then(|text| OwnedObjectPath::try_from(text).ok()).ok_or_else(|| {
                DbusError::Invalid { what: SESSION_WHAT, detail: "no session handle".to_owned() }
            })?;
        Ok(Self { conn: conn.clone(), handle, portal })
    }

    /// The session's handle.
    #[must_use]
    pub fn handle(&self) -> &ObjectPath<'_> {
        &self.handle
    }

    /// The session's events from now on. Call it before [`Self::bind`], and poll and drop the
    /// stream in the tokio runtime's context.
    pub async fn events(&self) -> Result<ShortcutEvents, DbusError> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(PORTAL)
            .and_then(|rule| rule.path_namespace(PORTAL_PATH))
            .map(Builder::build);
        let signals = subscribe(&self.conn, rule, SIGNALS_WHAT).await?;
        let (portal, session) = (self.portal.clone(), self.handle.clone());
        Ok(ShortcutEvents { signals, portal, session, ended: false })
    }

    /// Binds `shortcuts`, the session's whole set, waiting at most `deadline` for the answer,
    /// which typically comes once the user has seen the desktop's dialog: the shortcuts the
    /// desktop bound, which may be fewer. Past the deadline the request is closed and the answer
    /// is [`DbusError::DeadlineExceeded`]. GNOME leaves its dialog up all the same: an accept
    /// that comes later is stored but binds nothing, and the next bind of those shortcuts gets
    /// them without a dialog.
    pub async fn bind(
        &self,
        shortcuts: &[Shortcut],
        deadline: Duration,
    ) -> Result<Vec<BoundShortcut>, DbusError> {
        let mut request = Request::new(&self.conn)?;
        request.responses().await?;
        let reply = {
            let list: Vec<_> = shortcuts.iter().map(Shortcut::to_portal).collect();
            let options = HashMap::from([("handle_token", Value::from(request.token()))]);
            let body = (&self.handle, list, "", options);
            self.conn
                .call_method(Some(PORTAL), PORTAL_PATH, Some(SHORTCUTS), "BindShortcuts", &body)
                .await
                .map_err(failed(BIND_WHAT))?
        };
        let portal = request.follow(&reply).await?;
        let mut response = request.answer(&portal, deadline).await?;
        confirmed(BIND_WHAT, response.code)?;
        let list = response.results.remove("shortcuts").map(Vec::<Described>::try_from);
        let list = list.transpose().map_err(|_malformed| DbusError::Invalid {
            what: BIND_WHAT,
            detail: "the bound shortcuts are not a(sa{sv})".to_owned(),
        })?;
        Ok(bound(list.unwrap_or_default()))
    }

    /// Ends the session (`Session.Close`), answered within the connection's method timeout.
    pub async fn close(self) -> Result<(), DbusError> {
        self.conn
            .call_method(Some(PORTAL), &self.handle, Some(SESSION), "Close", &())
            .await
            .map(|_reply| ())
            .map_err(failed(SESSION_WHAT))
    }
}

impl Shortcut {
    /// The shortcut as `BindShortcuts` takes it: its ID, and its description and trigger.
    fn to_portal(&self) -> (&str, HashMap<&'static str, Value<'_>>) {
        let mut options =
            HashMap::from([("description", Value::from(self.description.replace('\0', "")))]);
        if let Some(trigger) = &self.trigger {
            options.insert("preferred_trigger", Value::from(trigger.as_str()));
        }
        (self.id.as_str(), options)
    }
}

/// A session's events, from [`ShortcutsSession::events`], as a stream the caller drives with
/// [`next`](Self::next) or as a [`Stream`]; nothing runs on its own. The module's doc gives its
/// rules and traps.
#[derive(Debug)]
#[must_use = "a stream does nothing unless polled"]
pub struct ShortcutEvents {
    /// The signals the portal sends under its object.
    signals: Signals,
    portal: OwnedUniqueName,
    session: OwnedObjectPath,
    /// Whether the session's `Closed`, or the connection's end, has been passed on.
    ended: bool,
}

impl ShortcutEvents {
    /// The next event; `None` after [`ShortcutEvent::Closed`] or once the connection is gone.
    /// Cancel-safe.
    pub async fn next(&mut self) -> Option<ShortcutEvent> {
        std::future::poll_fn(|cx| self.poll_event(cx)).await
    }

    /// Polls for the next event, passing over at most
    /// [`DRAIN_CAP`](eileen_dbus_linux::DRAIN_CAP) signals that make none before it yields.
    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<ShortcutEvent>> {
        if self.ended {
            return Poll::Ready(None);
        }
        let (portal, session) = (&self.portal, &self.session);
        let polled = drain(&mut self.signals, cx, |signal| event_of(&signal, portal, session));
        self.ended = matches!(polled, Poll::Ready(Some(ShortcutEvent::Closed) | None));
        polled
    }
}

impl Stream for ShortcutEvents {
    type Item = ShortcutEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<ShortcutEvent>> {
        self.get_mut().poll_event(cx)
    }
}

/// The event `signal` reports: one the session's portal, `portal`, sent for `session`, with the
/// arguments the documentation gives.
fn event_of(
    signal: &Message,
    portal: &OwnedUniqueName,
    session: &OwnedObjectPath,
) -> Option<ShortcutEvent> {
    let header = signal.header();
    let (path, interface, member) = (header.path()?, header.interface()?, header.member()?);
    if header.sender() != Some(&**portal) {
        return None;
    }
    if interface.as_str() == SESSION && member.as_str() == "Closed" {
        return (path.as_str() == session.as_str()).then_some(ShortcutEvent::Closed);
    }
    let edge: fn(String) -> ShortcutEvent = match member.as_str() {
        "Activated" => ShortcutEvent::Activated,
        "Deactivated" => ShortcutEvent::Deactivated,
        _ => return None,
    };
    if interface.as_str() != SHORTCUTS || path.as_str() != PORTAL_PATH {
        return None;
    }
    let (handle, id, _time, _options) = signal.body().deserialize::<Edge>().ok()?;
    (handle == *session).then(|| edge(id))
}

/// An `Activated` or `Deactivated` signal's arguments: the session's handle, the shortcut's ID,
/// the time and the options.
type Edge = (OwnedObjectPath, String, u64, HashMap<String, OwnedValue>);

/// A shortcut as the portal describes it: its ID and its details.
type Described = (String, HashMap<String, OwnedValue>);

/// The shortcuts in `list` with the trigger each one's details describe.
fn bound(list: Vec<Described>) -> Vec<BoundShortcut> {
    let trigger = |details: &HashMap<String, OwnedValue>| {
        let described =
            details.get("trigger_description").and_then(|text| <&str>::try_from(text).ok());
        Untrusted::new(described.unwrap_or_default().to_owned())
    };
    list.into_iter().map(|(id, details)| BoundShortcut { trigger: trigger(&details), id }).collect()
}

/// What a `Response` code says of a request to `what`, by the Request documentation's codes.
fn confirmed(what: &'static str, code: u32) -> Result<(), DbusError> {
    match code {
        0 => Ok(()),
        1 => Err(DbusError::Refused { what, reason: "cancelled by the user" }),
        2 => Err(DbusError::Refused { what, reason: "refused or cancelled" }),
        other => Err(DbusError::Invalid { what: RESPONSE_WHAT, detail: format!("code {other}") }),
    }
}

/// Maps a failed call to `what`.
fn failed(what: &'static str) -> impl Fn(zbus::Error) -> DbusError {
    move |error| DbusError::from_zbus(what, &error)
}

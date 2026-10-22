//! The session's lock state on Linux, from logind: [`SessionLock::session_locked`] serves
//! `ShellHost::session_locked` and [`SessionLock::lock_workstation`] serves
//! `ShellHost::lock_workstation` (BUILD.md sections 3.4 and 4.8; docs/LINUX.md sections 3 and 4).
//! An armed fleet run skips a locked session, records `skipped: locked` when a policy locks it
//! anyway, and locks the workstation when it ends.
//!
//! Both use `/org/freedesktop/login1/session/auto` on the system bus: the caller's session or,
//! for a process outside every session, the user's display session ([logind-dbus.c]). Where
//! systemd runs the desktop, its apps run in the user's own systemd units rather than in the
//! session, so `auto` finds the session where `self` would find none.
//! [`SessionLock::session_locked`] reads the session's `LockedHint` afresh on every call, and
//! [`SessionLock::lock_workstation`] calls its `Lock()`.
//!
//! # Traps
//!
//! - `LockedHint` is only what the desktop reports through `SetLockedHint`, which "is intended
//!   to be used by the desktop environment to tell systemd-logind when the session is locked and
//!   unlocked" ([logind's D-Bus API]). GNOME Shell ([screenShield.js]) and KDE Plasma's screen
//!   locker ([kscreenlocker]) set it. Window managers and wlroots compositors whose locker does
//!   not, such as swaylock, leave it false, so a locked screen there reads unlocked.
//! - `Lock()` only asks. logind emits the session's `Lock` signal, which the desktop's locker acts
//!   on (GNOME Shell's screen shield listens for it), and answers before the screen is locked; a
//!   desktop that does not listen never locks. [`SessionLock::session_locked`] confirms where the
//!   desktop sets the hint. logind lets the session's owner ask without authentication: its
//!   polkit action, `org.freedesktop.login1.lock-sessions`, passes the session's own user.
//! - A caller with neither a session nor a display session has none to read or lock, and logind
//!   answers both calls with `org.freedesktop.DBus.Error.UnknownObject`: its lookup of
//!   `session/auto` drops the `org.freedesktop.login1.NoSessionForPID` it makes and finds no
//!   object (`session_object_find` in [logind-session-dbus.c]). That becomes
//!   [`DbusError::NotFound`], as does a logind older than `LockedHint` (systemd 230).
//!   `NoSessionForPID` itself, which logind's manager calls send, becomes
//!   [`DbusError::NotFound`] too, as a guard.
//!
//! [logind's D-Bus API]: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html
//! [logind-dbus.c]: https://github.com/systemd/systemd/blob/main/src/login/logind-dbus.c
//! [logind-session-dbus.c]: https://github.com/systemd/systemd/blob/main/src/login/logind-session-dbus.c
//! [screenShield.js]: https://gitlab.gnome.org/GNOME/gnome-shell/-/blob/main/js/ui/screenShield.js
//! [kscreenlocker]: https://invent.kde.org/plasma/kscreenlocker/-/merge_requests/10

use zbus::proxy::CacheProperties;

use crate::DbusError;

/// logind's error for a caller that has no session and no display session. Its manager's calls
/// send it; on `session/auto` the caller gets `UnknownObject` instead (the module's doc says
/// why), so this is a guard.
const NO_SESSION_FOR_PID: &str = "org.freedesktop.login1.NoSessionForPID";

/// What the errors name.
const SESSION: &str = "logind session";

/// The caller's logind session (`org.freedesktop.login1.Session`).
#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1/session/auto"
)]
trait LoginSession {
    /// Asks the session's screen locker to lock.
    fn lock(&self) -> zbus::Result<()>;

    /// Whether the desktop reports the session locked.
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;
}

/// The session's lock state, and the request to lock it, over logind on the system bus.
#[derive(Debug, Clone)]
pub struct SessionLock {
    system: zbus::Connection,
}

impl SessionLock {
    /// A client of logind on `system`, the system bus.
    #[must_use]
    pub fn new(system: &zbus::Connection) -> Self {
        Self { system: system.clone() }
    }

    /// Whether the session is locked, as the desktop reports it to logind (`LockedHint`), read
    /// afresh; the module's doc says what that misses.
    pub async fn session_locked(&self) -> Result<bool, DbusError> {
        let session = self.session().await?;
        session.locked_hint().await.map_err(|error| session_error(&error))
    }

    /// Asks the session to lock (`Lock()`), at the end of an armed run. It returns once logind has
    /// asked the desktop's locker, not once the screen is locked.
    pub async fn lock_workstation(&self) -> Result<(), DbusError> {
        let session = self.session().await?;
        session.lock().await.map_err(|error| session_error(&error))
    }

    /// The caller's session, with no property cache: every read asks logind, and the proxy adds
    /// no match rule.
    async fn session(&self) -> Result<LoginSessionProxy<'static>, DbusError> {
        LoginSessionProxy::builder(&self.system)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|error| session_error(&error))
    }
}

/// What a failed session call means: [`DbusError::from_zbus`], which makes the `UnknownObject`
/// a caller with no session gets [`DbusError::NotFound`]. `NoSessionForPID`, the guard, is
/// [`DbusError::NotFound`] too.
fn session_error(error: &zbus::Error) -> DbusError {
    if let zbus::Error::MethodError(name, _, _) = error
        && name.as_str() == NO_SESSION_FOR_PID
    {
        return DbusError::NotFound { what: SESSION };
    }
    DbusError::from_zbus(SESSION, error)
}

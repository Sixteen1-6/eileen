//! Keeping the session awake on Linux: [`LinuxPowerRequest::keep_awake`] serves
//! `ShellHost::keep_awake` (BUILD.md sections 3.4 and 4.8; docs/LINUX.md sections 3 and 4). An
//! armed fleet run "holds a display-required and system-required power request for its
//! duration, so the session neither sleeps nor locks mid-run".
//!
//! # Two holders
//!
//! Windows' one power request is two holders here, and the request takes both:
//!
//! - **Sleep, from logind on the system bus.** `Inhibit("sleep:idle", "Eileen", why, "block")`
//!   on `org.freedesktop.login1.Manager` holds until the file descriptor it returns "and all its
//!   duplicates are closed" ([logind's D-Bus API]). It stops logind's own idle action, and other
//!   users' suspends unless polkit lets them ignore inhibitors. Before systemd 257 it did not stop
//!   the user who took it ([systemd's NEWS] for 257; Ubuntu 24.04 runs 255), so a power manager
//!   in the user's session that calls `Suspend()` on its idle timer passes it. Nor can logind
//!   keep the screen on: the desktop has the "screensaver avoidance locks which logind lacks"
//!   ([inhibitor locks]).
//! - **Suspend and idle, from the desktop on the session bus.** GNOME's session manager first,
//!   `org.gnome.SessionManager.Inhibit("Eileen", 0, why, 12)` for suspend (4) and idle (8), so
//!   the screen neither blanks nor locks on idle, ended by `Uninhibit(cookie)` ([GNOME's session
//!   manager]). Where nothing owns that name (KDE Plasma, other desktops, a sandbox), the Inhibit
//!   portal's `Inhibit("", 12, {handle_token, reason})`, ended by closing its request ([the
//!   Inhibit portal]).
//!
//! GNOME's session manager comes first. On Ubuntu 24.04's GNOME 46 the portal cannot hold for an
//! app outside a sandbox: it passes an empty app ID, which gnome-session refused until 49.alpha.0
//! ([gnome-session issue 89]). And GNOME answers once the inhibitor exists, while the portal
//! answers before its backend has run and tells the outcome only in the request's `Response`
//! signal ([the portal's inhibit.c]). On GNOME the portal's backend makes the same call
//! ([xdg-desktop-portal-gtk's inhibit.c]), so asking directly holds the same thing.
//!
//! # The portal's answer
//!
//! A portal request holds only once its `Response` says 0. `eileen-dbus-linux`'s [`Request`]
//! subscribes to it before the call, follows the handle a portal before 0.9 names instead,
//! counts only a `Response` from the connection that answered the call, and closes a request
//! that misses its deadline.
//!
//! - **0:** the backend took the request, which is not always the desktop holding:
//!   xdg-desktop-portal-gtk answers before GNOME's session manager or the screen saver does, and
//!   xdg-desktop-portal-kde before KDE's power manager. Nor is it always every flag asked for:
//!   the portal passes its backend only the flags the permission store allows.
//! - **1:** the user cancelled it: a refusal, not asked again.
//! - **2:** the backend refused. Suspend and idle are asked again for idle alone (8): where
//!   xdg-desktop-portal-gtk is the backend and GNOME's session manager is absent, it falls back
//!   to `org.freedesktop.ScreenSaver` and refuses any flag there but idle.
//! - **No answer by the deadline:** the request is closed and missed. The portal sends no
//!   `Response` at all when the permission store masks every flag asked for
//!   ([the portal's inhibit.c]), so a store that turns inhibiting off shows as a
//!   [`DbusError::DeadlineExceeded`] miss once the whole deadline has passed, never as a quick
//!   refusal.
//! - **`org.freedesktop.portal.Error.NotAllowed` for the call:** a refusal
//!   ([`DbusError::Refused`], "not allowed"), by `eileen-dbus-linux`'s mapping. Other portals
//!   send it when the desktop's lockdown turns them off, but xdg-desktop-portal's `Inhibit`
//!   handler never does: it checks no lockdown setting and refuses only flags it does not know
//!   ([the portal's inhibit.c]). The mapping is defensive here, for other portal
//!   implementations.
//!
//! The deadline is one budget for the portal's requests and their answers, signals that no method
//! timeout bounds; each method call keeps its connection's method timeout, as
//! `eileen-dbus-linux`'s deadline model has it. A holder that is absent, refuses or does not
//! confirm is named in [`LinuxPowerRequest::missed`], and the request fails only when nothing
//! holds. A run whose session locks anyway records `skipped: locked` (BUILD.md section 4.8).
//!
//! # Releasing
//!
//! [`LinuxPowerRequest`] mirrors `eileen-platform`'s `PowerRequest` (`handles.rs`, unmerged), so
//! the trait returns `PowerRequest::new(move || drop(request))`, a closure that may run on any
//! thread and cannot wait for a D-Bus answer.
//!
//! - Dropping the request closes logind's descriptor, which ends that lock at once.
//! - The request owns the session connection the desktop's hold was taken on. GNOME's session
//!   manager drops an inhibitor whose caller leaves the bus (`gsm-inhibitor.c`), and the portal a
//!   disconnected caller's requests (`xdp-request.c`), so when the request holds that connection
//!   alone, dropping it ends the desktop's hold too, once the bus has seen it close.
//! - It holds no signal stream, whose drop needs the tokio runtime (`eileen-dbus-linux`'s stream
//!   traps): the portal's [`Request`] ends inside [`LinuxPowerRequest::keep_awake`].
//! - [`LinuxPowerRequest::release`] ends both now and says whether the desktop agreed:
//!   `Uninhibit(cookie)`, or the portal request's `Close()` ([`close_request`]), answered.
//!
//! # Traps
//!
//! - A `zbus::MessageStream` over the whole system connection that is not polled keeps every
//!   message it has queued, the reply that carried logind's descriptor among them, and with it a
//!   duplicate of the descriptor: the lock then outlives the request.
//! - Give the request a session connection of its own, a new `eileen_dbus_linux::session_bus()`:
//!   it is the one argument taken by value, because closing it is how a dropped request ends the
//!   desktop's hold. A caller that keeps clones of it must call [`LinuxPowerRequest::release`].
//!
//! [logind's D-Bus API]: https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html
//! [systemd's NEWS]: https://github.com/systemd/systemd/blob/main/NEWS
//! [inhibitor locks]: https://systemd.io/INHIBITOR_LOCKS/
//! [GNOME's session manager]: https://gnome.pages.gitlab.gnome.org/gnome-session/re04.html
//! [the Inhibit portal]: https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Inhibit.html
//! [gnome-session issue 89]: https://gitlab.gnome.org/GNOME/gnome-session/-/issues/89
//! [the portal's inhibit.c]: https://github.com/flatpak/xdg-desktop-portal/blob/main/desktop-portal/inhibit.c
//! [xdg-desktop-portal-gtk's inhibit.c]: https://github.com/flatpak/xdg-desktop-portal-gtk/blob/main/src/inhibit.c

use std::collections::HashMap;
use std::fmt;
use std::os::fd::OwnedFd;
use std::time::Duration;

use eileen_dbus_linux::{Request, close_request};
use tokio::time::Instant;
use zbus::zvariant::{self, OwnedObjectPath};

use crate::DbusError;

/// logind's lock: who takes it (GNOME's app ID too), what it inhibits (suspend and hibernation,
/// and logind's idle action) and its mode (it blocks, rather than delays).
const WHO: &str = "Eileen";
const WHAT: &str = "sleep:idle";
const MODE: &str = "block";

/// The desktop's inhibitor flags in GNOME's and the portal's numbering: suspending, and marking
/// the session idle.
const SUSPEND: u32 = 4;
const IDLE: u32 = 8;

/// The portal's service, its object, and the interface of its inhibitions.
const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT: &str = "/org/freedesktop/portal/desktop";
const PORTAL_INHIBIT: &str = "org.freedesktop.portal.Inhibit";

/// What the errors name, each a bare noun.
const LOGIND_LOCK: &str = "logind inhibitor lock";
const GNOME: &str = "GNOME session manager";
const PORTAL: &str = "Inhibit portal";
const RESPONSE: &str = "portal response";
const REASON: &str = "keep-awake reason";
const HOLDER: &str = "keep-awake holder";

/// The portal's `Response` code 2 to an inhibition: its backend refused or failed.
const BACKEND_FAILED: DbusError = DbusError::Refused { what: PORTAL, reason: "its backend failed" };

/// logind's manager (`org.freedesktop.login1.Manager`), for its inhibitor lock.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LoginManager {
    /// A lock on `what` that lasts while the returned descriptor is open.
    fn inhibit(
        &self,
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> zbus::Result<zvariant::OwnedFd>;
}

/// GNOME's session manager (`org.gnome.SessionManager`), for its inhibitors.
#[zbus::proxy(
    interface = "org.gnome.SessionManager",
    default_service = "org.gnome.SessionManager",
    default_path = "/org/gnome/SessionManager"
)]
trait GnomeSessionManager {
    /// An inhibitor of `flags`, named by the returned cookie. The second argument, once a
    /// window's X11 ID, is ignored.
    fn inhibit(
        &self,
        app_id: &str,
        toplevel_xid: u32,
        reason: &str,
        flags: u32,
    ) -> zbus::Result<u32>;

    /// Ends the inhibitor `inhibit_cookie` names.
    fn uninhibit(&self, inhibit_cookie: u32) -> zbus::Result<()>;
}

/// Which desktop holder keeps the session from going idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DesktopHolder {
    /// GNOME's session manager, `org.gnome.SessionManager`.
    GnomeSession,
    /// The Inhibit portal, `org.freedesktop.portal.Inhibit`.
    Portal,
}

/// The holds [`Self::keep_awake`] took: the Linux side of `eileen-platform`'s `PowerRequest`,
/// which it mirrors (`handles.rs`, on the unmerged base crates). Dropping it releases them, on any
/// thread: logind's lock at once, and the desktop's hold once the bus sees the session connection
/// close, when the request held that connection alone (the module's doc says how).
/// [`release`](Self::release) ends both now.
#[must_use = "dropping the request releases it"]
pub struct LinuxPowerRequest {
    /// logind's lock, which holds while the descriptor is open.
    sleep: Option<OwnedFd>,
    /// The desktop's hold, and what ending it needs.
    desktop: Option<DesktopHold>,
    /// Why logind's lock or the desktop's hold is missing.
    missed: Vec<DbusError>,
}

/// The desktop's hold: the connection it was taken on, which the holder ties it to, and how to
/// end it.
struct DesktopHold {
    connection: zbus::Connection,
    end: End,
}

/// How a desktop hold ends.
enum End {
    /// `Uninhibit` with GNOME's cookie.
    Gnome(u32),
    /// `Close` on the portal's request, at its handle.
    Portal(OwnedObjectPath),
}

impl DesktopHold {
    const fn holder(&self) -> DesktopHolder {
        match self.end {
            End::Gnome(_) => DesktopHolder::GnomeSession,
            End::Portal(_) => DesktopHolder::Portal,
        }
    }
}

impl LinuxPowerRequest {
    /// Keeps the PC from sleeping and the session from going idle and locking until the request
    /// drops: logind's lock on `system`, then the desktop's hold on `session`, GNOME's session
    /// manager first and the Inhibit portal second. An `Err` stands for a bus the caller could not
    /// reach, and is that holder's miss: [`DbusError::Unavailable`] for a bus that is not there,
    /// a holder merely absent, or [`DbusError::DeadlineExceeded`] for one that did not answer the
    /// connect in time. Give `session` to the request alone, a new
    /// `eileen_dbus_linux::session_bus()`: the request keeps it, and closing it is how a dropped
    /// request ends the desktop's hold.
    ///
    /// `why` is the reason the desktop and `systemd-inhibit --list` show beside "Eileen"; GNOME's
    /// session manager refuses an empty one, so a blank `why` is [`DbusError::Invalid`].
    ///
    /// `deadline` bounds the portal's requests and the `Response` signals that answer them, which
    /// no method timeout bounds. A request not confirmed by then is closed and holds nothing; its
    /// miss is [`DbusError::DeadlineExceeded`]. logind and GNOME's session manager answer their
    /// calls directly, within the connections' method timeouts.
    ///
    /// It fails only when nothing holds: with logind's error, or the desktop's when logind was
    /// merely absent, or [`DbusError::Unavailable`] when every holder is absent.
    pub async fn keep_awake(
        system: Result<&zbus::Connection, DbusError>,
        session: Result<zbus::Connection, DbusError>,
        why: &str,
        deadline: Duration,
    ) -> Result<Self, DbusError> {
        if why.trim().is_empty() {
            return Err(DbusError::Invalid { what: REASON, detail: "empty".to_owned() });
        }
        let sleep = match system {
            Ok(system) => inhibit_sleep(system, why).await,
            Err(unreached) => Err(unreached),
        };
        let desktop = match session {
            Ok(session) => hold_desktop(session, why, deadline).await,
            Err(unreached) => Err(unreached),
        };
        match (sleep, desktop) {
            (Err(sleep), Err(desktop)) => Err(if absent(&sleep) && absent(&desktop) {
                DbusError::Unavailable { what: HOLDER }
            } else if absent(&sleep) {
                desktop
            } else {
                sleep
            }),
            (sleep, desktop) => {
                let mut missed = Vec::new();
                let sleep = sleep.map_err(|error| missed.push(error)).ok();
                let desktop = desktop.map_err(|error| missed.push(error)).ok();
                Ok(Self { sleep, desktop, missed })
            }
        }
    }

    /// Whether logind's lock holds: logind's idle action waits, and so do other users' requests
    /// to suspend or hibernate. Before systemd 257 the user's own requests pass it (the module's
    /// doc says why).
    #[must_use]
    pub const fn holds_sleep(&self) -> bool {
        self.sleep.is_some()
    }

    /// The holder that keeps the desktop from going idle, blanking and locking, if any.
    #[must_use]
    pub fn desktop(&self) -> Option<DesktopHolder> {
        self.desktop.as_ref().map(DesktopHold::holder)
    }

    /// Why a holder holds nothing: logind's failure first, then the desktop's, each only when it
    /// holds nothing. Empty when both hold.
    #[must_use]
    pub fn missed(&self) -> &[DbusError] {
        &self.missed
    }

    /// Releases everything now: closes logind's lock, then ends the desktop's hold with
    /// `Uninhibit(cookie)` or the portal request's `Close()` and waits for the answer. When that
    /// call fails the request is gone all the same, and the desktop drops its hold once the
    /// session connection closes.
    pub async fn release(self) -> Result<(), DbusError> {
        let Self { sleep, desktop, missed: _ } = self;
        drop(sleep);
        let Some(DesktopHold { connection, end }) = desktop else {
            return Ok(());
        };
        match end {
            End::Gnome(cookie) => {
                let manager =
                    GnomeSessionManagerProxy::new(&connection).await.map_err(failed(GNOME))?;
                manager.uninhibit(cookie).await.map_err(failed(GNOME))
            }
            End::Portal(handle) => close_request(&connection, &handle, PORTAL).await,
        }
    }
}

impl fmt::Debug for LinuxPowerRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinuxPowerRequest")
            .field("sleep", &self.holds_sleep())
            .field("desktop", &self.desktop())
            .field("missed", &self.missed)
            .finish()
    }
}

/// logind's lock on suspend, hibernation and the idle action.
async fn inhibit_sleep(system: &zbus::Connection, why: &str) -> Result<OwnedFd, DbusError> {
    let manager = LoginManagerProxy::new(system).await.map_err(failed(LOGIND_LOCK))?;
    let lock = manager.inhibit(WHAT, WHO, why, MODE).await.map_err(failed(LOGIND_LOCK))?;
    Ok(lock.into())
}

/// The desktop's hold on suspend and idle: GNOME's session manager, else the portal. The error is
/// GNOME's unless GNOME's session manager was absent. On an error `session` drops here: when it
/// is the request's own, as [`LinuxPowerRequest::keep_awake`] asks, it closes, which ends any
/// portal request that does not hold.
async fn hold_desktop(
    session: zbus::Connection,
    why: &str,
    deadline: Duration,
) -> Result<DesktopHold, DbusError> {
    let gnome = match gnome_inhibit(&session, why).await {
        Ok(cookie) => return Ok(DesktopHold { connection: session, end: End::Gnome(cookie) }),
        Err(error) => error,
    };
    match portal_inhibit(&session, why, deadline).await {
        Ok(handle) => Ok(DesktopHold { connection: session, end: End::Portal(handle) }),
        Err(portal) => Err(if absent(&gnome) { portal } else { gnome }),
    }
}

/// An inhibitor of suspend and idle from GNOME's session manager: its cookie.
async fn gnome_inhibit(session: &zbus::Connection, why: &str) -> Result<u32, DbusError> {
    let manager = GnomeSessionManagerProxy::new(session).await.map_err(failed(GNOME))?;
    manager.inhibit(WHO, 0, why, SUSPEND | IDLE).await.map_err(failed(GNOME))
}

/// An inhibition of suspend and idle from the portal, or of idle alone when its backend refuses
/// both: the request's handle, once the portal has confirmed it within `deadline`, one budget
/// for both requests.
async fn portal_inhibit(
    session: &zbus::Connection,
    why: &str,
    deadline: Duration,
) -> Result<OwnedObjectPath, DbusError> {
    let until = Instant::now().checked_add(deadline);
    let answer = portal_request(session, why, SUSPEND | IDLE, until).await;
    if answer != Err(BACKEND_FAILED) {
        return answer;
    }
    portal_request(session, why, IDLE, until).await
}

/// One `Inhibit` call of `flags` to the portal, with no window, and the `Response` it ends in by
/// `until` (`None` for never): the request's handle when that says it holds.
async fn portal_request(
    session: &zbus::Connection,
    why: &str,
    flags: u32,
    until: Option<Instant>,
) -> Result<OwnedObjectPath, DbusError> {
    let mut request = Request::new(session)?;
    request.responses().await?;
    let reply = {
        let options = HashMap::from([
            ("handle_token", zvariant::Value::from(request.token())),
            ("reason", zvariant::Value::from(why)),
        ]);
        let body = ("", flags, options);
        session
            .call_method(
                Some(PORTAL_SERVICE),
                PORTAL_OBJECT,
                Some(PORTAL_INHIBIT),
                "Inhibit",
                &body,
            )
            .await
            .map_err(failed(PORTAL))?
    };
    let portal = request.follow(&reply).await?;
    let left = until.map_or(Duration::MAX, |until| until.saturating_duration_since(Instant::now()));
    let code = request.wait(&portal, left).await?;
    confirmed(code)?;
    Ok(request.path().to_owned().into())
}

/// What a `Response` code says of an inhibition, by the Request documentation's codes: 0 done, 1
/// cancelled by the user, 2 ended another way, which for an inhibition is its backend's failure.
fn confirmed(code: u32) -> Result<(), DbusError> {
    match code {
        0 => Ok(()),
        1 => Err(DbusError::Refused { what: PORTAL, reason: "cancelled by the user" }),
        2 => Err(BACKEND_FAILED),
        other => Err(DbusError::Invalid { what: RESPONSE, detail: format!("code {other}") }),
    }
}

/// Maps a failed call to `what`.
fn failed(what: &'static str) -> impl Fn(zbus::Error) -> DbusError {
    move |error| DbusError::from_zbus(what, &error)
}

/// Whether a holder is merely not there: no service, or no such interface or method.
const fn absent(error: &DbusError) -> bool {
    matches!(error, DbusError::Unavailable { .. } | DbusError::NotFound { .. })
}

#[cfg(test)]
mod tests {
    use super::{BACKEND_FAILED, DbusError, absent, confirmed};

    #[test]
    fn only_a_missing_service_or_member_counts_as_absent() {
        assert!(absent(&DbusError::Unavailable { what: "x" }), "no service");
        assert!(absent(&DbusError::NotFound { what: "x" }), "no interface or method");
        assert!(!absent(&DbusError::Refused { what: "x", reason: "not authorized" }), "refused");
        assert!(
            !absent(&DbusError::Invalid { what: "x", detail: "y".to_owned() }),
            "a malformed reply"
        );
        assert!(
            !absent(&DbusError::DeadlineExceeded { call: "x" }),
            "a portal that never answered"
        );
    }

    #[test]
    fn only_response_code_0_confirms_an_inhibition() {
        assert_eq!(confirmed(0), Ok(()), "done");
        assert_eq!(
            confirmed(1),
            Err(DbusError::Refused { what: "Inhibit portal", reason: "cancelled by the user" }),
            "cancelled"
        );
        assert_eq!(confirmed(2), Err(BACKEND_FAILED), "the backend failed");
        let odd = confirmed(3).unwrap_err();
        assert_eq!(
            odd,
            DbusError::Invalid { what: "portal response", detail: "code 3".to_owned() },
            "a code the documentation does not give"
        );
        assert_eq!(odd.to_string(), "invalid portal response: code 3", "a bare noun reads well");
    }
}

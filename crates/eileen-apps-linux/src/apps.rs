//! [`LinuxApps`]: `eileen-platform`'s `Apps` over the desktop entries and [`Opener`], through
//! the D-Bus bridge. `index` reads the desktop entries with [`index`] on the caller's thread,
//! with std file reads and no bus; `launch` and the opening methods run [`Opener`] on the
//! bridge's session bus. Which apps the menu shows, how an app starts and how each target
//! opens are the `index` and `open` modules' to decide: this converts types and errors.
//!
//! # The entries
//!
//! The trait's `AppEntry` is the crate's [`AppEntry`] field for field. `id` is the desktop file
//! ID, which names an app to the desktop on Linux as the AUMID the trait's doc names does on
//! Windows, for packaged and desktop apps alike; `name` is the menu's localized name; `kind` is
//! the same [`AppKind`]; `path` is the desktop entry file, still `Personal`, as a user's own
//! entry lives under their home folder. `launch` and `open_path` with an app find its
//! [`DesktopEntry`] by that ID with [`entry_for`] among the entries of the last `index`, which
//! the adapter keeps for this. An app not among them, as before the first `index` or one
//! installed since, takes a new index, and one not in that either is `NotFound`.
//!
//! # The deadlines
//!
//! No method takes a `Deadline`, so each bridge call waits at most [`DEFAULT_DEADLINE`], the
//! connection's method timeout and half a second. The portal's `Response` gets [`RESPONSE_WAIT`]
//! of it, so that the opener's own wait ends first and closes the request, which dismisses any
//! dialog the portal shows; past the bridge's deadline the call would be cancelled and leave the
//! request open. An app chooser the portal shows, as when no default app is set, is dismissed when
//! that wait ends: the trait leaves the user no longer to choose.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use eileen_contracts::desktop::WindowRef;
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, METHOD_TIMEOUT};
use eileen_platform::{self as platform, Apps, Deadline, PlatformError};

use crate::{AppEntry, AppKind, DesktopEntry, FileError, Opener, Session, entry_for, index};

/// How long the opener waits for the portal's `Response`, and for `open_path` the checks of the
/// path with it: half the connection's method timeout. The calls before the wait have the other
/// half and the bridge's half second, far more than they take on a working bus.
const RESPONSE_WAIT: Duration = match METHOD_TIMEOUT.checked_div(2) {
    Some(half) => half,
    None => METHOD_TIMEOUT,
};

/// What `launch` names when it passes its deadline.
const LAUNCH: &str = "app launch";
/// What `open_url` names when it passes its deadline.
const OPEN_URL: &str = "opening a web address";
/// What `open_uri` names when it passes its deadline.
const OPEN_URI: &str = "opening a URI";
/// What `open_path` names when it passes its deadline.
const OPEN_PATH: &str = "opening a file or folder";
/// What `main_window` names as unavailable.
const MAIN_WINDOW: &str = "an app's main window";

/// The apps of the user's session: `Apps` on Linux, from the desktop entries of a [`Session`],
/// started and opened by [`Opener`] on a [`Bridge`]. The module's doc gives the rules.
///
/// - `index` is [`index`] of the session. Each call reads the entries again, on the caller's
///   thread with no deadline, as the trait gives none, and keeps them.
/// - `launch` is [`Opener::launch`]: `Activate` on the app's `org.freedesktop.Application`,
///   which has the bus start the app. Its process ID is that of the connection that answered,
///   the app's own for an app on the host. A Flatpak app whose bus access is filtered answers
///   through `xdg-dbus-proxy`, whose ID it then probably is (yet to be measured on Ubuntu
///   26.04): no closer value is on the bus. An app that is not D-Bus activatable is
///   `Unavailable`: starting it from its `Exec` line waits for `eileen_supervisor::user_launch`
///   and the supervisor's Linux process model (docs/LINUX.md sections 5 and 7).
/// - `main_window` is `Unavailable` until phase L2 finds windows through AT-SPI and the
///   compositor (docs/LINUX.md section 4).
/// - `open_url`, `open_uri` and `open_path` without an app are [`Opener::open_url`],
///   [`Opener::open_uri`] and [`Opener::open_path`], through the `OpenURI` portal. `open_path`
///   with an app is [`Opener::open_path_with`], the app's own `Open`: `Unavailable` for an app
///   that is not D-Bus activatable or is packaged, which also waits for `user_launch`.
#[derive(Debug)]
pub struct LinuxApps {
    bridge: Bridge,
    session: Session,
    /// The entries of the last index, for `launch` and `open_path` with an app.
    indexed: Mutex<Vec<DesktopEntry>>,
}

impl LinuxApps {
    /// The apps of `session`, [`Session::from_env`] in the apps process, started and opened on
    /// `bridge`'s session bus. It reads nothing yet.
    #[must_use]
    pub const fn new(bridge: Bridge, session: Session) -> Self {
        Self { bridge, session, indexed: Mutex::new(Vec::new()) }
    }

    /// The indexed entry of `app`: from the last index, else from a new one, which is kept.
    fn desktop_entry(&self, app: &platform::AppEntry) -> Result<DesktopEntry, FileError> {
        let wanted = from_platform(app);
        let kept = entry_for(&self.indexed(), &wanted).ok().cloned();
        if let Some(entry) = kept {
            return Ok(entry);
        }
        let entries = index(&self.session);
        let found = entry_for(&entries, &wanted).cloned();
        *self.indexed() = entries;
        found
    }

    /// The kept entries. A thread that panicked while holding them left a whole list, so a
    /// poisoned lock is taken all the same.
    fn indexed(&self) -> std::sync::MutexGuard<'_, Vec<DesktopEntry>> {
        self.indexed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Apps for LinuxApps {
    fn index(&self) -> Result<Vec<platform::AppEntry>, PlatformError> {
        let entries = index(&self.session);
        let listed = entries.iter().map(|entry| to_platform(AppEntry::from(entry))).collect();
        *self.indexed() = entries;
        Ok(listed)
    }

    fn launch(&self, app: &platform::AppEntry) -> Result<u32, PlatformError> {
        let entry = self.desktop_entry(app)?;
        self.bridge.call(LAUNCH, DEFAULT_DEADLINE, move |buses| async move {
            Ok(Opener::new(&buses.session().await?).launch(&entry).await?)
        })
    }

    /// `Unavailable` until phase L2: AT-SPI's frames and the compositor.
    fn main_window(
        &self,
        _pid: u32,
        _deadline: Deadline,
    ) -> Result<Option<WindowRef>, PlatformError> {
        Err(PlatformError::Unavailable { what: MAIN_WINDOW })
    }

    fn open_url(&self, url: &str) -> Result<(), PlatformError> {
        let url = url.to_owned();
        self.bridge.call(OPEN_URL, DEFAULT_DEADLINE, move |buses| async move {
            Ok(Opener::new(&buses.session().await?).open_url(&url, RESPONSE_WAIT).await?)
        })
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        let uri = uri.to_owned();
        self.bridge.call(OPEN_URI, DEFAULT_DEADLINE, move |buses| async move {
            Ok(Opener::new(&buses.session().await?).open_uri(&uri, RESPONSE_WAIT).await?)
        })
    }

    fn open_path(
        &self,
        path: &str,
        with_app: Option<&platform::AppEntry>,
    ) -> Result<(), PlatformError> {
        let path = PathBuf::from(path);
        let app = with_app.map(|app| self.desktop_entry(app)).transpose()?;
        self.bridge.call(OPEN_PATH, DEFAULT_DEADLINE, move |buses| async move {
            let opener = Opener::new(&buses.session().await?);
            match app {
                Some(app) => opener.open_path_with(&path, &app).await?,
                None => opener.open_path(&path, RESPONSE_WAIT).await?,
            }
            Ok(())
        })
    }
}

/// The trait's entry for an indexed app, field for field.
fn to_platform(app: AppEntry) -> platform::AppEntry {
    let kind = match app.kind {
        AppKind::Packaged => platform::AppKind::Packaged,
        AppKind::Desktop => platform::AppKind::Desktop,
    };
    platform::AppEntry { id: app.id, name: app.name, kind, path: app.path }
}

/// The crate's entry for the trait's, field for field, which [`entry_for`] looks up.
fn from_platform(app: &platform::AppEntry) -> AppEntry {
    let kind = match app.kind {
        platform::AppKind::Packaged => AppKind::Packaged,
        platform::AppKind::Desktop => AppKind::Desktop,
    };
    AppEntry { id: app.id.clone(), name: app.name.clone(), kind, path: app.path.clone() }
}

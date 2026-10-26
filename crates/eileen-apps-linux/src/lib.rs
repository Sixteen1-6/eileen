//! The apps process's platform implementations on Linux: what `eileen-apps-win` does on Windows
//! (BUILD.md section 3.4; docs/LINUX.md section 4), on XDG files and the desktop's D-Bus services.
//!
//! # Owns
//!
//! - The trait implementations, thin adapters over the clients below: `eileen-platform`'s `Apps`
//!   for [`LinuxApps`], `RecentSources` for [`LinuxRecent`], `MediaSessions` for [`LinuxMedia`].
//! - For `Apps`: the index of desktop entries (the XDG Desktop Entry Specification 1.5): their
//!   format, which entries the menu shows, the data folders' precedence, desktop file IDs and which
//!   apps are packaged; and [`Opener`]: opening through the `OpenURI` portal, and D-Bus activation
//!   for the entry that [`entry_for`] finds from the trait's `AppEntry`.
//! - For `RecentSources`: reading `recently-used.xbel` (the Desktop Bookmark Specification 0.8.5):
//!   which bookmarks are local files, when each was last opened, by which app, of what kind.
//!   [`RecentItem`] keeps the app's name `Untrusted`, as another app wrote it.
//! - For `MediaSessions`: [`MprisSessions`] (which bus names are players, their properties as
//!   `MediaState`s, each `MediaAction`'s call) and [`MprisWatch`]'s rules: which signals read a
//!   player again, folded into one read per player, and `Gone` when a player loses its name.
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0), whose types [`AppEntry`], [`AppKind`], [`RecentItem`] and
//!   [`FileError`] mirror, but for the app's name, for the adapters to convert; the D-Bus plumbing
//!   and the bridge the adapters call through (`eileen-dbus-linux`'s, [`DbusError`] re-exported).
//! - Launching apps that are not D-Bus activatable, and opening files with packaged apps: they need
//!   `eileen_supervisor::user_launch` and its Linux process model (docs/LINUX.md section 5).
//! - Ranking recent files, and the media history: the recent-work index (BUILD.md section 2.7).
//! - Matching a spoken name to an app (the app-name scorer, WP5).
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - It starts apps only through the desktop, never as its children: the portal starts the default
//!   app, and D-Bus activation has the bus start the app (BUILD.md section 2.1). Clippy's
//!   disallowed methods keep `Command` out. It calls a media player only through the unique name
//!   that owns it, so no call starts one; a test pins a missing player as `Unavailable`.
//! - The first data folder holding a desktop file ID decides it, as the specification orders them:
//!   `XDG_DATA_HOME`, read once into [`Session`] for the recent files too, then `XDG_DATA_DIRS`.
//! - Another app's text is sanitized and kept to 512 characters: a media player's where the MPRIS
//!   readers read it, `Untrusted`, and a recent file's app name where [`LinuxRecent`] unwraps it.
//! - No error holds a path, a URI, a file's text or a service's message: only I/O kinds, error
//!   names and fixed descriptions, which the tests check.
//!
//! # Boundaries
//!
//! - May depend on `eileen-platform` (the traits), `eileen-contracts` (the `ids` types, `FileKind`
//!   and the media types), `eileen-dbus-linux` (the D-Bus helpers and the bridge), `eileen-stdx`
//!   (file work's threads), `zbus`, `tokio`, `rustix` (open flags) and `quick-xml` (the bookmarks).
//!   Nothing in the workspace may depend on it except the apps process's composition root on Linux.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-apps-linux`.
//! - Layers: unit tests for the formats and the MPRIS values and calls; property tests for the
//!   desktop entry lists and locale keys; the fuzz target `desktop_entry`, whose seed corpus runs
//!   as a test; integration tests over `tests/fixtures/` (see its README), the live session, and
//!   fakes on `eileen-dbus-linux`'s private bus, the adapters' through `Bridge::on_private_bus`.
//!
//! # Traps
//!
//! - A user's `Hidden=true` copy removes an app, and a user's `NoDisplay=true` copy keeps it out of
//!   the menu: both decide the ID, so lower folders' copies stay hidden. A file that is not a valid
//!   application entry (another `Type`, no `Name`, no `Exec`) does not decide its ID.
//! - `XDG_DATA_DIRS` usually lists Flatpak's and Snap's exports through the login profile; a
//!   runtime started without that profile would miss those apps, so the index adds them.
//! - `GLib` refuses a whole bookmark file over a bad date or a repeated `href`; the reader skips
//!   that bookmark, and lists a path once at its newest time, as it does when two URIs name it.
//!   A `private` bookmark is hidden from every app that did not register it, Eileen included.
//! - A hung mount may keep for good the thread a file read or check runs on, its own or a pool's.
//! - The D-Bus traps (unpolled streams, streams dropped off the runtime) are `eileen-dbus-linux`'s.
//!   An [`MprisWatch`] has them; [`LinuxMedia`] runs it as a task on the bridge, which keeps them.
//!   Its `subscribe` sink runs on the bridge's one thread: a sink that blocks stalls every call.
//!
//! # Logs and tracing
//!
//! - None yet: spans, whose names `eileen-trace` lists, join around the trait calls and the watch.
//!
//! # Do not change without a decision record
//!
//! - The precedence and masking rules, which are the specification's.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod apps;
mod bounded;
mod entry;
mod error;
#[doc(hidden)]
pub mod fuzz_entry;
mod index;
mod keyfile;
mod media;
mod mpris;
mod open;
mod recent;
mod recent_sources;

pub use apps::LinuxApps;
pub use eileen_dbus_linux::DbusError;
pub use entry::DesktopEntry;
pub use error::FileError;
pub use index::{AppEntry, AppKind, Session, entry_for, index};
pub use media::LinuxMedia;
pub use mpris::{MprisEvent, MprisSessions, MprisWatch};
pub use open::{OpenError, Opener};
pub use recent::{RecentItem, RecentlyUsed};
pub use recent_sources::LinuxRecent;

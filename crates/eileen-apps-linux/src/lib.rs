//! The apps process's platform implementations on Linux: what `eileen-apps-win` does on Windows
//! (BUILD.md section 3.4; docs/LINUX.md section 4). Phase L1 starts with the app index, [`index`]:
//! every app the desktop's menu shows, as `shell:AppsFolder` gives it on Windows.
//!
//! # Owns
//!
//! - For `Apps`: the index of desktop entries (the XDG Desktop Entry Specification 1.5): their
//!   format, which entries the menu shows, the data folders' precedence, desktop file IDs and which
//!   apps are packaged; and [`Opener`]: opening through the `OpenURI` portal, and D-Bus activation
//!   for the entry that [`entry_for`] finds from the trait's `AppEntry`.
//! - For `RecentSources`: reading `recently-used.xbel` (the Desktop Bookmark Specification 0.8.5):
//!   which bookmarks are local files, when each was last opened, by which app, of what kind.
//!   [`RecentItem`] keeps the app's name `Untrusted`, as another app wrote it.
//! - For `MediaSessions`: the MPRIS client, [`MprisSessions`]: which bus names are players, how
//!   their properties become `MediaState`s, and each `MediaAction`'s call.
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0). Thin adapters will serve them; until they merge,
//!   [`AppEntry`], [`AppKind`], [`RecentItem`] and [`FileError`] (`PlatformError` for files) mirror
//!   `eileen-platform`'s types field for field, but for `RecentItem`'s app name.
//! - Launching apps that are not D-Bus activatable, and opening files with packaged apps: they need
//!   `eileen_supervisor::user_launch` and its Linux process model (docs/LINUX.md section 5).
//! - Ranking recent files, and the media history: the recent-work index (BUILD.md section 2.7).
//! - Matching a spoken name to an app (the app-name scorer, WP5).
//! - D-Bus plumbing: `eileen-dbus-linux`'s, [`DbusError`] (re-exported here) included.
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
//! - A media player's text is `Untrusted` from where it is read, sanitized and kept to 512
//!   characters: the MPRIS readers return no other kind, and their unit tests pin it.
//! - No error holds a path, a URI, a file's text or a service's message: only I/O kinds, error
//!   names and fixed descriptions, which the tests check.
//!
//! # Boundaries
//!
//! - May depend on `eileen-contracts` (the `ids` types, `FileKind` and the media types),
//!   `eileen-dbus-linux` (the D-Bus helpers), `zbus`, `tokio` (files open on its blocking pool),
//!   `rustix` (open flags) and `quick-xml` (the bookmark file). Nothing in the workspace may depend
//!   on it except the apps process's composition root on Linux.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-apps-linux`.
//! - Layers: unit tests for the formats and the MPRIS values and calls; property tests for the
//!   desktop entry lists and locale keys; the fuzz target `desktop_entry`, whose seed corpus
//!   replays as a test; integration tests over `tests/fixtures/`, which its README describes, and
//!   over the live session; fakes on `eileen-dbus-linux`'s private bus for the D-Bus clients.
//!
//! # Traps
//!
//! - A user's `Hidden=true` copy removes an app, and a user's `NoDisplay=true` copy keeps it
//!   out of the menu: both decide the ID, so lower folders' copies stay hidden.
//! - A file that is not a valid application entry (another `Type`, no `Name`, no `Exec`) does
//!   not decide its ID: the specification gives no meaning to it.
//! - `XDG_DATA_DIRS` usually lists Flatpak's and Snap's exports through the login profile; a
//!   runtime started without that profile would miss those apps, so the index adds them.
//! - `GLib` refuses a whole bookmark file over a bad date or a repeated `href`; the reader skips
//!   that bookmark, and lists a path once at its newest time, as it does when two URIs name it.
//!   A `private` bookmark is hidden from every app that did not register it, Eileen included.
//! - The D-Bus traps (unpolled streams, streams dropped off the runtime) are `eileen-dbus-linux`'s.
//!   An [`MprisWatch`] has them, so a `Subscription`'s release must drop it inside the runtime.
//!
//! # Logs and tracing
//!
//! - None yet: spans join with the trait implementations.
//!
//! # Do not change without a decision record
//!
//! - The precedence and masking rules, which are the specification's.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod entry;
mod error;
#[doc(hidden)]
pub mod fuzz_entry;
mod index;
mod keyfile;
mod mpris;
mod open;
mod recent;

pub use eileen_dbus_linux::DbusError;
pub use entry::DesktopEntry;
pub use error::FileError;
pub use index::{AppEntry, AppKind, Session, entry_for, index};
pub use mpris::{MprisEvent, MprisSessions, MprisWatch};
pub use open::{OpenError, Opener};
pub use recent::{RecentItem, RecentlyUsed};

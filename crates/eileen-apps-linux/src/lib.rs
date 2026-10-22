//! The apps process's platform implementations on Linux: what `eileen-apps-win` does on
//! Windows (BUILD.md section 3.4; docs/LINUX.md section 4). Phase L1 starts with the app index,
//! [`index`]: every application the desktop's menu shows, read from the XDG desktop entries,
//! as `shell:AppsFolder` gives it on Windows. [`Opener`] opens web addresses, URIs, files and
//! folders for the user through the desktop's `OpenURI` portal, and starts D-Bus-activatable
//! apps.
//!
//! # Owns
//!
//! - The index, from desktop entries (the XDG Desktop Entry Specification 1.5): their format,
//!   which entries the menu shows (`Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`,
//!   `TryExec`), the precedence of data folders, desktop file IDs, and which apps are packaged.
//! - Opening and launching for the user ([`Opener`]): `Apps::open_url`, `open_uri` and
//!   `open_path` through the `OpenURI` portal, and `Apps::launch` and `open_path` with an app
//!   through D-Bus activation, for the entry that [`entry_for`] finds from the trait's `AppEntry`.
//! - The trait types, field for field until `eileen-platform` merges: [`AppEntry`] and
//!   [`AppKind`] (`Apps`); and [`FileError`] for local files, which [`OpenError`] can hold.
//!
//! # Does not own
//!
//! - The `Apps` trait (`eileen-platform`, WP0). When it merges, the index implements
//!   `Apps::index`, [`Opener`] serves the opening calls, and the mirrored types give way.
//! - Launching apps that are not D-Bus activatable, and opening files with packaged apps: those
//!   need `eileen_supervisor::user_launch` (`gio launch`) and the supervisor's Linux process
//!   model (docs/LINUX.md section 5).
//! - Matching a spoken name to an app (the app-name scorer, WP5).
//! - D-Bus plumbing: [`DbusError`], the buses, name owners, the portal request and signal
//!   streams are `eileen-dbus-linux`'s, and [`DbusError`] is re-exported here for this crate's
//!   D-Bus clients.
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - It starts apps only through the desktop, never as its children: the portal starts the
//!   default app, and D-Bus activation has the bus start the app (BUILD.md section 2.1).
//!   Clippy's disallowed methods keep `Command` out.
//! - The first data folder holding a desktop file ID decides it, as the specification orders
//!   them: `XDG_DATA_HOME`, then `XDG_DATA_DIRS`.
//! - No error holds a path, a URI, a file's text or a service's message: only I/O kinds, error
//!   names and fixed descriptions, which the tests check.
//!
//! # Boundaries
//!
//! - May depend on `eileen-contracts` (the `Personal` wrapper), `eileen-dbus-linux` (the D-Bus
//!   helpers), `zbus`, `tokio` (files open on its blocking pool) and `rustix` (open flags).
//!   Nothing in the workspace may depend on it except the apps process's composition root on
//!   Linux.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-apps-linux`.
//! - Layers: unit tests for the format, and property tests for its lists and locale keys; the
//!   fuzz target `desktop_entry` (module `fuzz_entry`), whose seed corpus replays as a test;
//!   integration tests over a fixture session in `tests/fixtures/session/` (a home folder, the
//!   system folders and Flatpak's and Snap's exports); one live test over the session the
//!   tests run in. D-Bus clients are tested against fakes on `eileen-dbus-linux`'s private bus
//!   (its `private-bus` feature, on for this crate's tests only).
//!
//! # Traps
//!
//! - A user's `Hidden=true` copy removes an app, and a user's `NoDisplay=true` copy keeps it
//!   out of the menu: both decide the ID, so lower folders' copies stay hidden.
//! - A file that is not a valid application entry (another `Type`, no `Name`, no `Exec`) does
//!   not decide its ID: the specification gives no meaning to it.
//! - `XDG_DATA_DIRS` usually lists Flatpak's and Snap's exports through the login profile; a
//!   runtime started without that profile would miss those apps, so the index adds them.
//! - The D-Bus traps (unpolled streams, streams dropped off the runtime) are
//!   `eileen-dbus-linux`'s.
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
mod open;

pub use eileen_dbus_linux::DbusError;
pub use entry::DesktopEntry;
pub use error::FileError;
pub use index::{AppEntry, AppKind, Session, entry_for, index};
pub use open::{OpenError, Opener};

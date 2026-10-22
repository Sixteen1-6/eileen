//! The apps process's platform implementations on Linux: what `eileen-apps-win` does on
//! Windows (BUILD.md section 3.4; docs/LINUX.md section 4). Phase L1 starts with the app index,
//! [`index`]: every application the desktop's menu shows, read from the XDG desktop entries,
//! as `shell:AppsFolder` gives it on Windows.
//!
//! # Owns
//!
//! - The desktop entry format (the XDG Desktop Entry Specification 1.5): groups, keys,
//!   localized keys, escapes and lists.
//! - The index: which entries the menu shows (`Hidden`, `NoDisplay`, `OnlyShowIn`,
//!   `NotShowIn`, `TryExec`), the precedence of data folders, desktop file IDs, and which apps
//!   are packaged (Flatpak, Snap).
//! - [`AppEntry`] and [`AppKind`]: the `Apps` trait's index types, field for field, until
//!   `eileen-platform` merges.
//!
//! # Does not own
//!
//! - The `Apps` trait (`eileen-platform`, WP0). When it merges, the index implements
//!   `Apps::index` and this crate's two types give way to the trait's.
//! - Launching: `Apps::launch` goes through `eileen_supervisor::user_launch` (D-Bus activation
//!   or `gio launch`), which needs the supervisor's Linux process model (docs/LINUX.md section
//!   5).
//! - Matching a spoken name to an app (the app-name scorer, WP5).
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - Read only: it reads desktop entries and never runs what they name.
//! - The first data folder holding a desktop file ID decides it, as the specification orders
//!   them: `XDG_DATA_HOME`, then `XDG_DATA_DIRS`.
//!
//! # Boundaries
//!
//! - May depend on `eileen-contracts` (the `Personal` wrapper). Nothing in the workspace may
//!   depend on it except the apps process's composition root on Linux.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-apps-linux`.
//! - Layers: unit tests for the format; integration tests over a fixture session in
//!   `tests/fixtures/session/` (a home folder, the system folders and Flatpak's and Snap's
//!   exports); one live test over the session the tests run in.
//!
//! # Traps
//!
//! - A user's `Hidden=true` copy removes an app, and a user's `NoDisplay=true` copy keeps it
//!   out of the menu: both decide the ID, so lower folders' copies stay hidden.
//! - A file that is not a valid application entry (another `Type`, no `Name`, no `Exec`) does
//!   not decide its ID: the specification gives no meaning to it.
//! - `XDG_DATA_DIRS` usually lists Flatpak's and Snap's exports through the login profile; a
//!   runtime started without that profile would miss those apps, so the index adds them.
//!
//! # Logs and tracing
//!
//! - None yet: the index is a short synchronous read. Spans join with the trait implementation.
//!
//! # Do not change without a decision record
//!
//! - The precedence and masking rules, which are the specification's.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod entry;
mod index;
mod keyfile;

pub use entry::DesktopEntry;
pub use index::{AppEntry, AppKind, Session, index};

//! The desktop process's platform implementations on Linux, as `eileen-desktop-win`'s on Windows
//! (BUILD.md sections 2.6 and 3.4; docs/LINUX.md sections 3 and 4): so far `Grounding`.
//!
//! # Owns
//!
//! - [`LinuxDesktop`], with [`LinuxDesktopSettings`]: `Grounding` over `eileen-dbus-linux`'s
//!   bridge, from each app's windows ([`window_id`] gives their `hwnd`), the foreground window,
//!   the focused element and its selection; what Wayland withholds is `Unavailable`.
//! - [`Atspi`], Eileen's own client of the accessibility bus on zbus (no `atspi` crate is pinned):
//!   the registry's apps and their processes, each object's name, role, states and interfaces
//!   ([`Accessible`]), children, text, selection, box and automation id, and a bounded walk
//!   ([`Walk`]); [`accessibility_bus`] with [`CALL_TIMEOUT`], and [`toolkit_accessibility`].
//! - Tables of AT-SPI's numbers and names: [`Role`], [`State`], [`StateSet`], [`Interfaces`].
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0). `Automation`, `TextInsertion`, `Input`, `Capture`,
//!   `SystemVolume` and `Grounding::subscribe`, on AT-SPI's events, come next.
//! - D-Bus plumbing ([`DbusError`], re-exported, the buses, the bridge): `eileen-dbus-linux`'s.
//! - The desktop server's tools and safety rules (`eileen-desktop`, WP5).
//!
//! # Invariants
//!
//! - Linux only: `cfg(target_os = "linux")`, an empty library elsewhere.
//! - It never reads a password field's text: [`Atspi::text`] and [`Atspi::selection`] refuse the
//!   `password text` role before any `Text` call, as Windows refuses `IsPassword` (BUILD.md 2.6;
//!   tests `password_text_is_refused_without_asking_for_it` and the grounding password test).
//! - It writes nothing: not `IsEnabled`, which would turn on every Qt, Chromium and Firefox tree
//!   (2.6), nor any app's state (test `toolkit_accessibility_is_read_and_never_written`).
//! - Every app text is sanitized (`eileen_contracts::desktop::sanitize`) and bounded, and errors
//!   never carry it, nor an error name an app chose. A role, a state or an interface means only
//!   what this crate's tables say: a window's class is its role's name, never the app's text.
//! - A window's pid is the one the bus's daemon gives for its app's connection, never what the
//!   app says, and its process is named from that pid (test `a_window_names_its_apps_own_process`).
//!   Its id is the same in every process (`window_id`'s golden vector).
//! - Every call is bounded: on the accessibility bus by its connection's call timeout, a walk by
//!   its `TreeLimits` and its budget, checked between reads, and a trait call by its deadline on
//!   the bridge, with no timer of its own around a call (`eileen-dbus-linux`'s deadline model).
//! - Unix sockets only (`bus_at_within`): no launcher's answer runs a program or leaves the PC.
//!
//! # Boundaries
//!
//! - Called by the desktop composition root. May depend on `eileen-platform`, `eileen-contracts`,
//!   `eileen-dbus-linux`, `zbus`, `tokio` (3.1 adds `eileen-stdx`, `eileen-trace`); no wire types.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-desktop-linux`.
//! - Layers: unit tests of the tables and the window id; integration tests in `tests/it/` with a
//!   fake launcher on one private bus and a fake registry and apps on a second, one app in a
//!   process of its own, and `Grounding` through `Bridge::on_private_bus`.
//!   A walk's budget passes in paused time; real time only waits a 100 ms timeout or deadline.
//! - A bug here starts as a window list or a tree that misses what is on screen, a call that runs
//!   long, or a read of a field Eileen must not read.
//!
//! # Traps
//!
//! - Qt, Chromium and Electron, and Firefox put their trees on the bus only while `IsEnabled` is
//!   on or a screen reader runs; GTK apps always do. A listing without them means both are off.
//! - `ChildCount` can be wrong, and `GetChildren` on a big table makes the app build every child:
//!   such an object is read child by child by index, and only as far as the room goes.
//! - libatspi gives a new app 15 s, then 800 ms; [`CALL_TIMEOUT`] is 800 ms from an app's start.
//! - Every call connects to the accessibility bus anew, and a listing reads one app after another:
//!   each that does not answer spends a call timeout and leaves the front window unknown.
//! - The connect is not bounded: zbus 5.19 times no handshake, so a socket that accepts and never
//!   answers holds [`accessibility_bus`] until its caller's deadline (the bridge's, in a trait).
//! - Wayland tells a GTK window nothing of being minimized: only a toolkit's `ICONIFIED` says so.
//! - Only Firefox's and Chromium's English names mark a private window, and a class is `frame` or
//!   `dialog`: BUILD.md 2.6's and 2.10's private-window rules and 2.6's terminal tier need more.
//! - A second at-spi-bus-launcher on the session's runtime folder and display takes the live bus.
//! - nextest runs each test whose path holds `::desktop::` alone: no module here is so named.
//!
//! # Logs and tracing
//!
//! - None yet: spans join with the trait implementations, named from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - [`CALL_TIMEOUT`], the walk's bounds and [`FOCUS_LIMITS`] (BUILD.md section 2.1, rule 2), the
//!   password rule, and [`window_id`]'s hash, which every process must compute alike.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod adapter;
mod atspi;
mod element;
mod grounding;
mod toplevel;

pub use adapter::{FOCUS_LIMITS, LinuxDesktop, LinuxDesktopSettings};
pub use atspi::{
    Accessible, Atspi, CALL_TIMEOUT, Interfaces, ObjectRef, Role, State, StateSet, Walk, WalkNode,
    accessibility_bus, toolkit_accessibility,
};
pub use eileen_dbus_linux::DbusError;
pub use toplevel::{KnownWindow, window_id};

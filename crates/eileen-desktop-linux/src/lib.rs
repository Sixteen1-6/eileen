//! The desktop process's platform implementations on Linux, as `eileen-desktop-win`'s on Windows
//! (BUILD.md sections 2.6 and 3.4; docs/LINUX.md sections 3 and 4): `Grounding` and `Automation`.
//!
//! # Owns
//!
//! - [`LinuxDesktop`], with [`LinuxDesktopSettings`], on the bridge: `Grounding` (windows, whose
//!   `hwnd` is [`window_id`], the focus, its selection) and `Automation` (trees, finds, actions,
//!   writes, through handles that pack an element's object); the rest `Unavailable`.
//! - [`Atspi`], Eileen's own AT-SPI2 client on zbus: apps, their processes, each object's name,
//!   role, states, interfaces ([`Accessible`]), children, text, selection, box, automation id,
//!   actions, number and a bounded walk ([`Walk`]); [`accessibility_bus`], [`CALL_TIMEOUT`].
//! - Tables of AT-SPI's numbers and names: [`Role`], [`State`], [`StateSet`], [`Interfaces`].
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0). `TextInsertion`, `Input`, `Capture`, `SystemVolume`
//!   and `Grounding::subscribe`, on AT-SPI's events, come next.
//! - D-Bus plumbing ([`DbusError`], re-exported, the buses, the bridge): `eileen-dbus-linux`'s.
//! - The desktop server's tools, safety rules and selector identity (`eileen-desktop`, WP5).
//!
//! # Invariants
//!
//! - Linux only: `cfg(target_os = "linux")`, an empty library elsewhere.
//! - A password field is never read or written: [`Atspi::text`] and [`Atspi::selection`] refuse
//!   its role before any `Text` call, as Windows refuses `IsPassword`, and `set_value` refuses it
//!   and a read-only field before any write (BUILD.md 2.6; the tests whose names hold `password`).
//! - It writes only what `Automation` is asked to, to an element of its window's app, and never
//!   `IsEnabled` (2.6; test `toolkit_accessibility_is_read_and_never_written`).
//! - Every app text is sanitized and bounded, and no error carries it or an error name an app
//!   chose. Roles, states and interfaces mean what this crate's tables say; a class is a role name.
//! - A window's pid is the one the bus's daemon gives for its app's connection. Its process name
//!   is kept only when clean, and marks it Eileen's only when read from the executable (test
//!   `a_window_names_its_apps_own_process`).
//! - Every call is bounded: on the accessibility bus by its connection's call timeout, a walk by
//!   its `TreeLimits` and its budget, checked between reads, and a trait call by its deadline on
//!   the bridge, with no timer of its own around a call (`eileen-dbus-linux`'s deadline model).
//! - Unix sockets only (`bus_at_within`): no launcher's answer runs a program or leaves the PC.
//!
//! # Boundaries
//!
//! - Called by the desktop composition root. May depend on `eileen-platform`, `eileen-contracts`,
//!   `eileen-dbus-linux`, `zbus`, `tokio`, `serde_json` (3.1 adds `eileen-stdx`, `eileen-trace`).
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-desktop-linux`.
//! - Layers: unit tests of the tables, the window id, process names, handles (by property) and
//!   `find`'s scores; integration tests in `tests/it/` of the client, `Grounding` and `Automation`
//!   through `Bridge::on_private_bus`, on fakes on two private buses, one app in a process of its
//!   own. Walk tests run in paused time; in real time, 100 ms deadlines and call timeouts, and one
//!   250 ms call timeout under a 300 ms deadline, each kept by the code under test.
//! - A bug starts as a list or tree that misses what is on screen, a slow call or a forbidden read.
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
//! - A process name is a hint: `comm`, the fallback, is the process's own choice, and a Flatpak
//!   app reaches the bus through an `xdg-dbus-proxy` of its own, whose pid and name it gets.
//! - Wayland tells a GTK window nothing of being minimized: only a toolkit's `ICONIFIED` says so.
//! - Only Firefox's and Chromium's English names mark a private window, and a class is `frame` or
//!   `dialog`: BUILD.md 2.6's and 2.10's private-window rules and 2.6's terminal tier need more.
//! - A second at-spi-bus-launcher on the session's runtime folder and display takes the live bus.
//! - nextest runs each test whose path holds `::desktop::` alone: no module here is so named.
//!
//! # Logs and tracing
//!
//! - None yet: nothing here logs. A span should first mark an app left out.
//!
//! # Do not change without a decision record
//!
//! - [`CALL_TIMEOUT`], the walk's bounds and [`FOCUS_LIMITS`] (BUILD.md section 2.1, rule 2), the
//!   password rule, and [`window_id`]'s hash, which every process must compute alike.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod adapter;
mod atspi;
mod automation;
mod element;
mod find;
mod grounding;
mod handle;
mod toplevel;

pub use adapter::{FOCUS_LIMITS, LinuxDesktop, LinuxDesktopSettings};
pub use atspi::{
    Accessible, Atspi, CALL_TIMEOUT, Interfaces, ObjectRef, Role, State, StateSet, Walk, WalkNode,
    accessibility_bus, toolkit_accessibility,
};
pub use eileen_dbus_linux::DbusError;
pub use toplevel::{KnownWindow, window_id};

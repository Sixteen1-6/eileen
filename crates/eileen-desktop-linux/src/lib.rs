//! The desktop process's platform implementations on Linux, as `eileen-desktop-win`'s on Windows
//! (BUILD.md sections 2.6 and 3.4; docs/LINUX.md sections 3 and 4). So far it holds the AT-SPI2
//! client its desktop traits will read the desktop through.
//!
//! # Owns
//!
//! - [`Atspi`], Eileen's own client of the accessibility bus on zbus (the `atspi` crate is not
//!   pinned): the registry's applications, each object's name, role, states and interfaces
//!   ([`Accessible`]), its children and text, and a bounded walk of its tree ([`Walk`]).
//! - [`accessibility_bus`], from `org.a11y.Bus`, with [`CALL_TIMEOUT`], and
//!   [`toolkit_accessibility`], which reads `org.a11y.Status`'s `IsEnabled`.
//! - The tables that give AT-SPI's numbers and names a meaning: [`Role`], [`State`],
//!   [`StateSet`] and [`Interfaces`].
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0) and, so far, their implementations: `Grounding`,
//!   `Automation`, `TextInsertion`, `Input`, `Capture` and `SystemVolume` come next.
//! - D-Bus plumbing ([`DbusError`], re-exported, the buses, the bridge): `eileen-dbus-linux`'s.
//! - The desktop server's tools and safety rules (`eileen-desktop`, WP5).
//!
//! # Invariants
//!
//! - Linux only: `cfg(target_os = "linux")`, an empty library elsewhere.
//! - It never reads a password field's text: [`Atspi::text`] reads the role first and refuses
//!   `password text` before any `Text` call, as Windows refuses `IsPassword` (BUILD.md 2.6;
//!   test `password_text_is_refused_without_asking_for_it`).
//! - It writes nothing: not `IsEnabled`, which would turn on every Qt, Chromium and Firefox
//!   tree (2.6 keeps Chromium out of accessibility mode too), and no app's state. Its proxies
//!   declare no setter (test `toolkit_accessibility_is_read_and_never_written`).
//! - Every app text is sanitized (`eileen_contracts::desktop::sanitize`) and bounded, and errors
//!   never carry it. A role, a state or an interface means only what this crate's tables say.
//! - Every call is bounded by its connection's call timeout, and a walk by its `TreeLimits` and
//!   its budget, checked between reads: `eileen-dbus-linux`'s deadline model, with no timer of
//!   its own around a call (tests `each_call_is_bounded_by_the_call_timeout` and the walk's).
//! - Unix sockets only: an accessibility bus address that is not `unix:path=` or
//!   `unix:abstract=` is refused before any connect, so the launcher's answer never makes zbus
//!   start a program or reach the network (test `an_address_that_is_not_a_unix_socket_is_refused`).
//!
//! # Boundaries
//!
//! - Called by the desktop process's composition root on Linux, once the traits are served here.
//! - May depend on `eileen-platform` (`TreeLimits`), `eileen-contracts` (`sanitize`,
//!   `Untrusted`), `eileen-dbus-linux`, `zbus` and `tokio` (`join!`, `Instant`); BUILD.md 3.1
//!   allows `eileen-stdx` and `eileen-trace` too. Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-desktop-linux`.
//! - Layers: unit tests of the state words and the address check; integration tests in
//!   `tests/it/` against a fake launcher on one private bus and a fake registry and apps on a
//!   second, each object a `#[zbus::interface]` that counts its calls. No sleeps and no paused
//!   time: the one wait in real time is a 100 ms call timeout's own.
//! - A bug here starts as a tree that misses what is on screen, a walk that runs long, or a
//!   read of a field Eileen must not read.
//!
//! # Traps
//!
//! - Qt, Chromium and Electron, and Firefox put their trees on the bus only while `IsEnabled` is
//!   on; GTK apps always do. A listing without them, on a desktop that runs them, means it is off.
//! - `ChildCount` can be wrong, and `GetChildren` on a big table makes the app build every
//!   child: an object that manages its descendants, or has more children than there is room
//!   for, is read child by child by index, and only as far as the room goes.
//! - A second at-spi-bus-launcher started with the session's `XDG_RUNTIME_DIR` and `DISPLAY`
//!   takes over the live bus's socket path and display property, which cuts the desktop's apps
//!   off from it: a test that runs a real launcher gives it a folder and a display of its own.
//! - nextest runs every test whose path holds `::desktop::` one at a time (`.config/nextest.toml`,
//!   for the Windows desktop's tests): no module here is named `desktop`.
//!
//! # Logs and tracing
//!
//! - None yet: spans join with the trait implementations, named from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - [`CALL_TIMEOUT`] and the walk's bounds (BUILD.md section 2.1, rule 2), and the password rule.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod atspi;

pub use atspi::{
    Accessible, Atspi, CALL_TIMEOUT, Interfaces, ObjectRef, Role, State, StateSet, Walk, WalkNode,
    accessibility_bus, toolkit_accessibility,
};
pub use eileen_dbus_linux::DbusError;

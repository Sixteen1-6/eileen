//! The D-Bus helpers every Linux crate shares (docs/LINUX.md section 4): what a failed call
//! means, the buses with a method timeout, who owns a name, the portal request client and the
//! signal-stream conventions.
//! `eileen-shell-linux` and `eileen-apps-linux` use them and never re-implement them.
//!
//! # Owns
//!
//! - [`DbusError`] and its mappings from zbus errors and D-Bus error names.
//! - [`session_bus`] and [`system_bus`], with [`METHOD_TIMEOUT`], and their `_within` forms.
//! - [`name_owner`] and [`owner_changes`]: `GetNameOwner` and `NameOwnerChanged`.
//! - [`Request`]: an xdg-desktop-portal request, from its token to its `Response`.
//! - The signal-stream conventions: [`QUEUE_SIZE`], [`DRAIN_CAP`], [`next`], [`Stream`].
//! - `testing`, behind the `private-bus` feature: the private bus and fake services for tests.
//!
//! # Does not own
//!
//! - Any one service's client (notifications, logind, `UPower`, MPRIS, the portals): the crates
//!   that serve the platform traits own those.
//! - `PlatformError` (`eileen-platform`, WP0): [`DbusError`] mirrors it until the traits land.
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - One deadline model. Every method call is bounded by its connection's method timeout,
//!   which [`DbusError::from_zbus`] maps to [`DbusError::DeadlineExceeded`]. Only a wait that a
//!   signal answers takes an explicit `Duration`, enforced with `tokio::time::timeout`.
//! - Errors keep only error names and kinds, never a service's message text, a path or a user
//!   name.
//! - It never starts a process, except `dbus-daemon` for a test through `testing`.
//!
//! # Boundaries
//!
//! - May depend on `zbus` (tokio only, no async-io), `tokio` (the timer of a signal-answered
//!   wait) and `thiserror`. The Linux crates depend on it; it depends on no workspace crate.
//! - Product code never turns on `private-bus`: only dev-dependencies do.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-dbus-linux`.
//! - Layers: unit tests for every error mapping and owner-change parse; integration tests in
//!   `tests/it/` against fake services on a `testing::PrivateBus`. No test asserts that
//!   nothing arrived within a wall-clock window: `testing::round_trip` and
//!   `testing::nothing_ready` make that deterministic. Deadlines use tokio's paused time.
//!
//! # Traps
//!
//! - Every signal stream, the ones zbus's proxies make included, has the same traps: one
//!   that is not polled stalls its whole connection, and one dropped outside the tokio runtime's
//!   context panics. They are written once, in the doc of `src/stream.rs`.
//! - The bus delivers a signal addressed to a connection whatever its match rules say: check
//!   the sender of every signal that must come from a service.
//!
//! # Logs and tracing
//!
//! - None: the helpers are thin. Spans join with the trait implementations.
//!
//! # Do not change without a decision record
//!
//! - The deadline model and [`METHOD_TIMEOUT`] (BUILD.md section 2.1, rule 2).

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod bus;
mod error;
mod names;
mod portal;
mod stream;
#[cfg(feature = "private-bus")]
pub mod testing;

pub use bus::{METHOD_TIMEOUT, session_bus, session_bus_within, system_bus, system_bus_within};
pub use error::DbusError;
pub use names::{Names, OwnerChange, OwnerChanges, name_owner, owner_changes};
pub use portal::{CLOSE_GRACE, Request};
pub use stream::{DRAIN_CAP, QUEUE_SIZE, Stream, next};

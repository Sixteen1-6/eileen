//! The D-Bus helpers every Linux crate shares (docs/LINUX.md section 4): what a failed call
//! means and the buses with a method timeout. `eileen-shell-linux` and `eileen-apps-linux` use
//! them and never re-implement them.
//!
//! # Owns
//!
//! - [`DbusError`] and its mappings from zbus errors and D-Bus error names.
//! - [`session_bus`] and [`system_bus`], with [`METHOD_TIMEOUT`], and their `_within` forms.
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
//! - May depend on `zbus` (tokio only, no async-io) and `thiserror`. The Linux crates depend on
//!   it; it depends on no workspace crate.
//! - Product code never turns on `private-bus`: only dev-dependencies do.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-dbus-linux`.
//! - Layers: unit tests for every error mapping; integration tests in `tests/it/` against fake
//!   services on a `testing::PrivateBus`. Deadlines use tokio's paused time.
//!
//! # Traps
//!
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
#[cfg(feature = "private-bus")]
pub mod testing;

pub use bus::{METHOD_TIMEOUT, session_bus, session_bus_within, system_bus, system_bus_within};
pub use error::DbusError;

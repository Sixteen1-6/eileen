//! The D-Bus helpers every Linux crate shares (docs/LINUX.md section 4): what a failed call
//! means, the buses with a method timeout, who owns a name, the portal request client, the
//! signal streams' helpers and conventions, and the bridge from the synchronous platform traits
//! to the async clients. The other Linux crates use them and never re-implement them.
//!
//! # Owns
//!
//! - [`DbusError`], its mappings from zbus errors and D-Bus error names, and its `From` into
//!   `PlatformError`, variant for variant.
//! - [`session_bus`] and [`system_bus`], with [`METHOD_TIMEOUT`], and their `_within` forms;
//!   [`bus_at_within`] for a bus at a Unix socket's address, such as the accessibility bus.
//! - [`name_owner`] and [`owner_changes`]: `GetNameOwner` and `NameOwnerChanged`.
//! - [`Request`]: an xdg-desktop-portal request, from its token to its `Response`, and
//!   [`close_request`], which ends one and waits for the portal's answer.
//! - The signal streams: [`subscribe`] and its [`Signals`], and [`drain`], which keep the
//!   conventions of `src/stream.rs` ([`QUEUE_SIZE`], [`DRAIN_CAP`], [`next`], [`Stream`]).
//! - [`Bridge`]: the one thread where trait calls run their async jobs on [`Buses`] (the shared
//!   buses, or a session connection of the job's own), within a `Deadline` ([`Bridge::call`])
//!   or behind a `Subscription` ([`Bridge::watch`]); [`DEFAULT_DEADLINE`] for a call with none.
//! - `testing`, behind the `private-bus` feature: the private bus and fake services for tests.
//!
//! # Does not own
//!
//! - Any one service's client (notifications, logind, `UPower`, MPRIS, the portals), nor the
//!   trait implementations: the crates that serve the platform traits own those, and map their
//!   own error types to `PlatformError` there.
//! - `PlatformError`, `Deadline` and `Subscription` (`eileen-platform`, WP0).
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - One deadline model. Every method call is bounded by its connection's method timeout,
//!   which [`DbusError::from_zbus`] maps to [`DbusError::DeadlineExceeded`]. Only a wait that a
//!   signal answers takes an explicit `Duration`, enforced with `tokio::time::timeout`. A trait
//!   call through the bridge waits at most its `Deadline`, past which its job is cancelled.
//! - Errors keep only error names and kinds: never a service's message, a path or a user name.
//! - Constructors and calls take `&zbus::Connection` and clone it to keep, here and in the
//!   crates that use these helpers, so the caller (or a test, with a private bus) picks the bus.
//!   The one exception takes one by value because dropping it ends a hold: the session connection
//!   of `LinuxPowerRequest::keep_awake`, whose close ends the desktop's hold.
//! - Bridge jobs answer plain data, never a zbus connection, proxy or stream (all are `Send`),
//!   so those stay on the bridge's thread: a rule each job keeps, which no type checks.
//! - It never starts a process, except `dbus-daemon` for a test through `testing`, not even for
//!   an address: [`bus_at_within`] follows a Unix socket's only and refuses `unixexec:`.
//!
//! # Boundaries
//!
//! - May depend on `eileen-platform` (the error, `Deadline`, `Subscription`), `eileen-stdx`
//!   (the named thread), `zbus` (tokio only, no async-io), `tokio` (the bridge's runtime, the
//!   timer of a signal-answered wait) and `thiserror`. The Linux crates depend on it.
//! - Product code never turns on `private-bus`: only dev-dependencies do.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-dbus-linux`.
//! - Layers: unit tests for every error mapping and owner-change parse; integration tests in
//!   `tests/it/` against fake services on a `testing::PrivateBus`, the bridge's through
//!   `Bridge::on_private_bus`. No test asserts that nothing arrived within a wall-clock window:
//!   `testing::round_trip` and `testing::nothing_ready` make that deterministic. Deadlines use
//!   tokio's paused time, except the bridge's own, which its tests pass with a short budget.
//!
//! # Traps
//!
//! - Every signal stream, the ones zbus's proxies make included, has the same traps: one
//!   that is not polled stalls its whole connection, and one dropped outside the tokio runtime's
//!   context panics. They are written once, in the doc of `src/stream.rs`.
//! - The bus delivers a signal addressed to a connection whatever its match rules say: check
//!   the sender of every signal that must come from a service.
//! - A bridge job and a watch's sink run on the bridge's one thread: one that blocks it stalls
//!   every call until its deadline. A `Bridge::call` or `watch` made there is refused at once.
//!
//! # Logs and tracing
//!
//! - None: the helpers are thin. Spans join with the trait implementations. A bridge thread
//!   that panicked fails a soft assertion (`eileen_stdx::never!`) when it is joined.
//!
//! # Do not change without a decision record
//!
//! - The deadline model and [`METHOD_TIMEOUT`] (BUILD.md section 2.1, rule 2).

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod bridge;
mod bus;
mod error;
mod names;
mod portal;
mod stream;
#[cfg(feature = "private-bus")]
pub mod testing;

pub use bridge::{Bridge, Buses, DEFAULT_DEADLINE};
pub use bus::{
    METHOD_TIMEOUT, bus_at_within, session_bus, session_bus_within, system_bus, system_bus_within,
};
pub use error::DbusError;
pub use names::{Names, OwnerChange, OwnerChanges, name_owner, owner_changes};
pub use portal::{CLOSE_GRACE, Request, close_request};
pub use stream::{DRAIN_CAP, QUEUE_SIZE, Signals, Stream, drain, next, subscribe};

//! Connections to the user's session bus and the system bus, each with a method timeout.
//!
//! Every method call on these connections is bounded by the connection's method timeout, which
//! zbus enforces with a tokio timer and reports as an I/O error of kind `TimedOut`;
//! [`DbusError::from_zbus`] makes that [`DbusError::DeadlineExceeded`]. That is the one deadline
//! model for method calls: no client adds a timer of its own around a call. Only a wait that a
//! signal answers, such as a portal's `Response`, takes an explicit `Duration`.
//!
//! The default, [`METHOD_TIMEOUT`], is 5 s, the deadline BUILD.md section 2.1 (rule 2: "Every
//! call to another process has a deadline") gives a UI Automation call. A desktop service is the
//! same kind of callee: another process that can hang. zbus itself sets no method timeout, and
//! the bus need not end a call that is never answered, so a connection made without one lets a
//! hung service hold a call for good.

use std::time::Duration;

use crate::DbusError;

/// The method timeout [`session_bus`] and [`system_bus`] give their connections: 5 s, as
/// BUILD.md section 2.1 gives a UI Automation call by default.
pub const METHOD_TIMEOUT: Duration = Duration::from_secs(5);

/// What errors name the session bus.
const SESSION: &str = "the session bus";
/// What errors name the system bus.
const SYSTEM: &str = "the system bus";

/// The user's session bus, with the default method timeout, [`METHOD_TIMEOUT`].
pub async fn session_bus() -> Result<zbus::Connection, DbusError> {
    session_bus_within(METHOD_TIMEOUT).await
}

/// The system bus, with the default method timeout, [`METHOD_TIMEOUT`].
pub async fn system_bus() -> Result<zbus::Connection, DbusError> {
    system_bus_within(METHOD_TIMEOUT).await
}

/// The user's session bus, each method call bounded by `timeout`.
pub async fn session_bus_within(timeout: Duration) -> Result<zbus::Connection, DbusError> {
    let builder = zbus::connection::Builder::session()
        .map_err(|error| DbusError::from_zbus(SESSION, &error))?;
    build_within(builder, SESSION, timeout).await
}

/// The system bus, each method call bounded by `timeout`.
pub async fn system_bus_within(timeout: Duration) -> Result<zbus::Connection, DbusError> {
    let builder = zbus::connection::Builder::system()
        .map_err(|error| DbusError::from_zbus(SYSTEM, &error))?;
    build_within(builder, SYSTEM, timeout).await
}

/// The connection `builder` makes, each method call bounded by `timeout`; errors name `what`.
/// The private test bus builds its connections here too, so the tests run this path.
pub(crate) async fn build_within(
    builder: zbus::connection::Builder<'_>,
    what: &'static str,
    timeout: Duration,
) -> Result<zbus::Connection, DbusError> {
    builder
        .method_timeout(timeout)
        .build()
        .await
        .map_err(|error| DbusError::from_zbus(what, &error))
}

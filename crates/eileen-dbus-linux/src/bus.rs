//! Connections to the user's session bus, the system bus and a bus at a Unix socket's address,
//! each with a method timeout.
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

use zbus::address::transport::{Transport, Unix, UnixSocket};

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

/// The bus at `address`, a D-Bus server address such as `unix:path=/run/user/1000/at-spi/bus`,
/// each method call bounded by `timeout`; errors name `what`. It is for a bus that is neither
/// the session's nor the system's, such as the accessibility bus, whose address another
/// service hands out.
///
/// - Only a Unix socket's address, `unix:path=` or `unix:abstract=`, is followed. Any other is
///   [`DbusError::Invalid`] before any connect: zbus would start a program for some
///   (`unixexec:`, or `ibus:`, which runs `ibus address`) and reach the network for others
///   (`tcp:`, `nonce-tcp:`), and this crate never starts a process.
/// - A socket where no bus listens is [`DbusError::Unavailable`].
pub async fn bus_at_within(
    address: &str,
    what: &'static str,
    timeout: Duration,
) -> Result<zbus::Connection, DbusError> {
    let Some(address) = unix_address(address) else {
        return Err(DbusError::Invalid { what, detail: "not a unix socket's address".to_owned() });
    };
    let builder = zbus::connection::Builder::address(address)
        .map_err(|error| DbusError::from_zbus(what, &error))?;
    build_within(builder, what, timeout).await
}

/// `address`, parsed, when it is a Unix socket's, at a path or an abstract name: the one kind of
/// address zbus connects to without starting a program or leaving the machine.
fn unix_address(address: &str) -> Option<zbus::Address> {
    let address = zbus::Address::try_from(address).ok()?;
    let socket = |unix: &Unix| matches!(unix.path(), UnixSocket::File(_) | UnixSocket::Abstract(_));
    matches!(address.transport(), Transport::Unix(unix) if socket(unix)).then_some(address)
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

#[cfg(test)]
mod tests {
    use super::unix_address;

    #[test]
    fn only_a_unix_socket_is_an_address_to_follow() {
        let guid = "0123456789abcdef0123456789abcdef";
        for address in [
            format!("unix:path=/run/user/1000/at-spi/bus_0,guid={guid}"),
            "unix:path=/run/user/1000/at-spi/bus".to_owned(),
            format!("unix:abstract=/tmp/dbus-AbCdEf0123,guid={guid}"),
        ] {
            assert!(unix_address(&address).is_some(), "{address} is a unix socket's");
        }
        for address in [
            "tcp:host=127.0.0.1,port=4444",
            "nonce-tcp:host=127.0.0.1,port=4444,noncefile=/tmp/nonce",
            "unixexec:path=/bin/true",
            "ibus:",
            "unix:dir=/tmp",
            "unix:tmpdir=/tmp",
            "unix:path=/tmp/a,abstract=/tmp/b",
            "unix:",
            "",
            "not an address",
        ] {
            assert!(unix_address(address).is_none(), "{address:?} is not");
        }
    }
}

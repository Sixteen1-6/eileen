//! The accessibility bus, which at-spi-bus-launcher runs beside the session bus and names there
//! as `org.a11y.Bus`, and the session's switch for toolkit accessibility, `org.a11y.Status`.
//!
//! The launcher is a process on the session bus like any other, so its answer is checked before
//! it is followed: only a Unix socket, at a path or an abstract name, is an accessibility bus.
//! zbus would also follow an address that starts a program (`unixexec:`, or `ibus:`, which runs
//! `ibus address`) or reaches the network (`tcp:`).

use std::time::Duration;

use eileen_dbus_linux::DbusError;
use zbus::Connection;
use zbus::address::transport::{Transport, Unix, UnixSocket};
use zbus::proxy::CacheProperties;

use super::Atspi;
use super::proxies::{BusProxy, StatusProxy};

/// What errors of the bus name.
const BUS: &str = "accessibility bus";
/// What errors of the switch name.
const STATUS: &str = "accessibility status";

/// The client of the accessibility bus whose address `session`'s `org.a11y.Bus` gives, each
/// call bounded by `call_timeout` ([`super::CALL_TIMEOUT`] in the product).
///
/// - No `org.a11y.Bus` on the session bus, or an empty address (the launcher's answer when its
///   bus did not start), is [`DbusError::Unavailable`].
/// - An address that is not `unix:path=` or `unix:abstract=` is [`DbusError::Invalid`], before
///   any connect.
pub async fn accessibility_bus(
    session: &Connection,
    call_timeout: Duration,
) -> Result<Atspi, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(BUS, &error);
    let launcher = BusProxy::builder(session)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(failed)?;
    let address = launcher.get_address().await.map_err(failed)?;
    if address.is_empty() {
        return Err(DbusError::Unavailable { what: BUS });
    }
    if !unix_socket(&address) {
        let detail = "not a unix socket's address".to_owned();
        return Err(DbusError::Invalid { what: BUS, detail });
    }
    let conn = eileen_dbus_linux::bus_at_within(&address, BUS, call_timeout).await?;
    Ok(Atspi::new(&conn))
}

/// Whether toolkits are asked to put their trees on the accessibility bus: `IsEnabled` of
/// `session`'s `org.a11y.Status`, read, never written. Qt, Chromium and Electron, and Firefox
/// serve their trees only while it is on; turning it on would switch every one of them into
/// accessibility mode, which only the user decides.
pub async fn toolkit_accessibility(session: &Connection) -> Result<bool, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(STATUS, &error);
    let status = StatusProxy::builder(session)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(failed)?;
    status.is_enabled().await.map_err(failed)
}

/// Whether `address` is a Unix socket's, at a path or an abstract name, as zbus reads it.
fn unix_socket(address: &str) -> bool {
    let socket = |unix: &Unix| matches!(unix.path(), UnixSocket::File(_) | UnixSocket::Abstract(_));
    zbus::Address::try_from(address)
        .is_ok_and(|address| matches!(address.transport(), Transport::Unix(unix) if socket(unix)))
}

#[cfg(test)]
mod tests {
    use super::unix_socket;

    #[test]
    fn only_a_unix_socket_is_an_accessibility_bus_address() {
        let guid = "0123456789abcdef0123456789abcdef";
        for address in [
            format!("unix:path=/run/user/1000/at-spi/bus_0,guid={guid}"),
            "unix:path=/run/user/1000/at-spi/bus".to_owned(),
            format!("unix:abstract=/tmp/dbus-AbCdEf0123,guid={guid}"),
        ] {
            assert!(unix_socket(&address), "{address} is a unix socket");
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
            assert!(!unix_socket(address), "{address:?} is not");
        }
    }
}

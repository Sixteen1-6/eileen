//! The accessibility bus, which at-spi-bus-launcher runs beside the session bus and names there
//! as `org.a11y.Bus`, and the session's switch for toolkit accessibility, `org.a11y.Status`.
//!
//! The launcher is a process on the session bus like any other, so its answer is checked before
//! it is followed: `eileen_dbus_linux::bus_at_within` follows only a Unix socket's address, at a
//! path or an abstract name, where zbus would also follow one that starts a program (`unixexec:`,
//! or `ibus:`, which runs `ibus address`) or reaches the network (`tcp:`).

use std::time::Duration;

use eileen_dbus_linux::DbusError;
use zbus::Connection;
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
///   any connect: `eileen_dbus_linux::bus_at_within` refuses it.
/// - A socket where no bus listens, as a launcher whose bus has gone gives, is
///   [`DbusError::Unavailable`].
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
    let conn = eileen_dbus_linux::bus_at_within(&address, BUS, call_timeout).await?;
    Ok(Atspi::new(&conn))
}

/// Whether toolkits are asked to put their trees on the accessibility bus: `IsEnabled` of
/// `session`'s `org.a11y.Status`, read, never written. Qt, Chromium and Electron, and Firefox
/// serve their trees only while it is on or a screen reader runs; turning it on would switch
/// every one of them into accessibility mode, which only the user decides. No `org.a11y.Status`
/// on the session bus, as without the launcher, is [`DbusError::Unavailable`].
pub async fn toolkit_accessibility(session: &Connection) -> Result<bool, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(STATUS, &error);
    let status = StatusProxy::builder(session)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .map_err(failed)?;
    status.is_enabled().await.map_err(failed)
}

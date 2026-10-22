//! Battery saver from the power-profiles daemon: its `ActiveProfile`, and whether a program
//! holds `power-saver`, under the current name and else the legacy one. The parent module's doc
//! gives the sources and the rules.

use std::collections::HashMap;

use zbus::Connection;
use zbus::zvariant::OwnedValue;

use super::{Callee, PowerDaemon};
use crate::DbusError;

/// The profile that is battery saver.
const POWER_SAVER: &str = "power-saver";

/// What a profiles daemon answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProfilesAnswer {
    /// The daemon that answered.
    pub(super) daemon: PowerDaemon,
    /// `ActiveProfile` is `power-saver`.
    pub(super) saver: bool,
    /// A hold in `ActiveProfileHolds` is for `power-saver`.
    pub(super) saver_held: bool,
}

/// The profiles daemon's facts, under the current name, else the legacy one.
pub(super) async fn read_profiles(bus: &Connection) -> Result<ProfilesAnswer, DbusError> {
    match read_profiles_of(bus, PowerDaemon::PowerProfiles).await {
        Err(current @ (DbusError::Unavailable { .. } | DbusError::NotFound { .. })) => {
            match read_profiles_of(bus, PowerDaemon::LegacyPowerProfiles).await {
                // Neither name has a daemon, or only the current one has, without the object.
                Err(DbusError::Unavailable { .. }) => Err(current),
                legacy => legacy,
            }
        }
        answer => answer,
    }
}

/// `ActiveProfile` and `ActiveProfileHolds` of the profiles daemon under `daemon`'s name.
async fn read_profiles_of(
    bus: &Connection,
    daemon: PowerDaemon,
) -> Result<ProfilesAnswer, DbusError> {
    let profiles = Callee { bus, daemon };
    let (path, interface) = (daemon.path(), daemon.name());
    let active: String = profiles.property(path, interface, "ActiveProfile").await?;
    // The holds only say who chose the profile; a daemon without a readable list (one older
    // than holds, or a malformed one) leaves battery saver to `ActiveProfile`.
    let holds: Vec<HashMap<String, OwnedValue>> =
        profiles.property(path, interface, "ActiveProfileHolds").await.unwrap_or_default();
    let saver_held = holds.iter().any(|hold| {
        hold.get("Profile").and_then(|profile| profile.downcast_ref::<&str>().ok())
            == Some(POWER_SAVER)
    });
    Ok(ProfilesAnswer { daemon, saver: active == POWER_SAVER, saver_held })
}

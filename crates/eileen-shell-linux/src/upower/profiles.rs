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
        Err(current) if tries_legacy(&current) => {
            match read_profiles_of(bus, PowerDaemon::LegacyPowerProfiles).await {
                // Neither name has a daemon, or only the current one has, without the object.
                Err(DbusError::Unavailable { .. }) => Err(current),
                legacy => legacy,
            }
        }
        answer => answer,
    }
}

/// Whether a reading that failed under the current name with `error` tries the legacy name
/// next: only when the current name has no daemon, one the bus cannot start included, or a
/// daemon without the profiles object. A daemon that refused, sent a reply Eileen cannot use or
/// did not answer in time is not asked twice, and a failed bus is not asked again.
const fn tries_legacy(error: &DbusError) -> bool {
    match error {
        DbusError::Unavailable { .. } | DbusError::NotFound { .. } => true,
        DbusError::DeadlineExceeded { .. }
        | DbusError::Refused { .. }
        | DbusError::Invalid { .. }
        | DbusError::Io { .. } => false,
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

#[cfg(test)]
mod tests {
    use super::{DbusError, tries_legacy};

    #[test]
    fn the_legacy_name_is_tried_only_when_the_current_one_has_no_daemon_or_object() {
        let what = "the power-profiles daemon";
        let cases = [
            (DbusError::Unavailable { what }, true, "no daemon, or one the bus cannot start"),
            (DbusError::NotFound { what }, true, "a daemon without the profiles object"),
            (DbusError::DeadlineExceeded { call: what }, false, "a hung daemon is not asked twice"),
            (DbusError::Refused { what, reason: "not authorized" }, false, "a refusal stands"),
            (
                DbusError::Invalid { what, detail: "ActiveProfile has another type".to_owned() },
                false,
                "a reply Eileen cannot use stands",
            ),
            (DbusError::Io { what, detail: "broken pipe".to_owned() }, false, "a failed bus"),
        ];
        for (error, legacy, case) in cases {
            assert_eq!(tries_legacy(&error), legacy, "{case}");
        }
    }
}

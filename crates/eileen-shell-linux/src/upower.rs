//! The power facts over D-Bus: whether the PC has a system battery and runs on it, from `UPower`,
//! and whether battery saver is on, from the power-profiles daemon (docs/LINUX.md sections 4
//! and 7, phase L1). It is the D-Bus counterpart of the file reads in the `power` module and,
//! unlike the daemon's state file, it sees a program's profile hold.
//!
//! [`LinuxPower::read`] serves the power state of `MachineProbe::facts` (the `power` field, with
//! its `degraded` entries) and of `MachineProbe::load`, once `eileen-platform` has the trait
//! (BUILD.md section 3.4; [`PowerState`] mirrors its contract type until then).
//! [`LinuxPower::changes`] serves `MachineProbe::watch`'s `MachineChange::PowerSource` and
//! `MachineChange::BatterySaver`; module `changes` gives the stream's own rules.
//!
//! # Sources
//!
//! Both daemons are on the system bus, as their interfaces describe them:
//! [`UPower`'s](https://upower.freedesktop.org/docs/) and
//! [power-profiles-daemon's](https://upower.pages.freedesktop.org/power-profiles-daemon/gdbus-org.freedesktop.UPower.PowerProfiles.html).
//!
//! - `on_battery` is `OnBattery` of `org.freedesktop.UPower` at `/org/freedesktop/UPower`.
//! - `has_battery` is a present system battery among the devices `EnumerateDevices` lists: an
//!   `org.freedesktop.UPower.Device` whose `Type` is 2 (battery), whose `PowerSupply` is true and
//!   whose `IsPresent` is true. `PowerSupply` is true only for the system's own supplies, so a
//!   mouse's or a headset's battery is not the PC's.
//! - `battery_saver` is `ActiveProfile` being `power-saver`, read from
//!   `org.freedesktop.UPower.PowerProfiles` at `/org/freedesktop/UPower/PowerProfiles`
//!   (power-profiles-daemon 0.20 and later, and tuned-ppd), else from the legacy
//!   `net.hadess.PowerProfiles` at `/net/hadess/PowerProfiles`. A program's hold (`HoldProfile`,
//!   listed in `ActiveProfileHolds`) sets the active profile too, as GNOME's low-battery power
//!   saver does; [`PowerReading::saver_held`] says when a program holds `power-saver`.
//!
//! # Rules
//!
//! - A daemon that does not answer is named in `degraded`, in the words of the probe's entries,
//!   and its facts are left at their unknown value, `false`: they are never passed off as read.
//! - `OnBattery` also covers a UPS that is discharging, so a desktop on a failing mains supply
//!   reports `on_battery` without `has_battery`: it is not on mains power, which is what the plan
//!   waits for.
//! - A device that is gone since the listing is skipped. Any other unusable answer about a
//!   device degrades all of `UPower`'s facts rather than leave a battery out.
//! - Each property is read alone with `Get`, never with `GetAll`, which would carry a device's
//!   serial number. Of a hold only its profile is kept: its program's id and reason are another
//!   app's text.
//! - The legacy profiles name is read only when the current one has no daemon or no profiles
//!   object (`Unavailable` or `NotFound`), so a current daemon that fails or hangs is not asked
//!   twice. A daemon without a readable hold list leaves battery saver to `ActiveProfile`.
//! - [`LinuxPower::read`] calls each daemon by its name, without D-Bus's no-auto-start flag, so
//!   an installed daemon that is not running yet is started by the bus, as for any other client
//!   of it. When the bus cannot start it (its systemd unit masked or missing, or its program
//!   failing), [`DbusError`] maps the bus's answer to `Unavailable`: the daemon is named as not
//!   on the bus, and the legacy profiles name is tried next.
//! - The first reading of [`LinuxPower::changes`] calls them by name too; its later readings
//!   never have the bus start a daemon.
//!
//! # Traps
//!
//! - No call has a deadline of its own: give [`LinuxPower`] a connection with a method timeout,
//!   as [`eileen_dbus_linux::system_bus`] makes, or a hung daemon holds the reading. A call
//!   past it is worded "did not answer in time".
//! - [`LinuxPower::read`] lets the bus start a daemon that is not running, so calling it on a
//!   timer restarts a daemon the user stopped: between readings, take the state from
//!   [`LinuxPower::changes`].

mod changes;
mod profiles;

use eileen_dbus_linux::name_owner;
use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::DbusError;
use crate::facts::PowerState;
pub use changes::PowerChanges;
use profiles::{ProfilesAnswer, read_profiles};

/// `UPower`'s bus name, which is also its root object's interface.
const UPOWER: &str = "org.freedesktop.UPower";

/// `UPower`'s root object.
const UPOWER_PATH: &str = "/org/freedesktop/UPower";

/// The interface of each power device `UPower` lists.
const DEVICE: &str = "org.freedesktop.UPower.Device";

/// The properties interface every object has.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The profiles daemon's bus name and interface since power-profiles-daemon 0.20.
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";

/// The profiles daemon's bus name and interface before 0.20, which later versions keep.
const LEGACY_PROFILES: &str = "net.hadess.PowerProfiles";

/// `UPower`'s device `Type` for a battery.
const BATTERY: u32 = 2;

/// A daemon the power facts come from, on the system bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerDaemon {
    /// `UPower`, `org.freedesktop.UPower`: `has_battery` and `on_battery`.
    UPower,
    /// The profiles daemon under `org.freedesktop.UPower.PowerProfiles` (power-profiles-daemon
    /// 0.20 and later, and tuned-ppd): `battery_saver`.
    PowerProfiles,
    /// The profiles daemon under its legacy name, `net.hadess.PowerProfiles`: `battery_saver`.
    LegacyPowerProfiles,
}

impl PowerDaemon {
    /// Its bus name; for a profiles daemon, also its interface.
    const fn name(self) -> &'static str {
        match self {
            Self::UPower => UPOWER,
            Self::PowerProfiles => PROFILES,
            Self::LegacyPowerProfiles => LEGACY_PROFILES,
        }
    }

    /// The object its facts are read from.
    const fn path(self) -> &'static str {
        match self {
            Self::UPower => UPOWER_PATH,
            Self::PowerProfiles => "/org/freedesktop/UPower/PowerProfiles",
            Self::LegacyPowerProfiles => "/net/hadess/PowerProfiles",
        }
    }

    /// What its errors name.
    const fn what(self) -> &'static str {
        match self {
            Self::UPower => "UPower",
            Self::PowerProfiles | Self::LegacyPowerProfiles => "the power-profiles daemon",
        }
    }
}

/// The power facts over D-Bus, from the power daemons on the system bus. Each reading asks the
/// daemons again; nothing is cached between readings.
#[derive(Debug, Clone)]
pub struct LinuxPower {
    /// The system bus.
    bus: Connection,
}

impl LinuxPower {
    /// A client of the power daemons on `bus`, the system bus, which it clones.
    #[must_use]
    pub fn new(bus: &Connection) -> Self {
        Self { bus: bus.clone() }
    }

    /// Reads the power facts from `UPower` and the profiles daemon. It never fails: a daemon
    /// that does not answer is named in [`PowerReading::degraded`].
    pub async fn read(&self) -> PowerReading {
        let upower = read_upower(&self.bus, Reach::Start).await;
        let profiles = read_profiles(&self.bus, Reach::Start).await;
        reading_of(upower.as_ref(), profiles.as_ref())
    }

    /// Starts following the power facts: the stream yields each new [`PowerState`], starting
    /// from [`PowerChanges::state`]. It fails only when the bus refuses the subscriptions; a
    /// daemon that is absent now is read when it starts.
    pub async fn changes(&self) -> Result<PowerChanges, DbusError> {
        changes::follow(&self.bus).await
    }
}

/// One reading of the power facts from both daemons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerReading {
    /// The facts. One whose daemon did not answer is left at its unknown value, `false`, and
    /// named in `degraded`.
    pub state: PowerState,
    /// The daemons that answered: `UPower` for `has_battery` and `on_battery`, one profiles daemon
    /// for `battery_saver`.
    pub answered: Vec<PowerDaemon>,
    /// A program holds the `power-saver` profile (`ActiveProfileHolds`), as GNOME does when the
    /// battery runs low: battery saver is on without the user choosing it. `false` also when the
    /// daemon's hold list cannot be read, which leaves `battery_saver` as it is.
    pub saver_held: bool,
    /// Each daemon that did not answer and why, worded as
    /// [`MachineFacts::degraded`](crate::facts::MachineFacts::degraded) entries.
    pub degraded: Vec<String>,
}

/// What `UPower` answered.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UPowerAnswer {
    /// `OnBattery`.
    on_battery: bool,
    /// The system batteries, present or not, each with whether it is present.
    batteries: Vec<(OwnedObjectPath, bool)>,
}

impl UPowerAnswer {
    /// A system battery is present.
    fn has_battery(&self) -> bool {
        self.batteries.iter().any(|(_, present)| *present)
    }
}

/// The reading the two answers make.
fn reading_of(
    upower: Result<&UPowerAnswer, &DbusError>,
    profiles: Result<&ProfilesAnswer, &DbusError>,
) -> PowerReading {
    let mut answered = Vec::new();
    let mut degraded = Vec::new();
    match upower {
        Ok(_) => answered.push(PowerDaemon::UPower),
        Err(error) => degraded.push(format!(
            "power: {}; taken as no battery and mains power",
            failure(PowerDaemon::UPower, error)
        )),
    }
    match profiles {
        Ok(answer) => answered.push(answer.daemon),
        Err(error) => degraded.push(format!(
            "battery saver: {}; taken as off",
            failure(PowerDaemon::PowerProfiles, error)
        )),
    }
    PowerReading {
        state: state_of(upower.ok(), profiles.ok()),
        answered,
        saver_held: profiles.is_ok_and(|answer| answer.saver_held),
        degraded,
    }
}

/// The state the answers give; a missing answer leaves its facts `false`.
fn state_of(upower: Option<&UPowerAnswer>, profiles: Option<&ProfilesAnswer>) -> PowerState {
    PowerState {
        has_battery: upower.is_some_and(UPowerAnswer::has_battery),
        on_battery: upower.is_some_and(|answer| answer.on_battery),
        battery_saver: profiles.is_some_and(|answer| answer.saver),
    }
}

/// Why `daemon` gave no facts, in a few words that never carry the service's own text.
fn failure(daemon: PowerDaemon, error: &DbusError) -> String {
    let who = daemon.what();
    match error {
        DbusError::Unavailable { .. } => format!("{who} is not on the system bus"),
        DbusError::DeadlineExceeded { .. } => format!("{who} did not answer in time"),
        DbusError::Refused { reason, .. } => format!("{who} refused the reading ({reason})"),
        DbusError::NotFound { .. } => format!("{who} lacks the properties Eileen reads"),
        DbusError::Invalid { .. } => format!("{who} sent a reply Eileen cannot use"),
        DbusError::Io { detail, .. } => format!("the system bus failed ({detail})"),
    }
}

/// `UPower`'s facts: `OnBattery` and each system battery.
async fn read_upower(bus: &Connection, reach: Reach) -> Result<UPowerAnswer, DbusError> {
    let upower = Callee::new(bus, PowerDaemon::UPower, reach).await?;
    let on_battery = upower.property(UPOWER_PATH, UPOWER, "OnBattery").await?;
    let devices = upower.devices().await?;
    let mut batteries = Vec::new();
    for device in devices {
        match system_battery(&upower, &device).await {
            Ok(Some(present)) => batteries.push((device, present)),
            // Not a system battery, or gone since the listing.
            Ok(None) | Err(DbusError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(UPowerAnswer { on_battery, batteries })
}

/// Whether `device` is a system battery (`Type` 2 with `PowerSupply`) and, if it is, whether
/// it is present.
async fn system_battery(
    upower: &Callee<'_>,
    device: &OwnedObjectPath,
) -> Result<Option<bool>, DbusError> {
    let at = device.as_str();
    let kind: u32 = upower.property(at, DEVICE, "Type").await?;
    if kind != BATTERY {
        return Ok(None);
    }
    let supply: bool = upower.property(at, DEVICE, "PowerSupply").await?;
    if !supply {
        return Ok(None);
    }
    upower.property(at, DEVICE, "IsPresent").await.map(Some)
}

/// How a reading's calls reach a daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// By the daemon's name, so the bus starts a daemon that is installed but not running:
    /// [`LinuxPower::read`] and the first reading of [`LinuxPower::changes`].
    Start,
    /// By the unique name of its name's owner, which the bus starts nothing for, so a daemon
    /// that is not running is not read: the stream's later readings. zbus sets D-Bus's
    /// no-auto-start flag only through `Proxy::call_with_flags`, which skips the connection's
    /// method timeout: hence the unique name rather than the flag.
    Running,
}

/// One daemon as a reading calls it, every call bounded by the connection's method timeout.
struct Callee<'a> {
    /// The system bus.
    bus: &'a Connection,
    /// The daemon.
    daemon: PowerDaemon,
    /// Where its calls go: the daemon's name, or the unique name of its name's owner.
    to: String,
}

impl<'a> Callee<'a> {
    /// `daemon` on `bus`, reached as `reach` says; with [`Reach::Running`], unavailable while
    /// nothing owns the daemon's name.
    async fn new(
        bus: &'a Connection,
        daemon: PowerDaemon,
        reach: Reach,
    ) -> Result<Self, DbusError> {
        let to = match reach {
            Reach::Start => daemon.name().to_owned(),
            Reach::Running => name_owner(bus, daemon.name())
                .await?
                .ok_or_else(|| DbusError::Unavailable { what: daemon.what() })?
                .to_string(),
        };
        Ok(Self { bus, daemon, to })
    }

    /// The property `name` of `interface` at `path`, read alone with `Get`.
    async fn property<T>(
        &self,
        path: &str,
        interface: &str,
        name: &'static str,
    ) -> Result<T, DbusError>
    where
        T: TryFrom<OwnedValue>,
    {
        let (what, to) = (self.daemon.what(), self.to.as_str());
        let reply = self
            .bus
            .call_method(Some(to), path, Some(PROPERTIES), "Get", &(interface, name))
            .await
            .map_err(|error| DbusError::from_zbus(what, &error))?;
        let value: OwnedValue =
            reply.body().deserialize().map_err(|error| DbusError::from_zbus(what, &error))?;
        T::try_from(value)
            .map_err(|_| DbusError::Invalid { what, detail: format!("{name} has another type") })
    }

    /// The devices `UPower` lists, with `EnumerateDevices`.
    async fn devices(&self) -> Result<Vec<OwnedObjectPath>, DbusError> {
        let (what, to) = (self.daemon.what(), self.to.as_str());
        let reply = self
            .bus
            .call_method(Some(to), UPOWER_PATH, Some(UPOWER), "EnumerateDevices", &())
            .await
            .map_err(|error| DbusError::from_zbus(what, &error))?;
        reply.body().deserialize().map_err(|error| DbusError::from_zbus(what, &error))
    }
}

#[cfg(test)]
mod tests {
    use super::{DbusError, PowerDaemon, PowerReading, PowerState, ProfilesAnswer, UPowerAnswer};
    use super::{failure, reading_of};

    #[test]
    fn a_failure_is_worded_without_the_services_text() {
        let of = |error: DbusError| failure(PowerDaemon::UPower, &error);
        assert_eq!(
            of(DbusError::Unavailable { what: "UPower" }),
            "UPower is not on the system bus",
            "nothing owns the name, or the bus could not start the daemon"
        );
        assert_eq!(
            of(DbusError::DeadlineExceeded { call: "UPower" }),
            "UPower did not answer in time",
            "the connection's method timeout"
        );
        assert_eq!(
            of(DbusError::Refused { what: "UPower", reason: "not authorized" }),
            "UPower refused the reading (not authorized)",
            "a refusal keeps its reason"
        );
        assert_eq!(
            of(DbusError::NotFound { what: "UPower" }),
            "UPower lacks the properties Eileen reads",
            "an older service"
        );
        assert_eq!(
            of(DbusError::Invalid { what: "UPower", detail: "a service's own words".to_owned() }),
            "UPower sent a reply Eileen cannot use",
            "the detail is dropped"
        );
        assert_eq!(
            of(DbusError::Io { what: "UPower", detail: "broken pipe".to_owned() }),
            "the system bus failed (broken pipe)",
            "the bus itself"
        );
    }

    #[test]
    fn the_reading_names_each_daemon_that_did_not_answer() {
        let upower = UPowerAnswer { on_battery: true, batteries: Vec::new() };
        let profiles = ProfilesAnswer {
            daemon: PowerDaemon::LegacyPowerProfiles,
            saver: true,
            saver_held: true,
        };
        let absent = DbusError::Unavailable { what: "the power-profiles daemon" };
        assert_eq!(
            reading_of(Ok(&upower), Err(&absent)),
            PowerReading {
                state: PowerState { has_battery: false, on_battery: true, battery_saver: false },
                answered: vec![PowerDaemon::UPower],
                saver_held: false,
                degraded: vec![
                    "battery saver: the power-profiles daemon is not on the system bus; taken \
                     as off"
                        .to_owned()
                ],
            },
            "no profiles daemon"
        );
        assert_eq!(
            reading_of(Err(&DbusError::Unavailable { what: "UPower" }), Ok(&profiles)),
            PowerReading {
                state: PowerState { has_battery: false, on_battery: false, battery_saver: true },
                answered: vec![PowerDaemon::LegacyPowerProfiles],
                saver_held: true,
                degraded: vec![
                    "power: UPower is not on the system bus; taken as no battery and mains power"
                        .to_owned()
                ],
            },
            "no UPower"
        );
    }
}

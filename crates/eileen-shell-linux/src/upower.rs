//! The power facts over D-Bus: whether the PC has a system battery and runs on it, from `UPower`
//! (docs/LINUX.md sections 4 and 7, phase L1). It is the D-Bus counterpart of the file reads in
//! the `power` module.
//!
//! [`LinuxPower::read`] serves the power state of `MachineProbe::facts` (the `power` field, with
//! its `degraded` entries) and of `MachineProbe::load`, once `eileen-platform` has the trait
//! (BUILD.md section 3.4; [`PowerState`] mirrors its contract type until then). It does not read
//! battery saver yet, which stays `false`.
//!
//! # Sources
//!
//! `UPower` is on the system bus, as [its interface](https://upower.freedesktop.org/docs/)
//! describes it.
//!
//! - `on_battery` is `OnBattery` of `org.freedesktop.UPower` at `/org/freedesktop/UPower`.
//! - `has_battery` is a present system battery among the devices `EnumerateDevices` lists: an
//!   `org.freedesktop.UPower.Device` whose `Type` is 2 (battery), whose `PowerSupply` is true and
//!   whose `IsPresent` is true. `PowerSupply` is true only for the system's own supplies, so a
//!   mouse's or a headset's battery is not the PC's.
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
//!   serial number.
//! - [`LinuxPower::read`] calls the daemon by its name, without D-Bus's no-auto-start flag, so an
//!   installed daemon that is not running yet is started by the bus, as for any other client of
//!   it. When the bus cannot start it (its systemd unit masked or missing, or its program
//!   failing), [`DbusError`] maps the bus's answer to `Unavailable`: the daemon is named as not
//!   on the bus.
//!
//! # Traps
//!
//! - No call has a deadline of its own: give [`LinuxPower`] a connection with a method timeout,
//!   as [`eileen_dbus_linux::system_bus`] makes, or a hung daemon holds the reading. A call
//!   past it is worded "did not answer in time".

use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::DbusError;
use crate::facts::PowerState;

/// `UPower`'s bus name, which is also its root object's interface.
const UPOWER: &str = "org.freedesktop.UPower";

/// `UPower`'s root object.
const UPOWER_PATH: &str = "/org/freedesktop/UPower";

/// The interface of each power device `UPower` lists.
const DEVICE: &str = "org.freedesktop.UPower.Device";

/// The properties interface every object has.
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// `UPower`'s device `Type` for a battery.
const BATTERY: u32 = 2;

/// A daemon the power facts come from, on the system bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerDaemon {
    /// `UPower`, `org.freedesktop.UPower`: `has_battery` and `on_battery`.
    UPower,
}

impl PowerDaemon {
    /// Its bus name.
    const fn name(self) -> &'static str {
        match self {
            Self::UPower => UPOWER,
        }
    }

    /// What its errors name.
    const fn what(self) -> &'static str {
        match self {
            Self::UPower => "UPower",
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

    /// Reads the power facts. It never fails: a daemon that does not answer is named in
    /// [`PowerReading::degraded`].
    pub async fn read(&self) -> PowerReading {
        reading_of(read_upower(&self.bus).await.as_ref())
    }
}

/// One reading of the power facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerReading {
    /// The facts. One whose daemon did not answer is left at its unknown value, `false`, and
    /// named in `degraded`. `battery_saver` is not read yet and stays `false`.
    pub state: PowerState,
    /// The daemons that answered: `UPower` for `has_battery` and `on_battery`.
    pub answered: Vec<PowerDaemon>,
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

/// The reading `UPower`'s answer makes.
fn reading_of(upower: Result<&UPowerAnswer, &DbusError>) -> PowerReading {
    let mut answered = Vec::new();
    let mut degraded = Vec::new();
    match upower {
        Ok(_) => answered.push(PowerDaemon::UPower),
        Err(error) => degraded.push(format!(
            "power: {}; taken as no battery and mains power",
            failure(PowerDaemon::UPower, error)
        )),
    }
    PowerReading { state: state_of(upower.ok()), answered, degraded }
}

/// The state the answer gives; a missing answer leaves its facts `false`.
fn state_of(upower: Option<&UPowerAnswer>) -> PowerState {
    PowerState {
        has_battery: upower.is_some_and(UPowerAnswer::has_battery),
        on_battery: upower.is_some_and(|answer| answer.on_battery),
        battery_saver: false,
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
async fn read_upower(bus: &Connection) -> Result<UPowerAnswer, DbusError> {
    let upower = Callee { bus, daemon: PowerDaemon::UPower };
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

/// One daemon as a reading calls it: by its name, every call bounded by the connection's
/// method timeout.
struct Callee<'a> {
    /// The system bus.
    bus: &'a Connection,
    /// The daemon.
    daemon: PowerDaemon,
}

impl Callee<'_> {
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
        let (what, to) = (self.daemon.what(), self.daemon.name());
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
        let (what, to) = (self.daemon.what(), self.daemon.name());
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
    use super::reading_of;
    use super::{DbusError, PowerDaemon, PowerReading, PowerState, UPowerAnswer, failure};

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
    fn the_reading_names_upower_when_it_did_not_answer() {
        let upower = UPowerAnswer { on_battery: true, batteries: Vec::new() };
        assert_eq!(
            reading_of(Ok(&upower)),
            PowerReading {
                state: PowerState { has_battery: false, on_battery: true, battery_saver: false },
                answered: vec![PowerDaemon::UPower],
                degraded: Vec::new(),
            },
            "UPower answered"
        );
        assert_eq!(
            reading_of(Err(&DbusError::Unavailable { what: "UPower" })),
            PowerReading {
                state: PowerState::default(),
                answered: Vec::new(),
                degraded: vec![
                    "power: UPower is not on the system bus; taken as no battery and mains power"
                        .to_owned()
                ],
            },
            "no UPower"
        );
    }
}

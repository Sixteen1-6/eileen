//! The power facts over D-Bus against a fake `UPower` (its root object, a system battery and a
//! mouse's battery) and a fake profiles daemon on a private bus: each fact, a program's hold, the
//! legacy name, each daemon absent, one the bus cannot start and one that does not answer in
//! time, replies Eileen cannot use, and the stream of changes, with a bus monitor showing that
//! the stream never has the bus start a daemon. No test waits on a wall-clock window: a round
//! trip, a marker call or a change that must come next tells when nothing more is coming.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, next_message, nothing_ready, round_trip, serve};
use eileen_dbus_linux::{Names, owner_changes};
use eileen_shell_linux::facts::PowerState;
use eileen_shell_linux::{LinuxPower, PowerChanges, PowerDaemon, PowerReading};
use zbus::message::{Flags, Type as MessageType};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Str};
use zbus::{Message, MessageStream};

const ROOT: &str = "/org/freedesktop/UPower";
const AC: &str = "/org/freedesktop/UPower/devices/line_power_AC";
const BAT0: &str = "/org/freedesktop/UPower/devices/battery_BAT0";
const MOUSE: &str = "/org/freedesktop/UPower/devices/battery_hidpp_battery_0";
const UPS: &str = "/org/freedesktop/UPower/devices/ups_hiddev0";
const PROFILES: &str = "/org/freedesktop/UPower/PowerProfiles";
const LEGACY: &str = "/net/hadess/PowerProfiles";

// The daemons' bus names.
const UPOWER_NAME: &str = "org.freedesktop.UPower";
const PROFILES_NAME: &str = "org.freedesktop.UPower.PowerProfiles";
const LEGACY_NAME: &str = "net.hadess.PowerProfiles";

// UPower's device types and states, as its documentation numbers them.
const LINE_POWER: u32 = 1;
const BATTERY: u32 = 2;
const UPS_KIND: u32 = 3;
const DISCHARGING: u32 = 2;
const FULLY_CHARGED: u32 = 4;

// The degraded entries a missing daemon leaves.
const NO_UPOWER: &str =
    "power: UPower is not on the system bus; taken as no battery and mains power";
const NO_PROFILES: &str =
    "battery saver: the power-profiles daemon is not on the system bus; taken as off";

/// `UPower`'s root object.
struct FakeUPower {
    on_battery: bool,
    devices: Vec<&'static str>,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl FakeUPower {
    fn enumerate_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices.iter().map(|path| OwnedObjectPath::try_from(*path).unwrap()).collect()
    }

    #[zbus(property)]
    fn on_battery(&self) -> bool {
        self.on_battery
    }

    #[zbus(signal)]
    async fn device_added(emitter: &SignalEmitter<'_>, device: OwnedObjectPath)
    -> zbus::Result<()>;
}

/// One power device, with a property the reader must never ask for.
struct FakeDevice {
    kind: u32,
    power_supply: bool,
    is_present: bool,
    state: u32,
    percentage: f64,
    /// How often `Serial` was read; a `GetAll` reads it too.
    serial_reads: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl FakeDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> u32 {
        self.kind
    }

    #[zbus(property)]
    fn power_supply(&self) -> bool {
        self.power_supply
    }

    #[zbus(property)]
    fn is_present(&self) -> bool {
        self.is_present
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.state
    }

    #[zbus(property)]
    fn percentage(&self) -> f64 {
        self.percentage
    }

    #[zbus(property)]
    fn serial(&self) -> String {
        self.serial_reads.fetch_add(1, Ordering::SeqCst);
        "SN-0042".to_owned()
    }
}

/// A device of `kind`, discharging at 81 percent.
fn device(kind: u32, power_supply: bool, is_present: bool, serials: &Arc<AtomicU32>) -> FakeDevice {
    FakeDevice {
        kind,
        power_supply,
        is_present,
        state: DISCHARGING,
        percentage: 81.0,
        serial_reads: Arc::clone(serials),
    }
}

/// A profiles daemon's object: the active profile and each hold's profile and program.
#[derive(Clone)]
struct Profiles {
    active: &'static str,
    holds: Vec<(&'static str, &'static str)>,
}

impl Profiles {
    fn active(active: &'static str) -> Self {
        Self { active, holds: Vec::new() }
    }

    fn holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        let text = |text: &str| OwnedValue::from(Str::from(text.to_owned()));
        self.holds
            .iter()
            .map(|(profile, program)| {
                HashMap::from([
                    ("ApplicationId".to_owned(), text(program)),
                    ("Profile".to_owned(), text(profile)),
                    ("Reason".to_owned(), text("Battery is low")),
                ])
            })
            .collect()
    }
}

/// The profiles daemon under its current name.
struct CurrentProfiles(Profiles);

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl CurrentProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.0.active.to_owned()
    }

    #[zbus(property)]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.0.holds()
    }
}

/// The profiles daemon under its legacy name.
struct LegacyProfiles(Profiles);

#[zbus::interface(name = "net.hadess.PowerProfiles")]
impl LegacyProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.0.active.to_owned()
    }

    #[zbus(property)]
    fn active_profile_holds(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.0.holds()
    }
}

/// A fake `UPower` on `bus` listing `listed` and serving `devices` beside its root object.
async fn upower_listing(
    bus: &PrivateBus,
    on_battery: bool,
    listed: Vec<&'static str>,
    devices: Vec<(&'static str, FakeDevice)>,
) -> zbus::Connection {
    let root = FakeUPower { on_battery, devices: listed };
    let service = serve(bus, UPOWER_NAME, ROOT, root).await;
    for (path, device) in devices {
        service.object_server().at(path, device).await.unwrap();
    }
    service
}

/// A fake `UPower` on `bus` listing every device it serves.
async fn upower(
    bus: &PrivateBus,
    on_battery: bool,
    devices: Vec<(&'static str, FakeDevice)>,
) -> zbus::Connection {
    let listed = devices.iter().map(|(path, _)| *path).collect();
    upower_listing(bus, on_battery, listed, devices).await
}

/// A laptop's `UPower`: line power, its battery and a wireless mouse's battery, which `UPower`
/// types as a battery but not a power supply. The counter counts reads of any `Serial`.
async fn laptop(bus: &PrivateBus, on_battery: bool) -> (zbus::Connection, Arc<AtomicU32>) {
    let serials = Arc::new(AtomicU32::new(0));
    let mut battery = device(BATTERY, true, true, &serials);
    if !on_battery {
        battery.state = FULLY_CHARGED;
    }
    let devices = vec![
        (AC, device(LINE_POWER, true, true, &serials)),
        (BAT0, battery),
        (MOUSE, device(BATTERY, false, true, &serials)),
    ];
    (upower(bus, on_battery, devices).await, serials)
}

/// The profiles daemon under its current name.
async fn current(bus: &PrivateBus, profiles: Profiles) -> zbus::Connection {
    serve(bus, PROFILES_NAME, PROFILES, CurrentProfiles(profiles)).await
}

/// The profiles daemon under its legacy name only, as before power-profiles-daemon 0.20.
async fn legacy(bus: &PrivateBus, profiles: Profiles) -> zbus::Connection {
    serve(bus, LEGACY_NAME, LEGACY, LegacyProfiles(profiles)).await
}

/// A reading of `bus` through a new connection.
async fn read(bus: &PrivateBus) -> PowerReading {
    LinuxPower::new(&bus.connect().await).read().await
}

const fn state(has_battery: bool, on_battery: bool, battery_saver: bool) -> PowerState {
    PowerState { has_battery, on_battery, battery_saver }
}

fn reading(
    state: PowerState,
    answered: &[PowerDaemon],
    saver_held: bool,
    degraded: &[&str],
) -> PowerReading {
    PowerReading {
        state,
        answered: answered.to_vec(),
        saver_held,
        degraded: degraded.iter().map(|entry| (*entry).to_owned()).collect(),
    }
}

/// Both daemons answered.
const BOTH: [PowerDaemon; 2] = [PowerDaemon::UPower, PowerDaemon::PowerProfiles];

#[tokio::test]
async fn a_laptop_on_battery_with_saver_on_gives_every_fact() {
    let bus = PrivateBus::start();
    let (_upower, serials) = laptop(&bus, true).await;
    let _profiles = current(&bus, Profiles::active("power-saver")).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, true, true), &BOTH, false, &[]), "every fact");
    assert_eq!(serials.load(Ordering::SeqCst), 0, "no serial number was read: never GetAll");
}

#[tokio::test]
async fn a_laptop_on_mains_with_the_balanced_profile_gives_its_battery_only() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, false).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, false, false), &BOTH, false, &[]), "on mains");
}

#[tokio::test]
async fn a_mouse_battery_or_an_empty_bay_is_not_the_pcs_battery() {
    let bus = PrivateBus::start();
    let serials = Arc::new(AtomicU32::new(0));
    let devices = vec![
        (AC, device(LINE_POWER, true, true, &serials)),
        // The bay is empty: the battery is listed, not present.
        (BAT0, device(BATTERY, true, false, &serials)),
        (MOUSE, device(BATTERY, false, true, &serials)),
    ];
    let _upower = upower(&bus, false, devices).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let reading = read(&bus).await;
    assert_eq!(reading.state, state(false, false, false), "neither is a present system battery");
    assert!(reading.degraded.is_empty(), "both answered: {:?}", reading.degraded);
}

#[tokio::test]
async fn a_desktop_on_a_discharging_ups_is_on_battery_without_a_battery() {
    let bus = PrivateBus::start();
    let serials = Arc::new(AtomicU32::new(0));
    let _upower = upower(&bus, true, vec![(UPS, device(UPS_KIND, true, true, &serials))]).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let reading = read(&bus).await;
    assert_eq!(reading.state, state(false, true, false), "OnBattery covers the UPS: not on mains");
}

#[tokio::test]
async fn a_programs_hold_on_power_saver_turns_battery_saver_on() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let held = Profiles {
        active: "power-saver",
        holds: vec![("power-saver", "org.gnome.SettingsDaemon.Power")],
    };
    let _profiles = current(&bus, held).await;
    let reading = read(&bus).await;
    assert_eq!(
        reading,
        self::reading(state(true, true, true), &BOTH, true, &[]),
        "the active profile is the hold's, and the reading says a program holds it"
    );
}

#[tokio::test]
async fn a_performance_hold_is_not_battery_saver() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, false).await;
    let held = Profiles { active: "performance", holds: vec![("performance", "org.example.Game")] };
    let _profiles = current(&bus, held).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, false, false), &BOTH, false, &[]), "no saver");
}

#[tokio::test]
async fn the_legacy_name_answers_when_the_current_one_is_absent() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let _profiles = legacy(&bus, Profiles::active("power-saver")).await;
    let reading = read(&bus).await;
    let answered = [PowerDaemon::UPower, PowerDaemon::LegacyPowerProfiles];
    assert_eq!(reading, self::reading(state(true, true, true), &answered, false, &[]), "legacy");
}

#[tokio::test]
async fn the_current_name_is_read_before_the_legacy_one() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let _current = current(&bus, Profiles::active("balanced")).await;
    let _legacy = legacy(&bus, Profiles::active("power-saver")).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, true, false), &BOTH, false, &[]), "current");
}

#[tokio::test]
async fn without_upower_the_battery_facts_are_degraded_not_reported_as_facts() {
    let bus = PrivateBus::start();
    let _profiles = current(&bus, Profiles::active("power-saver")).await;
    let reading = read(&bus).await;
    let answered = [PowerDaemon::PowerProfiles];
    assert_eq!(
        reading,
        self::reading(state(false, false, true), &answered, false, &[NO_UPOWER]),
        "UPower absent"
    );
}

#[tokio::test]
async fn without_a_profiles_daemon_battery_saver_is_degraded() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let reading = read(&bus).await;
    let answered = [PowerDaemon::UPower];
    assert_eq!(
        reading,
        self::reading(state(true, true, false), &answered, false, &[NO_PROFILES]),
        "neither profiles name is owned"
    );
}

#[tokio::test]
async fn with_neither_daemon_every_fact_is_degraded() {
    let bus = PrivateBus::start();
    let reading = read(&bus).await;
    assert_eq!(
        reading,
        self::reading(PowerState::default(), &[], false, &[NO_UPOWER, NO_PROFILES]),
        "nothing answered"
    );
}

/// A root object whose `OnBattery` is a string.
struct WrongRoot {
    on_battery: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl WrongRoot {
    #[zbus(property)]
    fn on_battery(&self) -> String {
        self.on_battery.to_owned()
    }
}

/// A profiles object whose `ActiveProfile` is a number.
struct WrongProfiles {
    active: u32,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl WrongProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> u32 {
        self.active
    }
}

#[tokio::test]
async fn a_reply_of_another_type_is_degraded_not_trusted() {
    let bus = PrivateBus::start();
    let _upower = serve(&bus, UPOWER_NAME, ROOT, WrongRoot { on_battery: "yes" }).await;
    let _profiles = serve(&bus, PROFILES_NAME, PROFILES, WrongProfiles { active: 2 }).await;
    let reading = read(&bus).await;
    assert_eq!(
        reading,
        self::reading(
            PowerState::default(),
            &[],
            false,
            &[
                "power: UPower sent a reply Eileen cannot use; taken as no battery and mains power",
                "battery saver: the power-profiles daemon sent a reply Eileen cannot use; taken \
                 as off",
            ]
        ),
        "neither reply is used"
    );
}

/// A device whose `Type` is a string.
struct WrongDevice {
    kind: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UPower.Device")]
impl WrongDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        self.kind.to_owned()
    }
}

#[tokio::test]
async fn a_device_of_another_shape_degrades_upower_instead_of_dropping_a_battery() {
    let bus = PrivateBus::start();
    let root = FakeUPower { on_battery: true, devices: vec![BAT0] };
    let upower = serve(&bus, UPOWER_NAME, ROOT, root).await;
    upower.object_server().at(BAT0, WrongDevice { kind: "battery" }).await.unwrap();
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let reading = read(&bus).await;
    assert_eq!(reading.state, state(false, false, false), "UPower's facts are not used");
    assert_eq!(
        reading.degraded,
        ["power: UPower sent a reply Eileen cannot use; taken as no battery and mains power"],
        "named"
    );
}

#[tokio::test]
async fn a_device_gone_since_the_listing_is_skipped() {
    let bus = PrivateBus::start();
    let serials = Arc::new(AtomicU32::new(0));
    let gone = "/org/freedesktop/UPower/devices/battery_hidpp_battery_1";
    let devices = vec![(BAT0, device(BATTERY, true, true, &serials))];
    let _upower = upower_listing(&bus, true, vec![gone, BAT0], devices).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, true, false), &BOTH, false, &[]), "skipped");
}

/// A root object without `OnBattery`.
struct OlderRoot {
    version: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl OlderRoot {
    #[zbus(property)]
    fn daemon_version(&self) -> String {
        self.version.to_owned()
    }
}

/// A root object that refuses its reader with a message of its own.
struct RefusingRoot {
    message: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UPower")]
impl RefusingRoot {
    #[zbus(property)]
    fn on_battery(&self) -> zbus::fdo::Result<bool> {
        Err(zbus::fdo::Error::AccessDenied(self.message.to_owned()))
    }
}

/// A profiles object that refuses its reader with a message of its own.
struct RefusingProfiles {
    message: &'static str,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl RefusingProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::AccessDenied(self.message.to_owned()))
    }
}

#[tokio::test]
async fn a_missing_property_or_a_refusal_is_named_without_the_services_text() {
    let bus = PrivateBus::start();
    let _upower = serve(&bus, UPOWER_NAME, ROOT, OlderRoot { version: "0.9.1" }).await;
    let refusing = RefusingProfiles { message: "the service's private words" };
    let _profiles = serve(&bus, PROFILES_NAME, PROFILES, refusing).await;
    let reading = read(&bus).await;
    assert_eq!(
        reading.degraded,
        [
            "power: UPower lacks the properties Eileen reads; taken as no battery and mains power",
            "battery saver: the power-profiles daemon refused the reading (not authorized); \
             taken as off",
        ],
        "named in Eileen's own words"
    );
    assert_eq!(reading.state, PowerState::default(), "nothing taken from either");

    let refusing = PrivateBus::start();
    let words = RefusingRoot { message: "the service's private words" };
    let _upower = serve(&refusing, UPOWER_NAME, ROOT, words).await;
    let _profiles = current(&refusing, Profiles::active("balanced")).await;
    assert_eq!(
        read(&refusing).await.degraded,
        ["power: UPower refused the reading (not authorized); taken as no battery and mains power"],
        "UPower's refusal, without its words"
    );
}

/// A profiles object with an older daemon's shape: no hold list, or one of another type.
struct OddHolds {
    active: &'static str,
    holds: bool,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl OddHolds {
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.active.to_owned()
    }

    #[zbus(property)]
    fn active_profile_holds(&self) -> zbus::fdo::Result<Vec<String>> {
        if self.holds {
            Ok(vec!["power-saver".to_owned()])
        } else {
            Err(zbus::fdo::Error::UnknownProperty("no holds before 0.9".to_owned()))
        }
    }
}

#[tokio::test]
async fn an_unreadable_hold_list_leaves_battery_saver_to_the_active_profile() {
    for holds in [false, true] {
        let bus = PrivateBus::start();
        let (_upower, _) = laptop(&bus, true).await;
        let odd = OddHolds { active: "power-saver", holds };
        let _profiles = serve(&bus, PROFILES_NAME, PROFILES, odd).await;
        let reading = read(&bus).await;
        assert_eq!(
            reading,
            self::reading(state(true, true, true), &BOTH, false, &[]),
            "battery saver from ActiveProfile alone (a hold list of another type: {holds})"
        );
    }
}

/// A daemon's name whose every call comes back with the error `error`, as the bus answers a call
/// it could not start the daemon for: systemd's error for a masked or missing unit, or the bus's
/// own for a program that failed. It answers only while [`Unstartable::refuse`] runs.
struct Unstartable {
    connection: zbus::Connection,
    calls: MessageStream,
    error: &'static str,
}

impl Unstartable {
    async fn on(bus: &PrivateBus, name: &'static str, error: &'static str) -> Self {
        let connection = owner(bus, name).await;
        let calls = MessageStream::from(&connection);
        Self { connection, calls, error }
    }

    /// Answers each call with the error, in words of the bus's own, until the connection closes.
    async fn refuse(&mut self) {
        while let Some(message) = eileen_dbus_linux::next(&mut self.calls).await {
            if message.message_type() == MessageType::MethodCall {
                let reply = Message::error(&message.header(), self.error)
                    .unwrap()
                    .build(&("the unit or program, in the bus's own words",))
                    .unwrap();
                self.connection.send(&reply).await.unwrap();
            }
        }
    }
}

/// A connection on `bus` that owns `name` and serves nothing, so it answers no call.
async fn owner(bus: &PrivateBus, name: &'static str) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address())
        .unwrap()
        .name(name)
        .unwrap()
        .build()
        .await
        .unwrap()
}

// What the bus answers when it could not start a daemon: systemd's errors for a masked unit and
// for a missing one, and dbus-daemon's for a program that exited.
const UNIT_MASKED: &str = "org.freedesktop.systemd1.UnitMasked";
const NO_SUCH_UNIT: &str = "org.freedesktop.systemd1.NoSuchUnit";
const CHILD_EXITED: &str = "org.freedesktop.DBus.Error.Spawn.ChildExited";

#[tokio::test]
async fn a_daemon_the_bus_cannot_start_is_named_absent_not_a_bad_reply() {
    for error in [CHILD_EXITED, UNIT_MASKED, NO_SUCH_UNIT] {
        let bus = PrivateBus::start();
        let mut no_upower = Unstartable::on(&bus, UPOWER_NAME, error).await;
        let mut no_profiles = Unstartable::on(&bus, PROFILES_NAME, error).await;
        let mut no_legacy = Unstartable::on(&bus, LEGACY_NAME, error).await;
        let power = LinuxPower::new(&bus.connect().await);
        let reading = tokio::select! {
            reading = power.read() => reading,
            () = no_upower.refuse() => panic!("the fake UPower's connection closed"),
            () = no_profiles.refuse() => panic!("the fake profiles daemon's connection closed"),
            () = no_legacy.refuse() => panic!("the fake legacy daemon's connection closed"),
        };
        assert_eq!(
            reading,
            self::reading(PowerState::default(), &[], false, &[NO_UPOWER, NO_PROFILES]),
            "a unit or a launcher that failed ({error}) is a daemon that is not there"
        );
    }
}

#[tokio::test]
async fn a_current_name_the_bus_cannot_start_falls_back_to_the_legacy_one() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let mut missing = Unstartable::on(&bus, PROFILES_NAME, NO_SUCH_UNIT).await;
    let _legacy = legacy(&bus, Profiles::active("power-saver")).await;
    let power = LinuxPower::new(&bus.connect().await);
    let reading = tokio::select! {
        reading = power.read() => reading,
        () = missing.refuse() => panic!("the fake profiles daemon's connection closed"),
    };
    let answered = [PowerDaemon::UPower, PowerDaemon::LegacyPowerProfiles];
    assert_eq!(
        reading,
        self::reading(state(true, true, true), &answered, false, &[]),
        "read under the legacy name"
    );
}

#[tokio::test]
async fn a_failing_current_daemon_is_not_asked_again_under_the_legacy_name() {
    let refused = "battery saver: the power-profiles daemon refused the reading (not authorized); \
                   taken as off";
    let unusable =
        "battery saver: the power-profiles daemon sent a reply Eileen cannot use; taken as off";
    for refusing in [true, false] {
        let bus = PrivateBus::start();
        let (_upower, _) = laptop(&bus, true).await;
        let (_current, entry) = if refusing {
            let words = RefusingProfiles { message: "the service's private words" };
            (serve(&bus, PROFILES_NAME, PROFILES, words).await, refused)
        } else {
            (serve(&bus, PROFILES_NAME, PROFILES, WrongProfiles { active: 2 }).await, unusable)
        };
        // A reading that asked the legacy name would take its battery saver.
        let _legacy = legacy(&bus, Profiles::active("power-saver")).await;
        let reading = read(&bus).await;
        assert_eq!(
            reading,
            self::reading(state(true, true, false), &[PowerDaemon::UPower], false, &[entry]),
            "the current daemon's failure stands, and the legacy name is not read: {entry}"
        );
    }
}

#[tokio::test]
async fn a_daemon_that_does_not_answer_in_time_is_named_so() {
    let bus = PrivateBus::start();
    let _hung_upower = owner(&bus, UPOWER_NAME).await;
    let _hung_profiles = owner(&bus, PROFILES_NAME).await;
    let client = bus.connect_within(Duration::from_secs(5)).await;
    // Paused only now: the connections above are made in real time. From here every call goes
    // to a daemon that never answers, so the only timer is the method timeout, which tokio fires
    // as soon as the runtime is idle.
    tokio::time::pause();
    let reading = LinuxPower::new(&client).read().await;
    assert_eq!(
        reading,
        self::reading(
            PowerState::default(),
            &[],
            false,
            &[
                "power: UPower did not answer in time; taken as no battery and mains power",
                "battery saver: the power-profiles daemon did not answer in time; taken as off",
            ]
        ),
        "the connection's method timeout passed for each"
    );
}

/// The stream of the power facts on `client`'s bus.
async fn follow(client: &zbus::Connection) -> PowerChanges {
    LinuxPower::new(client).changes().await.unwrap()
}

/// The stream's next state; nextest's slow-timeout ends a test that would wait for good.
async fn next(changes: &mut PowerChanges) -> PowerState {
    changes.next().await.expect("the stream is still open")
}

/// Waits until `owners` reports that its name lost its owner and none took it. The stream shares
/// its subscription with the power stream's on the same connection, so that one has it too.
async fn stopped_in(owners: &mut eileen_dbus_linux::OwnerChanges) {
    while let Some(change) = owners.next().await {
        if change.new.is_none() {
            return;
        }
    }
    panic!("the connection closed before the name lost its owner");
}

/// Polls `changes` once without waiting, so that a reading the waiting signals would start sends
/// its first call now, before the marker call of `Monitor::seen`. It shows nothing by itself: a
/// reading takes a round trip, so one poll never yields a state. The monitor's calls, and the
/// state that must come next, are what a test asserts.
fn poll_once(changes: &mut PowerChanges) {
    let _pending = nothing_ready(changes);
}

#[tokio::test]
async fn the_stream_yields_the_new_state_when_the_power_source_changes() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, false).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    assert_eq!(changes.state(), state(true, false, false), "it starts from a reading");
    let reading = LinuxPower::new(&client).read().await;
    assert_eq!(changes.state(), reading.state, "the same as a reading");

    let root = service.object_server().interface::<_, FakeUPower>(ROOT).await.unwrap();
    root.get_mut().await.on_battery = true;
    root.get().await.on_battery_changed(root.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, true, false), "unplugged");
    assert_eq!(changes.state(), state(true, true, false), "the state it last yielded");

    root.get_mut().await.on_battery = false;
    root.get().await.on_battery_changed(root.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, false, false), "plugged in again");
}

#[tokio::test]
async fn the_stream_follows_battery_saver_and_ignores_the_charge_level() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, true).await;
    let profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    let mut monitor = Monitor::of(&bus, &client).await;

    let battery = service.object_server().interface::<_, FakeDevice>(BAT0).await.unwrap();
    battery.get_mut().await.percentage = 80.0;
    battery.get().await.percentage_changed(battery.signal_emitter()).await.unwrap();
    // Back from a round trip to UPower, the client holds every signal UPower sent before it.
    round_trip(&client, Some(UPOWER_NAME)).await;
    poll_once(&mut changes);
    // No call is no reading, and the stream's states come only from readings.
    assert_eq!(
        made(&monitor.seen(&client).await),
        Vec::<String>::new(),
        "the charge level is no cause to read, so no new state"
    );

    let object = profiles.object_server().interface::<_, CurrentProfiles>(PROFILES).await.unwrap();
    object.get_mut().await.0.active = "power-saver";
    object.get().await.active_profile_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, true, true), "battery saver on");
    let _reading = monitor.seen(&client).await;
    round_trip(&client, Some(PROFILES_NAME)).await;
    poll_once(&mut changes);
    assert_eq!(
        made(&monitor.seen(&client).await),
        Vec::<String>::new(),
        "nothing more: not even a reading"
    );
}

#[tokio::test]
async fn the_stream_follows_a_programs_hold() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;

    let object = profiles.object_server().interface::<_, CurrentProfiles>(PROFILES).await.unwrap();
    let mut held = object.get_mut().await;
    held.0.active = "power-saver";
    held.0.holds = vec![("power-saver", "org.gnome.SettingsDaemon.Power")];
    drop(held);
    object.get().await.active_profile_holds_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, true, true), "held");
    let reading = LinuxPower::new(&client).read().await;
    assert!(reading.saver_held, "a reading says a program holds it");
}

#[tokio::test]
async fn the_stream_follows_the_system_battery_leaving_its_bay() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, false).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;

    let battery = service.object_server().interface::<_, FakeDevice>(BAT0).await.unwrap();
    battery.get_mut().await.is_present = false;
    battery.get().await.is_present_changed(battery.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(false, false, false), "no battery now");
}

#[tokio::test]
async fn the_stream_watches_a_battery_that_appears_later() {
    let bus = PrivateBus::start();
    let serials = Arc::new(AtomicU32::new(0));
    let service = upower(&bus, false, vec![(AC, device(LINE_POWER, true, true, &serials))]).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    assert_eq!(changes.state(), state(false, false, false), "no battery yet");

    service.object_server().at(BAT0, device(BATTERY, true, true, &serials)).await.unwrap();
    let root = service.object_server().interface::<_, FakeUPower>(ROOT).await.unwrap();
    root.get_mut().await.devices.push(BAT0);
    let path = OwnedObjectPath::try_from(BAT0).unwrap();
    FakeUPower::device_added(root.signal_emitter(), path).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, false, false), "the battery added");

    let battery = service.object_server().interface::<_, FakeDevice>(BAT0).await.unwrap();
    battery.get_mut().await.is_present = false;
    battery.get().await.is_present_changed(battery.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(false, false, false), "and followed since");
}

#[tokio::test]
async fn the_stream_reads_a_daemon_that_starts_later() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, true).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    assert_eq!(changes.state(), state(true, true, false), "no profiles daemon yet");

    let _profiles = current(&bus, Profiles::active("power-saver")).await;
    assert_eq!(next(&mut changes).await, state(true, true, true), "read once it started");
}

#[tokio::test]
async fn a_daemon_that_stops_keeps_its_last_facts() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, true).await;
    let profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    let mut owners = owner_changes(&client, Names::Name(UPOWER_NAME)).await.unwrap();
    let mut monitor = Monitor::of(&bus, &client).await;

    service.close().await.unwrap();
    stopped_in(&mut owners).await;
    poll_once(&mut changes);
    // No call is no reading, and the stream's states come only from readings.
    assert_eq!(
        made(&monitor.seen(&client).await),
        Vec::<String>::new(),
        "a daemon stopping is no cause to read, so no new state"
    );

    let object = profiles.object_server().interface::<_, CurrentProfiles>(PROFILES).await.unwrap();
    object.get_mut().await.0.active = "power-saver";
    object.get().await.active_profile_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(
        next(&mut changes).await,
        state(true, true, true),
        "UPower's last facts stand while it is away"
    );
}

/// A monitor of the bus (D-Bus's `BecomeMonitor`): the calls one connection sends and each name's
/// change of owner, in the order the bus carries them. The private bus starts no daemon, so this
/// is how a test sees a call that a real system bus would start a daemon for.
struct Monitor {
    _connection: zbus::Connection,
    messages: MessageStream,
}

/// The name a marker call asks about.
const MARKER: &str = "org.eileen.Test.Marker";

impl Monitor {
    /// Starts watching what `client` sends on `bus`.
    async fn of(bus: &PrivateBus, client: &zbus::Connection) -> Self {
        let connection = bus.connect().await;
        let messages = MessageStream::from(&connection);
        let calls = format!("type='method_call',sender='{}'", client.unique_name().unwrap());
        let owners = "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'";
        connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus.Monitoring"),
                "BecomeMonitor",
                &(vec![calls.as_str(), owners], 0_u32),
            )
            .await
            .unwrap();
        Self { _connection: connection, messages }
    }

    /// What it has seen since it started or was last asked, up to a marker call that `client`
    /// makes now. The bus carries each connection's messages in order, so all that `client`
    /// sent before the marker comes before it.
    async fn seen(&mut self, client: &zbus::Connection) -> Vec<Message> {
        client
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(MARKER,),
            )
            .await
            .unwrap();
        let mut seen = Vec::new();
        loop {
            let message = next_message(&mut self.messages).await;
            let marker = message.message_type() == MessageType::MethodCall
                && message.body().deserialize::<&str>().is_ok_and(|name| name == MARKER);
            if marker {
                return seen;
            }
            seen.push(message);
        }
    }
}

/// The calls among `seen`, other than the tests' own round trips (`org.freedesktop.DBus.Peer`).
fn calls(seen: &[Message]) -> Vec<&Message> {
    seen.iter()
        .filter(|message| message.message_type() == MessageType::MethodCall)
        .filter(|call| {
            call.header()
                .interface()
                .is_none_or(|interface| interface != "org.freedesktop.DBus.Peer")
        })
        .collect()
}

/// A call as its method and destination.
fn described(call: &Message) -> String {
    let header = call.header();
    let member = header.member().map(|member| member.as_str().to_owned()).unwrap_or_default();
    let to = header.destination().map(|name| name.as_str().to_owned()).unwrap_or_default();
    format!("{member} to {to}")
}

/// The calls among `seen`, described.
fn made(seen: &[Message]) -> Vec<String> {
    calls(seen).into_iter().map(described).collect()
}

/// Whether the bus may start a daemon for `call`: one to a daemon's name, rather than to the bus
/// itself or to a unique name, without the no-auto-start flag.
fn may_start_a_daemon(call: &Message) -> bool {
    let header = call.header();
    let to = header.destination().map(|name| name.as_str().to_owned()).unwrap_or_default();
    let flagged = header.primary().flags().contains(Flags::NoAutoStart);
    !flagged && !to.starts_with(':') && to != "org.freedesktop.DBus"
}

/// Whether `message` is the bus saying that `name` lost its owner and none took it.
fn stopped(message: &Message, name: &str) -> bool {
    message.header().member().is_some_and(|member| member == "NameOwnerChanged")
        && message
            .body()
            .deserialize::<(&str, &str, &str)>()
            .is_ok_and(|(owned, _, new_owner)| owned == name && new_owner.is_empty())
}

#[tokio::test]
async fn a_daemon_that_stops_is_not_called_again() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, true).await;
    let _profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    let mut owners = owner_changes(&client, Names::Name(UPOWER_NAME)).await.unwrap();
    let mut monitor = Monitor::of(&bus, &client).await;

    service.close().await.unwrap();
    stopped_in(&mut owners).await;
    poll_once(&mut changes);
    let seen = monitor.seen(&client).await;
    assert!(seen.iter().any(|message| stopped(message, UPOWER_NAME)), "the bus saw UPower stop");
    assert_eq!(
        made(&seen),
        Vec::<String>::new(),
        "no call once UPower stopped: it keeps its facts"
    );
}

#[tokio::test]
async fn the_watch_never_has_the_bus_start_a_daemon() {
    let bus = PrivateBus::start();
    let (service, _) = laptop(&bus, true).await;
    let profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    let mut monitor = Monitor::of(&bus, &client).await;
    let object = profiles.object_server().interface::<_, CurrentProfiles>(PROFILES).await.unwrap();

    // A change read while its daemon runs.
    object.get_mut().await.0.active = "power-saver";
    object.get().await.active_profile_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, true, true), "battery saver on");

    // A change the watch reads only once its daemon has stopped, as when UPower signals and then
    // stops, or crashes in the middle of a reading: the client holds the signal, unread, when
    // UPower goes.
    let root = service.object_server().interface::<_, FakeUPower>(ROOT).await.unwrap();
    root.get_mut().await.on_battery = false;
    root.get().await.on_battery_changed(root.signal_emitter()).await.unwrap();
    drop(root);
    round_trip(&client, Some(UPOWER_NAME)).await;
    service.close().await.unwrap();

    // The next state comes from a reading after UPower's, whose last facts stand.
    object.get_mut().await.0.active = "balanced";
    object.get().await.active_profile_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, true, false), "UPower's last facts stand");

    let seen = monitor.seen(&client).await;
    let calls = calls(&seen);
    let made: Vec<String> = calls.iter().copied().map(described).collect();
    assert!(
        made.iter().filter(|call| call.starts_with("Get to :")).count() >= 4,
        "the watch read the profiles daemon twice at its owner's unique name: {made:?}"
    );
    let starting: Vec<String> =
        calls.into_iter().filter(|call| may_start_a_daemon(call)).map(described).collect();
    assert_eq!(starting, Vec::<String>::new(), "no call the bus could start a daemon for");
}

#[tokio::test]
async fn the_stream_follows_the_legacy_name() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, false).await;
    let profiles = legacy(&bus, Profiles::active("power-saver")).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    assert_eq!(changes.state(), state(true, false, true), "read under the legacy name");

    let object = profiles.object_server().interface::<_, LegacyProfiles>(LEGACY).await.unwrap();
    object.get_mut().await.0.active = "balanced";
    object.get().await.active_profile_changed(object.signal_emitter()).await.unwrap();
    assert_eq!(next(&mut changes).await, state(true, false, false), "battery saver off");
}

#[tokio::test]
async fn the_stream_ends_when_the_connection_closes() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, false).await;
    let client = bus.connect().await;
    let mut changes = follow(&client).await;
    client.close().await.unwrap();
    assert_eq!(changes.next().await, None, "the stream ends with its connection");
}

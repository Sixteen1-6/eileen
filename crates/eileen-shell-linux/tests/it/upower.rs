//! The power facts over D-Bus against a fake `UPower` (its root object, a system battery and a
//! mouse's battery) and a fake profiles daemon on a private bus: each fact, a program's hold, the
//! legacy name, each daemon absent, one the bus cannot start and one that does not answer in
//! time, and replies Eileen cannot use.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_shell_linux::facts::PowerState;
use eileen_shell_linux::{LinuxPower, PowerDaemon, PowerReading};
use zbus::message::Type as MessageType;
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

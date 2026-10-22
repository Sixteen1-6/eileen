//! The power facts over D-Bus against a fake `UPower` (its root object, a system battery and a
//! mouse's battery) on a private bus: each fact, the daemon absent, one the bus cannot start and
//! one that does not answer in time, and replies Eileen cannot use.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_shell_linux::facts::PowerState;
use eileen_shell_linux::{LinuxPower, PowerDaemon, PowerReading};
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Message, MessageStream};

const ROOT: &str = "/org/freedesktop/UPower";
const AC: &str = "/org/freedesktop/UPower/devices/line_power_AC";
const BAT0: &str = "/org/freedesktop/UPower/devices/battery_BAT0";
const MOUSE: &str = "/org/freedesktop/UPower/devices/battery_hidpp_battery_0";
const UPS: &str = "/org/freedesktop/UPower/devices/ups_hiddev0";

// UPower's device types and states, as its documentation numbers them.
const LINE_POWER: u32 = 1;
const BATTERY: u32 = 2;
const UPS_KIND: u32 = 3;
const DISCHARGING: u32 = 2;
const FULLY_CHARGED: u32 = 4;

// The degraded entry a missing UPower leaves.
const NO_UPOWER: &str =
    "power: UPower is not on the system bus; taken as no battery and mains power";

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

/// A fake `UPower` on `bus` listing `listed` and serving `devices` beside its root object.
async fn upower_listing(
    bus: &PrivateBus,
    on_battery: bool,
    listed: Vec<&'static str>,
    devices: Vec<(&'static str, FakeDevice)>,
) -> zbus::Connection {
    let root = FakeUPower { on_battery, devices: listed };
    let service = serve(bus, "org.freedesktop.UPower", ROOT, root).await;
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

/// A reading of `bus` through a new connection.
async fn read(bus: &PrivateBus) -> PowerReading {
    LinuxPower::new(&bus.connect().await).read().await
}

const fn state(has_battery: bool, on_battery: bool, battery_saver: bool) -> PowerState {
    PowerState { has_battery, on_battery, battery_saver }
}

fn reading(state: PowerState, answered: &[PowerDaemon], degraded: &[&str]) -> PowerReading {
    PowerReading {
        state,
        answered: answered.to_vec(),
        degraded: degraded.iter().map(|entry| (*entry).to_owned()).collect(),
    }
}

/// `UPower` answered.
const UPOWER: [PowerDaemon; 1] = [PowerDaemon::UPower];

#[tokio::test]
async fn a_laptop_on_battery_gives_its_battery_facts() {
    let bus = PrivateBus::start();
    let (_upower, serials) = laptop(&bus, true).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, true, false), &UPOWER, &[]), "every fact");
    assert_eq!(serials.load(Ordering::SeqCst), 0, "no serial number was read: never GetAll");
}

#[tokio::test]
async fn a_laptop_on_mains_gives_its_battery_only() {
    let bus = PrivateBus::start();
    let (_upower, _) = laptop(&bus, false).await;
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, false, false), &UPOWER, &[]), "on mains");
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
    let reading = read(&bus).await;
    assert_eq!(reading.state, state(false, false, false), "neither is a present system battery");
    assert!(reading.degraded.is_empty(), "UPower answered: {:?}", reading.degraded);
}

#[tokio::test]
async fn a_desktop_on_a_discharging_ups_is_on_battery_without_a_battery() {
    let bus = PrivateBus::start();
    let serials = Arc::new(AtomicU32::new(0));
    let _upower = upower(&bus, true, vec![(UPS, device(UPS_KIND, true, true, &serials))]).await;
    let reading = read(&bus).await;
    assert_eq!(reading.state, state(false, true, false), "OnBattery covers the UPS: not on mains");
}

#[tokio::test]
async fn without_upower_the_battery_facts_are_degraded_not_reported_as_facts() {
    let bus = PrivateBus::start();
    let reading = read(&bus).await;
    assert_eq!(
        reading,
        self::reading(PowerState::default(), &[], &[NO_UPOWER]),
        "nothing owns UPower's name"
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

#[tokio::test]
async fn a_reply_of_another_type_is_degraded_not_trusted() {
    let bus = PrivateBus::start();
    let _upower =
        serve(&bus, "org.freedesktop.UPower", ROOT, WrongRoot { on_battery: "yes" }).await;
    let reading = read(&bus).await;
    assert_eq!(
        reading,
        self::reading(
            PowerState::default(),
            &[],
            &["power: UPower sent a reply Eileen cannot use; taken as no battery and mains power"]
        ),
        "the reply is not used"
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
    let upower = serve(&bus, "org.freedesktop.UPower", ROOT, root).await;
    upower.object_server().at(BAT0, WrongDevice { kind: "battery" }).await.unwrap();
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
    let reading = read(&bus).await;
    assert_eq!(reading, self::reading(state(true, true, false), &UPOWER, &[]), "skipped");
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

#[tokio::test]
async fn a_missing_property_or_a_refusal_is_named_without_the_services_text() {
    let older = PrivateBus::start();
    let _upower =
        serve(&older, "org.freedesktop.UPower", ROOT, OlderRoot { version: "0.9.1" }).await;
    assert_eq!(
        read(&older).await.degraded,
        ["power: UPower lacks the properties Eileen reads; taken as no battery and mains power"],
        "an older UPower, in Eileen's own words"
    );
    let refusing = PrivateBus::start();
    let words = RefusingRoot { message: "the service's private words" };
    let _upower = serve(&refusing, "org.freedesktop.UPower", ROOT, words).await;
    assert_eq!(
        read(&refusing).await.degraded,
        ["power: UPower refused the reading (not authorized); taken as no battery and mains power"],
        "a refusal, without the service's words"
    );
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
        let mut no_upower = Unstartable::on(&bus, "org.freedesktop.UPower", error).await;
        let power = LinuxPower::new(&bus.connect().await);
        let reading = tokio::select! {
            reading = power.read() => reading,
            () = no_upower.refuse() => panic!("the fake UPower's connection closed"),
        };
        assert_eq!(
            reading,
            self::reading(PowerState::default(), &[], &[NO_UPOWER]),
            "a unit or a launcher that failed ({error}) is a daemon that is not there"
        );
    }
}

#[tokio::test]
async fn a_daemon_that_does_not_answer_in_time_is_named_so() {
    let bus = PrivateBus::start();
    let _hung = owner(&bus, "org.freedesktop.UPower").await;
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
            &["power: UPower did not answer in time; taken as no battery and mains power"]
        ),
        "the connection's method timeout passed"
    );
}

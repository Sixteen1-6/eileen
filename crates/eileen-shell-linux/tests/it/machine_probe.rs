//! `MachineProbe` on Linux through `LinuxMachineProbe`: the facts within the caller's deadline,
//! with a FIFO that no one writes standing in for a read a hung mount or device holds; and the
//! changes `watch` reports, from the fake power daemons of the `upower` module, first as the
//! stream `MachineChanges` in tokio's paused time, then through a bridge on the private bus,
//! called from the test's thread as the core calls it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eileen_contracts::connectors::Platform;
use eileen_contracts::ids::WallTime;
use eileen_contracts::machine::{AudioFacts, MachineFacts};
use eileen_dbus_linux::Bridge;
use eileen_dbus_linux::testing::{PrivateBus, round_trip};
use eileen_platform::{Deadline, MachineChange, MachineProbe, PlatformError};
use eileen_shell_linux::{LinuxMachineProbe, MachineChanges, SysRoot, read_volume};
use rustix::fs::{CWD, Mode, mkfifoat};
use tokio::sync::mpsc;

use crate::tree;
use crate::upower::{
    PROFILES_NAME, Profiles, UPOWER_NAME, current, follow, laptop, set_on_battery, set_profile,
};

/// A deadline no read of a fixture tree comes near.
const LONG: Deadline = Deadline::after(Duration::from_secs(30));
/// The deadline of a read that never returns: it passes whatever its length, and a short one
/// keeps the test short.
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));

/// A data folder of the test's own that does not exist yet, on the volume the tests run on.
fn data_dir(test: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("machine_probe").join(test).join("eileen")
}

/// `facts` with its free space set aside: other tests write to the same volume meanwhile.
fn without_free_space(mut facts: MachineFacts) -> MachineFacts {
    facts.disk.data_free_mb = 0;
    facts
}

#[test]
fn facts_within_the_deadline_measure_the_data_folders_volume_and_leave_the_rest_to_others() {
    let root = tree::build("intel-hybrid-laptop", "probe_facts");
    let data = data_dir("facts");
    let probe = LinuxMachineProbe::new(SysRoot::at(root), &data);
    let facts = probe.facts(LONG).unwrap();
    assert_eq!(facts.os, Platform::Linux, "Linux");
    assert_eq!(facts.cpu.performance_cores(), 6, "the fixture's processor");
    assert!(facts.disk.data_total_mb > 0, "the real volume, measured at the nearest parent");
    assert_eq!((facts.read_at, facts.probe_ms), (WallTime(0), 0), "the caller stamps these");
    assert_eq!(facts.audio, AudioFacts::default(), "the audio component reports the endpoints");
    assert!(facts.execution_providers.is_empty(), "the speech process reports the providers");
    let on_the_callers_thread = probe.facts_on(read_volume(&data)).unwrap();
    assert_eq!(
        without_free_space(facts),
        without_free_space(on_the_callers_thread),
        "the same pass as on the caller's thread"
    );
}

#[test]
fn a_read_that_never_returns_passes_the_deadline_and_is_left_to_its_thread() {
    let root = tree::build("intel-hybrid-laptop", "probe_held_read");
    let meminfo = root.join("proc/meminfo");
    let text = std::fs::read_to_string(&meminfo).unwrap();
    // Opening a FIFO to read waits for a writer, as a read of a hung mount waits for its server.
    std::fs::remove_file(&meminfo).unwrap();
    mkfifoat(CWD, &meminfo, Mode::RUSR | Mode::WUSR).unwrap();
    let probe = LinuxMachineProbe::new(SysRoot::at(&root), data_dir("held_read"));
    assert_eq!(
        probe.facts(SHORT),
        Err(PlatformError::DeadlineExceeded { call: "MachineProbe::facts" }),
        "the call answers at its deadline"
    );
    // The writer meets the read that waits, which then goes on and ends.
    std::fs::write(&meminfo, &text).unwrap();
    std::fs::remove_file(&meminfo).unwrap();
    std::fs::write(&meminfo, &text).unwrap();
    assert!(probe.facts(LONG).is_ok(), "the next call reads again, on a thread of its own");
}

#[test]
fn a_root_without_meminfo_is_an_io_error_naming_it() {
    let root = tree::build("ubuntu-vm-qemu", "probe_no_meminfo");
    std::fs::remove_file(root.join("proc/meminfo")).unwrap();
    let probe = LinuxMachineProbe::new(SysRoot::at(root), data_dir("no_meminfo"));
    assert!(
        matches!(probe.facts(LONG), Err(PlatformError::Io { what: "/proc/meminfo", .. })),
        "no facts at all"
    );
}

#[test]
fn watch_without_a_bridge_is_unavailable() {
    let probe = LinuxMachineProbe::new(SysRoot::system(), data_dir("no_bridge"));
    let watch = probe.watch(Box::new(|_change| {}));
    assert_eq!(
        watch.err(),
        Some(PlatformError::Unavailable { what: "machine-change watch" }),
        "nothing to follow the daemons on"
    );
}

#[tokio::test(start_paused = true)]
async fn the_power_states_changes_are_the_power_source_and_battery_saver() {
    let bus = PrivateBus::start();
    let (upower, _) = laptop(&bus, false).await;
    let profiles = current(&bus, Profiles::active("balanced")).await;
    let client = bus.connect().await;
    let mut changes = MachineChanges::new(follow(&client).await);

    set_on_battery(&upower, true).await;
    assert_eq!(changes.next().await, Some(MachineChange::PowerSource), "unplugged");
    set_profile(&profiles, "power-saver").await;
    assert_eq!(changes.next().await, Some(MachineChange::BatterySaver), "the saver profile");

    // Both signals wait together, so one reading of both daemons changes both facts.
    set_on_battery(&upower, false).await;
    set_profile(&profiles, "balanced").await;
    round_trip(&client, Some(UPOWER_NAME)).await;
    round_trip(&client, Some(PROFILES_NAME)).await;
    assert_eq!(changes.next().await, Some(MachineChange::PowerSource), "plugged in, first");
    assert_eq!(changes.next().await, Some(MachineChange::BatterySaver), "and the saver off");

    client.close().await.unwrap();
    assert_eq!(changes.next().await, None, "the changes end with the connection");
}

/// The fake daemons, on a runtime of their own whose workers serve them while the test's thread
/// waits on the probe; dropped inside that runtime, a panicking test's included.
struct Daemons {
    runtime: tokio::runtime::Runtime,
    /// `UPower`'s connection, then the profiles daemon's.
    services: Vec<zbus::Connection>,
}

impl Daemons {
    /// A laptop on mains power with the balanced profile.
    fn start(bus: &PrivateBus) -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        let runtime = runtime.unwrap();
        let services = runtime.block_on(async {
            vec![laptop(bus, false).await.0, current(bus, Profiles::active("balanced")).await]
        });
        Self { runtime, services }
    }

    /// Sets `UPower`'s `OnBattery` and signals it.
    fn set_on_battery(&self, on_battery: bool) {
        self.runtime.block_on(set_on_battery(&self.services[0], on_battery));
    }

    /// Sets the active profile and signals it.
    fn set_profile(&self, active: &'static str) {
        self.runtime.block_on(set_profile(&self.services[1], active));
    }
}

impl Drop for Daemons {
    fn drop(&mut self) {
        // The connections end inside their runtime's context.
        let _context = self.runtime.enter();
        self.services.clear();
    }
}

#[test]
fn watch_reports_each_change_through_the_bridge_until_its_subscription_drops() {
    let bus = PrivateBus::start();
    let daemons = Daemons::start(&bus);
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let probe = LinuxMachineProbe::new(SysRoot::system(), data_dir("watch")).with_bridge(bridge);
    let (heard, mut hear) = mpsc::channel(8);
    let watch = probe
        .watch(Box::new(move |change| {
            let _ignored = heard.try_send(change);
        }))
        .unwrap();

    daemons.set_on_battery(true);
    assert_eq!(hear.blocking_recv(), Some(MachineChange::PowerSource), "unplugged");
    daemons.set_profile("power-saver");
    assert_eq!(hear.blocking_recv(), Some(MachineChange::BatterySaver), "the saver profile");

    drop(watch);
    assert_eq!(hear.blocking_recv(), None, "the bridge dropped the sink with the subscription");
}

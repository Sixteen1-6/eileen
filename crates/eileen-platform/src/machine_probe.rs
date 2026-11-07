//! The machine profile's facts and load (BUILD.md sections 2.16 and 3.4).

use eileen_contracts::machine::{MachineFacts, PowerState};

use crate::{Deadline, PlatformError, Subscription};

#[cfg(any(test, feature = "fake"))]
mod fixtures;

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeMachineProbe;
#[cfg(any(test, feature = "fake"))]
pub use fixtures::FixtureMachine;

/// What changed about the machine, so the runtime reads the facts again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachineChange {
    /// The PC moved between mains and battery power.
    PowerSource,
    /// Battery saver turned on or off.
    BatterySaver,
    /// The number of displays changed.
    Displays,
}

/// The machine's facts and load (`eileen-shell-win`: `GetLogicalProcessorInformationEx` and the
/// processor name, `GlobalMemoryStatusEx`, the `DXCore` adapters, the data volume's free space
/// and seek penalty, `CurrentBuild` and `UBR`, `GetSystemPowerStatus` and the monitor count;
/// Linux: `eileen-shell-linux`, from `/proc` and sysfs). The fake holds WP0's three fixture
/// machines.
pub trait MachineProbe: Send + Sync {
    /// Everything but the audio endpoints and the execution providers, which their own
    /// components report, so both are left empty; the caller stamps `read_at` and `probe_ms`
    /// from its own clock around the call. `deadline` bounds measuring the data folder's volume,
    /// which on Linux sits in the home folder, a network mount on some PCs.
    fn facts(&self, deadline: Deadline) -> Result<MachineFacts, PlatformError>;
    /// CPU busy percent since the last call (`GetSystemTimes` deltas) and the power state.
    fn load(&self) -> Result<(u8, PowerState), PlatformError>;
    /// Power source, battery saver and display count changes (`RegisterPowerSettingNotification`,
    /// `WM_DISPLAYCHANGE`).
    fn watch(
        &self,
        sink: Box<dyn Fn(MachineChange) + Send + Sync>,
    ) -> Result<Subscription, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use eileen_contracts::machine::{AudioFacts, MachineFacts, PowerState};
    use eileen_stdx::clock::ManualClock;

    use super::{FixtureMachine, MachineChange, MachineProbe};
    use crate::fake_util::{Sinks, lock};
    use crate::{Deadline, PlatformError, Subscription};

    /// How long a read of the facts takes on a clock the test holds.
    #[derive(Debug, Clone)]
    struct ProbeTime {
        clock: Arc<ManualClock>,
        takes: Duration,
    }

    /// One of WP0's fixture machines, with a load and changes a test sets. Reading the facts
    /// takes no time until a test sets a probe time on a `ManualClock`, so it can answer within
    /// its caller's deadline, or pass it, without sleeping.
    #[derive(Debug)]
    pub struct FakeMachineProbe {
        machine: Mutex<FixtureMachine>,
        load: Mutex<(u8, PowerState)>,
        probe_time: Mutex<Option<ProbeTime>>,
        changes: Sinks<MachineChange>,
    }

    impl FakeMachineProbe {
        /// A probe of `machine`, idle, on its own power state.
        #[must_use]
        pub fn new(machine: FixtureMachine) -> Self {
            let power = machine.facts.power;
            Self {
                machine: Mutex::new(machine),
                load: Mutex::new((0, power)),
                probe_time: Mutex::new(None),
                changes: Sinks::new(),
            }
        }

        /// The machine as it stands, its week of hourly summaries included.
        #[must_use]
        pub fn machine(&self) -> FixtureMachine {
            lock(&self.machine).clone()
        }

        /// From now on the facts read as `facts`, as after a change the OS reports.
        pub fn set_facts(&self, facts: MachineFacts) {
            lock(&self.machine).facts = facts;
        }

        /// From now on [`MachineProbe::load`] answers this.
        pub fn set_load(&self, cpu_busy_pct: u8, power: PowerState) {
            *lock(&self.load) = (cpu_busy_pct, power);
        }

        /// From now on each read of the facts takes `takes` on `clock`; one past its deadline
        /// moves the clock by the deadline's budget alone, the whole of the caller's wait, and
        /// answers [`PlatformError::DeadlineExceeded`].
        pub fn set_probe_time(&self, clock: Arc<ManualClock>, takes: Duration) {
            *lock(&self.probe_time) = Some(ProbeTime { clock, takes });
        }

        /// Reports `change` to every watcher, as the OS would.
        pub fn change(&self, change: MachineChange) {
            self.changes.emit(&change);
        }

        /// How many watchers there are.
        #[must_use]
        pub fn watchers(&self) -> usize {
            self.changes.len()
        }
    }

    impl MachineProbe for FakeMachineProbe {
        fn facts(&self, deadline: Deadline) -> Result<MachineFacts, PlatformError> {
            let probe_time = lock(&self.probe_time).clone();
            if let Some(ProbeTime { clock, takes }) = probe_time {
                let budget = deadline.budget();
                if takes > budget {
                    clock.advance(budget);
                    return Err(PlatformError::DeadlineExceeded { call: "MachineProbe::facts" });
                }
                clock.advance(takes);
            }
            let mut facts = lock(&self.machine).facts.clone();
            facts.audio = AudioFacts::default();
            facts.execution_providers.clear();
            Ok(facts)
        }

        fn load(&self) -> Result<(u8, PowerState), PlatformError> {
            Ok(*lock(&self.load))
        }

        fn watch(
            &self,
            sink: Box<dyn Fn(MachineChange) + Send + Sync>,
        ) -> Result<Subscription, PlatformError> {
            Ok(self.changes.add(sink))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use eileen_contracts::ids::WallTime;
    use eileen_contracts::machine::PowerState;
    use eileen_stdx::clock::{Clock as _, ManualClock};

    use super::{FakeMachineProbe, FixtureMachine, MachineChange, MachineProbe};
    use crate::Deadline;

    #[test]
    fn the_facts_leave_out_what_other_components_report() {
        let probe = FakeMachineProbe::new(FixtureMachine::owners_shape());
        let facts = probe.facts(Deadline::after(Duration::from_secs(1))).unwrap();
        assert!(facts.audio.capture.is_empty(), "the audio component reports the endpoints");
        assert!(facts.execution_providers.is_empty(), "the speech process reports providers");
        assert_eq!(facts.cpu, probe.machine().facts.cpu, "the rest is the fixture's");
    }

    #[test]
    fn a_read_within_its_deadline_moves_the_clock_by_its_own_time() {
        let clock = Arc::new(ManualClock::new(WallTime(0)));
        let probe = FakeMachineProbe::new(FixtureMachine::one_class_laptop());
        probe.set_probe_time(Arc::clone(&clock), Duration::from_millis(150));
        assert!(probe.facts(Deadline::after(Duration::from_millis(200))).is_ok(), "in time");
        assert_eq!(clock.mono().0, 150_000_000, "the read's own 150 ms");
    }

    #[test]
    fn watchers_hear_each_change_until_they_unsubscribe() {
        let probe = FakeMachineProbe::new(FixtureMachine::desktop());
        let heard = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&heard);
        let watch = probe
            .watch(Box::new(move |change| {
                assert_eq!(change, MachineChange::Displays, "the change reported");
                counter.fetch_add(1, Ordering::SeqCst);
            }))
            .unwrap();
        probe.change(MachineChange::Displays);
        assert_eq!(heard.load(Ordering::SeqCst), 1, "heard once");
        drop(watch);
        assert_eq!(probe.watchers(), 0, "unsubscribed");
        probe.change(MachineChange::Displays);
        assert_eq!(heard.load(Ordering::SeqCst), 1, "not heard after");
    }

    #[test]
    fn the_load_starts_idle_on_the_machine_s_own_power_and_follows_the_test() {
        let probe = FakeMachineProbe::new(FixtureMachine::one_class_laptop());
        let power = probe.machine().facts.power;
        assert_eq!(probe.load().unwrap(), (0, power), "idle");
        let saver = PowerState { has_battery: true, on_battery: true, battery_saver: true };
        probe.set_load(35, saver);
        assert_eq!(probe.load().unwrap(), (35, saver), "set");
    }
}

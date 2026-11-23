//! The fake `MachineProbe` and WP0's three fixture machines (acceptance 13 of BUILD.md section
//! 6.2): their facts round-trip through `machine\facts.toml`'s format, each week of hourly
//! summaries holds the usual memory its fixture names, and a read past its deadline answers
//! `DeadlineExceeded` without sleeping.

use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::ids::WallTime;
use eileen_contracts::machine::{AdapterKind, DiskMedia, MachineFacts};
use eileen_platform::{Deadline, FakeMachineProbe, FixtureMachine, MachineProbe, PlatformError};
use eileen_stdx::clock::{Clock as _, ManualClock};
use serde_json::Value;

/// The keys of a `memory-hours.jsonl` line, in the order `samples` writes them (BUILD.md
/// section 6.1).
const HOUR_KEYS: [&str; 10] = [
    "at_ms",
    "avail_p10_mb",
    "avail_p50_mb",
    "avail_p90_mb",
    "headroom_p10_mb",
    "paging_p75_kb_s",
    "plan_items_mb",
    "slot_held_pct",
    "samples",
    "src",
];

/// The value at `pct` percent by the nearest-rank method, as `samples::percentile` takes it.
fn nearest_rank(values: &mut [u64], pct: u64) -> u64 {
    values.sort_unstable();
    let rank = (pct * u64::try_from(values.len()).unwrap()).div_ceil(100).max(1);
    values[usize::try_from(rank - 1).unwrap()]
}

#[test]
fn the_three_fixture_machines_round_trip_through_facts_toml() {
    let machines = FixtureMachine::all();
    let names: Vec<_> = machines.iter().map(|machine| machine.name).collect();
    assert_eq!(names, ["owners-shape", "one-class-laptop", "desktop"], "the three, in order");
    for machine in &machines {
        let text = toml::to_string(&machine.facts).unwrap();
        let back: MachineFacts = toml::from_str(&text).unwrap();
        assert_eq!(back, machine.facts, "{} reads back from:\n{text}", machine.name);
    }
    let [owners, laptop, desktop] = machines;
    let free = |machine: &FixtureMachine| machine.usual.available_mb + machine.usual.plan_items_mb;

    let cpu = &owners.facts.cpu;
    assert_eq!((cpu.physical, cpu.logical), (16, 22), "16 cores, 22 logical processors");
    assert_eq!(cpu.performance_cores(), 6, "6 performance cores");
    let kinds: Vec<_> = owners.facts.adapters.iter().map(|adapter| adapter.kind).collect();
    assert_eq!(kinds, [AdapterKind::IntegratedGpu, AdapterKind::Npu], "an integrated GPU, an NPU");
    assert_eq!(owners.facts.disk.media, DiskMedia::Ssd, "a solid-state disk");
    assert_eq!(owners.facts.memory_total_mb / 1_000, 15, "16 GB, less the firmware's share");
    assert_eq!(free(&owners), 1_800, "usually 1.8 GB free");

    assert_eq!(laptop.facts.cpu.classes.len(), 1, "one class of cores");
    assert_eq!(laptop.facts.cpu.performance_cores(), 4, "4 cores");
    assert_eq!(laptop.facts.adapters[0].kind, AdapterKind::IntegratedGpu, "integrated graphics");
    assert_eq!(laptop.facts.disk.data_free_mb, 30 * 1_024, "30 GB of free disk");
    assert_eq!(laptop.facts.memory_total_mb / 1_000, 7, "8 GB");
    assert_eq!(free(&laptop), 900, "usually 0.9 GB free");

    assert_eq!(desktop.facts.cpu.performance_cores(), 8, "8 performance cores");
    assert_eq!(desktop.facts.adapters[0].kind, AdapterKind::DiscreteGpu, "a discrete GPU");
    assert!(!desktop.facts.power.has_battery, "no battery");
    assert_eq!(desktop.facts.memory_total_mb / 1_000, 32, "32 GB");
    assert_eq!(free(&desktop), 14_000, "usually 14 GB free");
}

#[test]
fn each_fixture_week_holds_the_usual_memory_it_names() {
    for machine in FixtureMachine::all() {
        let name = machine.name;
        let hours: Vec<Value> =
            machine.memory_hours.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(hours.len(), 168, "{name}: a week of hours");
        for (line, hour) in machine.memory_hours.iter().zip(&hours) {
            let mut keys: Vec<_> = hour.as_object().unwrap().keys().map(String::as_str).collect();
            keys.sort_unstable();
            let mut expected = HOUR_KEYS.to_vec();
            expected.sort_unstable();
            assert_eq!(keys, expected, "{name}: the line's keys: {line}");
            assert_eq!(hour["src"], "runtime", "{name}: written by a runtime");
            let mb = |key: &str| hour[key].as_u64().unwrap();
            let (p10, p50, p90) = (mb("avail_p10_mb"), mb("avail_p50_mb"), mb("avail_p90_mb"));
            assert!(p10 <= p50 && p50 <= p90, "{name}: the percentiles in order: {line}");
        }
        let ends: Vec<u64> = hours.iter().map(|hour| hour["at_ms"].as_u64().unwrap()).collect();
        assert!(ends.windows(2).all(|pair| pair[1] - pair[0] == 3_600_000), "{name}: hourly");
        let full: Vec<&Value> =
            hours.iter().filter(|hour| hour["samples"].as_u64().unwrap() >= 360).collect();
        let field = |key: &str| -> Vec<u64> {
            full.iter().map(|hour| hour[key].as_u64().unwrap()).collect()
        };
        let usual = machine.usual;
        assert_eq!(usize::from(usual.hours), full.len(), "{name}: the qualifying hours");
        assert_eq!(
            nearest_rank(&mut field("avail_p50_mb"), 25),
            u64::from(usual.available_mb),
            "{name}: the 25th percentile of the hourly medians"
        );
        assert_eq!(
            nearest_rank(&mut field("plan_items_mb"), 50),
            u64::from(usual.plan_items_mb),
            "{name}: the median of the plan's items"
        );
        assert_eq!(
            nearest_rank(&mut field("headroom_p10_mb"), 25),
            u64::from(usual.headroom_p10_mb),
            "{name}: the usual 10th percentile of headroom"
        );
    }
}

#[test]
fn facts_past_its_deadline_answers_deadline_exceeded() {
    let clock = Arc::new(ManualClock::new(WallTime(0)));
    let probe = FakeMachineProbe::new(FixtureMachine::owners_shape());
    probe.set_probe_time(Arc::clone(&clock), Duration::from_millis(300));
    let late = probe.facts(Deadline::after(Duration::from_millis(200)));
    assert_eq!(
        late,
        Err(PlatformError::DeadlineExceeded { call: "MachineProbe::facts" }),
        "a 300 ms read against a 200 ms deadline"
    );
    assert_eq!(clock.mono().0, 200_000_000, "the caller waited its whole budget and no more");
    let in_time = probe.facts(Deadline::after(Duration::from_secs(1))).unwrap();
    assert_eq!(in_time.cpu, FixtureMachine::owners_shape().facts.cpu, "in time, the facts");
    assert_eq!(clock.mono().0, 500_000_000, "plus the read's own 300 ms");
}

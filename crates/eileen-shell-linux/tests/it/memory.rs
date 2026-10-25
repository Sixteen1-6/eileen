//! The memory sample over the laptop fixture, and once over the machine the tests run on, by
//! explicit times and through `SystemMemory` at a manual clock's.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use eileen_contracts::ids::Personal;
use eileen_contracts::jobs::MemoryPressure;
use eileen_platform::{PlatformError, SystemMemory};
use eileen_shell_linux::{LinuxSystemMemory, MIN_RATE_INTERVAL_MS, ReadError, SysRoot};
use eileen_stdx::clock::{Clock, ManualClock};

use crate::tree;

/// The fixture's Eileen process: the root of the tree the largest process is never taken from.
const EILEEN: u32 = 5_000;

/// A clock that stays at monotonic zero, for the tests that pass their times to `sample_at`.
fn at_zero() -> Arc<dyn Clock> {
    Arc::new(ManualClock::default())
}

#[test]
fn the_trait_samples_at_the_clocks_time() {
    let root = tree::build("intel-hybrid-laptop", "memory_trait");
    let clock = Arc::new(ManualClock::default());
    let memory =
        LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, Arc::<ManualClock>::clone(&clock))
            .unwrap();
    fs::write(root.join("proc/vmstat"), "pgmajfault 48463\n").unwrap();
    let half = Duration::from_millis(MIN_RATE_INTERVAL_MS / 2);
    clock.advance(half);
    let soon = SystemMemory::sample(&memory).unwrap();
    assert_eq!(soon.paging_kb_s, 0, "half a second after opening, by the clock: no rate yet");
    clock.advance(Duration::from_secs(5).checked_sub(half).unwrap());
    let sample = SystemMemory::sample(&memory).unwrap();
    assert_eq!(sample.paging_kb_s, 200, "250 faults in the clock's 5 s since opening");
    assert_eq!(sample.available_mb, 9_631, "the fixture's figures, as sample_at reads them");
}

#[test]
fn the_trait_answers_a_file_it_cannot_use_as_an_io_error_naming_it() {
    let root = tree::build("intel-hybrid-laptop", "memory_trait_errors");
    let memory = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap();
    fs::write(root.join("proc/meminfo"), "MemTotal: 1 kB\n").unwrap();
    assert_eq!(
        SystemMemory::sample(&memory),
        Err(PlatformError::Io { what: "/proc/meminfo", detail: "no MemAvailable line".to_owned() }),
        "a missing line"
    );
    fs::remove_file(root.join("proc/meminfo")).unwrap();
    assert_eq!(
        SystemMemory::sample(&memory),
        Err(PlatformError::Io { what: "/proc/meminfo", detail: "entity not found".to_owned() }),
        "an unreadable file, by the error's kind alone"
    );
}

#[test]
fn a_sample_reads_available_memory_headroom_paging_and_the_largest_other_process() {
    let root = tree::build("intel-hybrid-laptop", "memory_sample");
    let memory = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap();
    // 250 major faults in the 5 s since the sampler opened.
    fs::write(root.join("proc/vmstat"), "pgpgin 812345\npgmajfault 48463\n").unwrap();
    let sample = memory.sample_at(5_000).unwrap();
    let expected = MemoryPressure {
        available_mb: 9_631,
        commit_headroom_mb: 13_727,
        paging_kb_s: 200,
        paging_limit_kb_s: 0,
        largest_process: Some((Personal::new("firefox".to_owned()), 1_224)),
    };
    assert_eq!(sample, expected, "node (1.9 GB) is under Eileen, so the browser is the largest");
}

#[test]
fn the_paging_rate_never_covers_less_than_a_second() {
    let root = tree::build("intel-hybrid-laptop", "memory_rate");
    let memory = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap();
    assert_eq!(memory.sample_at(10).unwrap().paging_kb_s, 0, "straight after opening");
    fs::write(root.join("proc/vmstat"), "pgmajfault 48463\n").unwrap();
    assert_eq!(memory.sample_at(5_000).unwrap().paging_kb_s, 200, "250 faults in 5 s");
    fs::write(root.join("proc/vmstat"), "pgmajfault 99999\n").unwrap();
    let soon = MIN_RATE_INTERVAL_MS / 2;
    assert_eq!(memory.sample_at(5_000 + soon).unwrap().paging_kb_s, 200, "too soon: repeated");
    fs::write(root.join("proc/vmstat"), "pgmajfault 48463\n").unwrap();
    assert_eq!(memory.sample_at(10_000).unwrap().paging_kb_s, 0, "no faults since 5 s");
}

#[test]
fn strict_overcommit_makes_the_commit_limit_binding() {
    let root = tree::build("intel-hybrid-laptop", "memory_strict");
    fs::write(root.join("proc/sys/vm/overcommit_memory"), "2\n").unwrap();
    let memory = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap();
    assert_eq!(
        memory.sample_at(0).unwrap().commit_headroom_mb,
        1_023,
        "CommitLimit - Committed_AS"
    );
}

#[test]
fn a_kernel_without_the_counters_is_an_error() {
    let root = tree::build("intel-hybrid-laptop", "memory_errors");
    fs::write(root.join("proc/vmstat"), "pgfault 1\n").unwrap();
    let err = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap_err();
    assert!(matches!(err, ReadError::Missing { field: "pgmajfault", .. }), "{err}");
    fs::write(root.join("proc/vmstat"), "pgmajfault 1\n").unwrap();
    let memory = LinuxSystemMemory::open(SysRoot::at(&root), EILEEN, at_zero()).unwrap();
    fs::write(root.join("proc/meminfo"), "MemTotal: 1 kB\n").unwrap();
    let err = memory.sample_at(0).unwrap_err();
    assert!(matches!(err, ReadError::Missing { field: "MemAvailable", .. }), "{err}");
}

#[test]
fn this_machine_samples_within_the_kernels_own_figures() {
    let memory = LinuxSystemMemory::system().unwrap();
    let sample = memory.sample_at(0).unwrap();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap();
    let kb = |name: &str| -> u64 {
        meminfo
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok()
            })
            .unwrap()
    };
    let total_mb = kb("MemTotal") / 1024;
    let available_mb = kb("MemAvailable") / 1024;
    assert!(u64::from(sample.available_mb) <= total_mb, "available fits in memory");
    assert!(
        u64::from(sample.available_mb).abs_diff(available_mb) < 512,
        "the kernel's MemAvailable"
    );
    assert!(sample.largest_process.is_some(), "some process outside this one's tree");
    assert_eq!(sample.paging_limit_kb_s, 0, "the runtime's to fill");
}

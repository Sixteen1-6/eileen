//! [`LinuxSystemMemory`]: the memory sample BUILD.md section 2.5 takes every 5 s, from
//! `/proc/meminfo`, `/proc/vmstat` and `/proc/<pid>/status`.
//!
//! - Available memory is `MemAvailable`.
//! - Commit headroom follows the overcommit policy (the `memory` module).
//! - The paging rate counts major faults (`pgmajfault`), 4 KB each, over the time since the
//!   previous sample, the build guard's Linux formula, so the runtime's samples and the guard's
//!   agree in the machine-wide files they share. A major fault reads at least one page; Windows'
//!   `Pages Input/sec` counts every page the fault read, so this is a lower bound.
//! - The largest process is the one with the most resident memory outside Eileen's own tree:
//!   this process and everything it started.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use eileen_contracts::ids::Personal;
use eileen_contracts::jobs::MemoryPressure;

use crate::memory::{Overcommit, commit_headroom_kb, kb_to_mb, parse_meminfo, parse_overcommit};
use crate::{ReadError, SysRoot};

/// The shortest time a paging rate covers, ms. A sample sooner than this after the previous
/// one repeats the previous rate, so no rate covers a few milliseconds and spikes.
pub const MIN_RATE_INTERVAL_MS: u64 = 1_000;

/// What one major fault reads at least, KB.
const KB_PER_FAULT: u64 = 4;

/// The paging counter at the previous sample.
#[derive(Debug, Clone, Copy)]
struct Counter {
    /// When it was read, monotonic ms.
    at_ms: u64,
    /// `pgmajfault` then.
    majfaults: u64,
    /// The rate the previous sample reported, KB/s.
    rate_kb_s: u32,
}

/// Samples the machine's memory on Linux, as `eileen-shell-win`'s `SystemMemory` does on
/// Windows. It takes time from its caller, because the clock is a port (`clippy.toml`): the
/// `SystemMemory` implementation passes `eileen_stdx::clock`'s monotonic time once
/// `eileen-platform` merges.
#[derive(Debug)]
pub struct LinuxSystemMemory {
    root: SysRoot,
    own_pid: u32,
    counter: Mutex<Counter>,
}

impl LinuxSystemMemory {
    /// Opens the sampler at monotonic time `now_ms`, priming the paging counter so the first
    /// sample's rate covers the time since. `own_pid` roots Eileen's own process tree, which
    /// the largest process is never taken from.
    pub fn open(root: SysRoot, own_pid: u32, now_ms: u64) -> Result<Self, ReadError> {
        let majfaults = read_majfaults(&root)?;
        Ok(Self {
            root,
            own_pid,
            counter: Mutex::new(Counter { at_ms: now_ms, majfaults, rate_kb_s: 0 }),
        })
    }

    /// The real machine, with this process as the root of Eileen's tree.
    pub fn system(now_ms: u64) -> Result<Self, ReadError> {
        Self::open(SysRoot::system(), std::process::id(), now_ms)
    }

    /// One sample at monotonic time `now_ms`. `paging_limit_kb_s` is left 0: the effective
    /// limit is the runtime's to fill (BUILD.md 2.5 rule 3).
    pub fn sample_at(&self, now_ms: u64) -> Result<MemoryPressure, ReadError> {
        let text = self.root.read("proc/meminfo").map_err(ReadError::io("/proc/meminfo"))?;
        let meminfo = parse_meminfo(&text);
        let available_kb = meminfo
            .available_kb
            .ok_or(ReadError::Missing { what: "/proc/meminfo", field: "MemAvailable" })?;
        // The kernel's default, should the file be unreadable.
        let overcommit = self
            .root
            .read_trimmed("proc/sys/vm/overcommit_memory")
            .and_then(|text| parse_overcommit(&text))
            .unwrap_or(Overcommit::Heuristic);
        let headroom_kb = commit_headroom_kb(&meminfo, overcommit).ok_or(ReadError::Missing {
            what: "/proc/meminfo",
            field: "CommitLimit, Committed_AS or SwapFree",
        })?;
        let paging_kb_s = self.paging_rate(now_ms, read_majfaults(&self.root)?);
        Ok(MemoryPressure {
            available_mb: kb_to_mb(available_kb),
            commit_headroom_mb: kb_to_mb(headroom_kb),
            paging_kb_s,
            paging_limit_kb_s: 0,
            largest_process: largest_process(&self.root, self.own_pid)
                .map(|(name, rss_kb)| (Personal::new(name), kb_to_mb(rss_kb))),
        })
    }

    /// The paging rate since the previous sample, KB/s; the previous rate when that was less
    /// than [`MIN_RATE_INTERVAL_MS`] ago.
    fn paging_rate(&self, now_ms: u64, majfaults: u64) -> u32 {
        let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
        let elapsed_ms = now_ms.saturating_sub(counter.at_ms);
        if elapsed_ms < MIN_RATE_INTERVAL_MS {
            return counter.rate_kb_s;
        }
        let kb = majfaults.saturating_sub(counter.majfaults).saturating_mul(KB_PER_FAULT);
        let rate_kb_s = u32::try_from(kb.saturating_mul(1_000) / elapsed_ms).unwrap_or(u32::MAX);
        *counter = Counter { at_ms: now_ms, majfaults, rate_kb_s };
        rate_kb_s
    }
}

/// `pgmajfault` from `/proc/vmstat`.
fn read_majfaults(root: &SysRoot) -> Result<u64, ReadError> {
    let text = root.read("proc/vmstat").map_err(ReadError::io("/proc/vmstat"))?;
    parse_majfaults(&text).ok_or(ReadError::Missing { what: "/proc/vmstat", field: "pgmajfault" })
}

/// The `pgmajfault` line of `/proc/vmstat`'s text.
pub(crate) fn parse_majfaults(text: &str) -> Option<u64> {
    text.lines().find_map(|line| line.strip_prefix("pgmajfault ")?.trim().parse().ok())
}

/// The fields of `/proc/<pid>/status` the largest-process scan uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    /// `Name`: the command, cut to 15 bytes by the kernel.
    pub(crate) name: String,
    /// `PPid`.
    pub(crate) ppid: u32,
    /// `VmRSS`, kB; kernel threads have none.
    pub(crate) rss_kb: Option<u64>,
}

/// Reads a `/proc/<pid>/status` text.
pub(crate) fn parse_status(text: &str) -> Option<Status> {
    let mut name = None;
    let mut ppid = None;
    let mut rss_kb = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key {
            "Name" => name = Some(value.trim().to_owned()),
            "PPid" => ppid = value.trim().parse().ok(),
            "VmRSS" => rss_kb = value.split_whitespace().next().and_then(|kb| kb.parse().ok()),
            _ => {}
        }
    }
    Some(Status { name: name?, ppid: ppid?, rss_kb })
}

/// The process with the most resident memory outside the tree rooted at `own_pid`, with its
/// resident size in kB. A process that exits during the scan is skipped.
pub(crate) fn largest_process(root: &SysRoot, own_pid: u32) -> Option<(String, u64)> {
    let processes: BTreeMap<u32, Status> = root
        .entries("proc")
        .iter()
        .filter_map(|entry| entry.parse::<u32>().ok())
        .filter_map(|pid| {
            Some((pid, parse_status(&root.read(&format!("proc/{pid}/status")).ok()?)?))
        })
        .collect();
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (pid, status) in &processes {
        children.entry(status.ppid).or_default().push(*pid);
    }
    let mut own = BTreeSet::from([own_pid]);
    let mut next = vec![own_pid];
    while let Some(pid) = next.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if own.insert(*child) {
                next.push(*child);
            }
        }
    }
    processes
        .into_iter()
        .filter(|(pid, _)| !own.contains(pid))
        .filter_map(|(_, status)| Some((status.name, status.rss_kb?)))
        .max_by_key(|(_, rss_kb)| *rss_kb)
}

#[cfg(test)]
mod tests {
    use super::{Status, parse_majfaults, parse_status};

    #[test]
    fn vmstat_gives_the_major_fault_count() {
        let text = "pgpgin 812345\npgmajfault 48213\npgmajfault_s 0\n";
        assert_eq!(parse_majfaults(text), Some(48_213), "its own line, not a longer key");
        assert_eq!(parse_majfaults("pgfault 1\n"), None, "missing");
    }

    #[test]
    fn status_gives_the_name_parent_and_resident_size() {
        let text = "Name:\tIsolated Web Co\nUmask:\t0022\nState:\tS (sleeping)\nTgid:\t4300\n\
                    Pid:\t4300\nPPid:\t4120\nVmPeak:\t 3412000 kB\nVmRSS:\t  610000 kB\n";
        assert_eq!(
            parse_status(text),
            Some(Status { name: "Isolated Web Co".to_owned(), ppid: 4120, rss_kb: Some(610_000) }),
            "a process"
        );
        let kernel = parse_status("Name:\tkthreadd\nPid:\t2\nPPid:\t0\n").unwrap();
        assert_eq!(kernel.rss_kb, None, "a kernel thread has no resident size");
        assert_eq!(parse_status("Pid:\t9\n"), None, "no name and parent");
    }
}

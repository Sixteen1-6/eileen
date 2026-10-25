//! Reading the machine's memory: available memory, commit headroom and the paging rate.
//!
//! The figures are the ones section 2.5 reads: available physical memory (free plus standby),
//! commit headroom (the commit limit minus the commit charge) and the hard-fault paging rate.
//! On Windows the paging counter is `\Memory\Pages Input/sec`, which counts pages read by hard
//! faults (memory-mapped file reads included); [`pages_to_kb_s`] converts it with the
//! system's page size. Off Windows, `/proc/meminfo` and the major-fault count in
//! `/proc/vmstat` give the same three figures, for CI and contributors. Linux enforces its
//! commit limit only under strict overcommit, so there commit headroom follows
//! `vm.overcommit_memory` as `eileen-shell-linux` reads it (docs/LINUX.md section 10).

use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::time::Duration;

use crate::clock::{Clock as _, RealClock, millis};

/// The shortest time a real paging rate covers. [`SystemMemory`] primes its counter when it
/// opens, and a reading that comes sooner than this after the prime or the previous reading
/// first waits for the rest, so no reading's rate covers a few microseconds (a zero, a PDH
/// status mapped to zero, or a spike) and lands in the shared samples.
pub const MIN_RATE_INTERVAL: Duration = Duration::from_secs(1);

/// One reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySample {
    /// Available physical memory, MB.
    pub avail_mb: u32,
    /// Commit headroom, MB.
    pub headroom_mb: u32,
    /// The paging rate since the previous reading, KB/s.
    pub paging_kb_s: u32,
}

/// Where readings come from.
pub trait MemorySource {
    /// One reading. The paging rate covers the time since the previous reading, or for the
    /// first one since the source opened, never less than [`MIN_RATE_INTERVAL`] for a real
    /// source.
    fn sample(&mut self) -> io::Result<MemorySample>;

    /// True for the PC's real memory. Only a real source writes the machine-wide files.
    fn is_real(&self) -> bool {
        false
    }
}

/// Converts a paging counter in pages per second to KB/s with the system's page size.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "NaN and the range are checked first, and the value is rounded, so the cast is exact"
)]
pub fn pages_to_kb_s(pages_per_sec: f64, page_size_bytes: u32) -> u32 {
    let kb_per_page = f64::from(page_size_bytes) / 1024.0;
    let kb_s = (pages_per_sec * kb_per_page).round();
    if kb_s.is_nan() || kb_s <= 0.0 {
        0
    } else if kb_s >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        kb_s as u32
    }
}

/// Bytes to whole MB, saturating.
#[cfg(windows)]
fn mb(bytes: u64) -> u32 {
    u32::try_from(bytes / (1024 * 1024)).unwrap_or(u32::MAX)
}

/// Readings a test scripts. The last one repeats once the queue runs dry.
#[derive(Debug, Default)]
pub struct FakeMemory {
    queue: VecDeque<MemorySample>,
    last: Option<MemorySample>,
}

impl FakeMemory {
    /// Readings that start with `samples`.
    #[must_use]
    pub fn new(samples: impl IntoIterator<Item = MemorySample>) -> Self {
        Self { queue: samples.into_iter().collect(), last: None }
    }

    /// One reading, repeated for ever.
    #[must_use]
    pub fn steady(avail_mb: u32, headroom_mb: u32, paging_kb_s: u32) -> Self {
        Self::new([MemorySample { avail_mb, headroom_mb, paging_kb_s }])
    }

    /// Queues more readings.
    pub fn push(&mut self, sample: MemorySample) {
        self.queue.push_back(sample);
    }
}

impl MemorySource for FakeMemory {
    fn sample(&mut self) -> io::Result<MemorySample> {
        if let Some(next) = self.queue.pop_front() {
            self.last = Some(next);
        }
        self.last.ok_or_else(|| io::Error::other("no fake memory reading queued"))
    }
}

/// The PC's real memory.
///
/// Not `Send` on any OS: on Windows it holds a PDH query handle, and the marker keeps Linux
/// to the same rule, so code that moves one to another thread fails on the Linux lane as it
/// would on Windows.
#[derive(Debug)]
pub struct SystemMemory {
    #[cfg(windows)]
    paging: crate::ffi::PagingCounter,
    #[cfg(windows)]
    page_size: u32,
    /// The major-fault count at the previous collect.
    #[cfg(not(windows))]
    majfaults: u64,
    /// When the counter was last collected (primed or read), monotonic ms.
    collected_ms: u64,
    /// How long the last reading's paging rate covered.
    last_interval: Duration,
    _thread_bound: PhantomData<*const ()>,
}

impl SystemMemory {
    /// Opens the counters and primes the paging rate, so the first reading's rate covers the
    /// time since this call, at least [`MIN_RATE_INTERVAL`].
    pub fn new() -> io::Result<Self> {
        #[cfg(windows)]
        let this = Self {
            paging: crate::ffi::PagingCounter::open()?,
            page_size: crate::ffi::page_size(),
            collected_ms: RealClock.mono_ms(),
            last_interval: Duration::ZERO,
            _thread_bound: PhantomData,
        };
        #[cfg(not(windows))]
        let this = Self {
            majfaults: read_majfaults()?,
            collected_ms: RealClock.mono_ms(),
            last_interval: Duration::ZERO,
            _thread_bound: PhantomData,
        };
        Ok(this)
    }

    /// How long the last reading's paging rate covered; zero before the first reading.
    #[must_use]
    pub const fn last_interval(&self) -> Duration {
        self.last_interval
    }

    /// Waits until at least [`MIN_RATE_INTERVAL`] has passed since the previous collect, then
    /// marks now as collected and records how long the coming rate covers.
    fn settle(&mut self) {
        let clock = RealClock;
        let since = clock.mono_ms().saturating_sub(self.collected_ms);
        let min = millis(MIN_RATE_INTERVAL);
        if since < min {
            clock.sleep(Duration::from_millis(min - since));
        }
        let now = clock.mono_ms();
        self.last_interval = Duration::from_millis(now.saturating_sub(self.collected_ms));
        self.collected_ms = now;
    }
}

#[cfg(windows)]
impl MemorySource for SystemMemory {
    fn sample(&mut self) -> io::Result<MemorySample> {
        self.settle();
        let avail = crate::ffi::available_physical_bytes()?;
        let headroom = crate::ffi::commit_headroom_bytes()?;
        let pages = self.paging.pages_per_sec()?;
        Ok(MemorySample {
            avail_mb: mb(avail),
            headroom_mb: mb(headroom),
            paging_kb_s: pages_to_kb_s(pages, self.page_size),
        })
    }

    fn is_real(&self) -> bool {
        true
    }
}

/// The major-fault count from `/proc/vmstat`.
#[cfg(not(windows))]
fn read_majfaults() -> io::Result<u64> {
    let vmstat = std::fs::read_to_string("/proc/vmstat")?;
    vmstat
        .lines()
        .find_map(|line| line.strip_prefix("pgmajfault ")?.trim().parse().ok())
        .ok_or_else(|| io::Error::other("pgmajfault missing from /proc/vmstat"))
}

#[cfg(not(windows))]
impl MemorySource for SystemMemory {
    fn sample(&mut self) -> io::Result<MemorySample> {
        self.settle();
        let interval_ms = millis(self.last_interval).max(1);
        let meminfo = std::fs::read_to_string("/proc/meminfo")?;
        let missing = |what: &str| io::Error::other(format!("{what} missing from /proc/meminfo"));
        let avail_kb =
            meminfo_kb(&meminfo, "MemAvailable").ok_or_else(|| missing("MemAvailable"))?;
        // Unreadable, it counts as the kernel's default policy.
        let overcommit = std::fs::read_to_string("/proc/sys/vm/overcommit_memory").ok();
        let headroom_kb = linux_headroom_kb(&meminfo, overcommit.as_deref())
            .ok_or_else(|| missing("CommitLimit, Committed_AS or SwapFree"))?;
        let majfaults = read_majfaults()?;
        let pages = majfaults.saturating_sub(self.majfaults);
        self.majfaults = majfaults;
        // Each major fault reads at least one 4 KB page.
        let paging_kb_s = u32::try_from(pages.saturating_mul(4).saturating_mul(1000) / interval_ms)
            .unwrap_or(u32::MAX);
        Ok(MemorySample {
            avail_mb: u32::try_from(avail_kb / 1024).unwrap_or(u32::MAX),
            headroom_mb: u32::try_from(headroom_kb / 1024).unwrap_or(u32::MAX),
            paging_kb_s,
        })
    }

    fn is_real(&self) -> bool {
        true
    }
}

/// One `/proc/meminfo` field, KB.
#[cfg(any(test, not(windows)))]
fn meminfo_kb(meminfo: &str, name: &str) -> Option<u64> {
    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?.strip_prefix(':')?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

/// Commit headroom on Linux, KB, as `vm.overcommit_memory` (`overcommit`, the file's text)
/// defines it: `CommitLimit` minus `Committed_AS` under strict overcommit (2), the only mode in
/// which the kernel enforces that limit; otherwise `MemAvailable` plus `SwapFree`, the memory
/// that can still be backed before the OOM killer acts. In the default mode `Committed_AS`
/// passes `CommitLimit` on a busy desktop while gigabytes are free, and the first form would
/// refuse every build. An unreadable or unknown mode counts as the default. This is
/// `eileen-shell-linux`'s rule, so the guard's samples and the runtime's agree in the files they
/// share. `None` when a field it needs is missing.
#[cfg(any(test, not(windows)))]
fn linux_headroom_kb(meminfo: &str, overcommit: Option<&str>) -> Option<u64> {
    let kb = |name: &str| meminfo_kb(meminfo, name);
    if overcommit.map(str::trim) == Some("2") {
        Some(kb("CommitLimit")?.saturating_sub(kb("Committed_AS")?))
    } else {
        Some(kb("MemAvailable")?.saturating_add(kb("SwapFree")?))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FakeMemory, MIN_RATE_INTERVAL, MemorySample, MemorySource, SystemMemory, linux_headroom_kb,
        pages_to_kb_s,
    };

    /// A busy Linux desktop in the default mode: `Committed_AS` past `CommitLimit` while
    /// 13.7 GB is available, the shape docs/LINUX.md section 10 describes.
    const MEMINFO: &str = "MemTotal:       16337548 kB\nMemFree:        13000000 kB\n\
                           MemAvailable:   14404764 kB\nSwapTotal:       4194304 kB\n\
                           SwapFree:        4194304 kB\nCommitLimit:    12363076 kB\n\
                           Committed_AS:   14883076 kB\n";

    #[test]
    fn linux_headroom_follows_the_overcommit_policy() {
        assert_eq!(
            linux_headroom_kb(MEMINFO, Some("2\n")),
            Some(0),
            "strict: the limit the kernel enforces, as on Windows"
        );
        assert_eq!(
            linux_headroom_kb(MEMINFO, Some("0\n")),
            Some(14_404_764 + 4_194_304),
            "the default heuristic: what can still be backed, not the unenforced limit"
        );
        let default = linux_headroom_kb(MEMINFO, Some("0"));
        assert_eq!(linux_headroom_kb(MEMINFO, Some("1")), default, "mode 1 enforces nothing");
        assert_eq!(linux_headroom_kb(MEMINFO, None), default, "unreadable: the default");
        assert_eq!(linux_headroom_kb(MEMINFO, Some("7")), default, "unknown: the default");
        assert_eq!(
            linux_headroom_kb("MemAvailable: 1 kB\n", Some("0")),
            None,
            "a missing line is not a zero"
        );
    }

    #[test]
    fn pages_convert_with_the_page_size() {
        assert_eq!(pages_to_kb_s(1_000.0, 4096), 4_000, "4 KB pages: times four");
        assert_eq!(pages_to_kb_s(1_000.0, 16_384), 16_000, "16 KB pages");
        assert_eq!(pages_to_kb_s(-1.0, 4096), 0, "negative");
        assert_eq!(pages_to_kb_s(f64::NAN, 4096), 0, "not a number");
        assert_eq!(pages_to_kb_s(1e12, 4096), u32::MAX, "saturates");
    }

    #[test]
    fn a_fake_repeats_its_last_reading() {
        let mut fake =
            FakeMemory::new([MemorySample { avail_mb: 1, headroom_mb: 2, paging_kb_s: 3 }]);
        assert_eq!(fake.sample().unwrap().avail_mb, 1, "first");
        assert_eq!(fake.sample().unwrap().avail_mb, 1, "repeated");
        assert!(!fake.is_real(), "a fake is not real memory");
        assert!(FakeMemory::default().sample().is_err(), "nothing queued");
    }

    #[test]
    fn the_real_source_reads_plausible_figures() {
        let mut memory = SystemMemory::new().unwrap();
        let first = memory.sample().unwrap();
        assert!(first.avail_mb > 0, "some memory is available");
        assert!(memory.is_real(), "real");
    }

    #[test]
    fn the_first_reading_after_new_covers_the_minimum_interval() {
        // Called straight after opening, as xtask's acquire does: the rate must not cover the
        // few microseconds since the prime.
        let mut memory = SystemMemory::new().unwrap();
        memory.sample().unwrap();
        assert!(
            memory.last_interval() >= MIN_RATE_INTERVAL,
            "the first rate covered {:?}",
            memory.last_interval()
        );
    }
}

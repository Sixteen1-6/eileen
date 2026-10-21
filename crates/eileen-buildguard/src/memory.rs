//! Reading the machine's memory: available memory, commit headroom and the paging rate.
//!
//! The figures are the ones section 2.5 reads: available physical memory (free plus standby),
//! commit headroom (the commit limit minus the commit charge) and the hard-fault paging rate.
//! On Windows the paging counter is `\Memory\Pages Input/sec`, which counts pages read by hard
//! faults (memory-mapped file reads included); [`pages_to_kb_s`] converts it with the
//! system's page size. Off Windows, `/proc/meminfo` and the major-fault count in
//! `/proc/vmstat` give the same three figures, for CI and contributors.

use std::collections::VecDeque;
use std::io;

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
    /// One reading. The paging rate covers the time since the previous call (zero on the
    /// first).
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
pub(crate) fn mb(bytes: u64) -> u32 {
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
#[derive(Debug)]
pub struct SystemMemory {
    #[cfg(windows)]
    paging: crate::ffi::PagingCounter,
    #[cfg(windows)]
    page_size: u32,
    #[cfg(not(windows))]
    faults: Option<(u64, u64)>,
}

impl SystemMemory {
    /// Opens the counters. On Windows the paging counter is primed here, so the first
    /// reading's rate covers the time since this call.
    pub fn new() -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                paging: crate::ffi::PagingCounter::open()?,
                page_size: crate::ffi::page_size(),
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self { faults: None })
        }
    }
}

#[cfg(windows)]
impl MemorySource for SystemMemory {
    fn sample(&mut self) -> io::Result<MemorySample> {
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

#[cfg(not(windows))]
impl MemorySource for SystemMemory {
    fn sample(&mut self) -> io::Result<MemorySample> {
        use crate::clock::{Clock, RealClock};

        let meminfo = std::fs::read_to_string("/proc/meminfo")?;
        let field = |name: &str| -> Option<u64> {
            meminfo.lines().find_map(|line| {
                let rest = line.strip_prefix(name)?.strip_prefix(':')?;
                rest.split_whitespace().next()?.parse().ok()
            })
        };
        let kb =
            |name: &str| field(name).ok_or_else(|| io::Error::other(format!("{name} missing")));
        let avail_kb = kb("MemAvailable")?;
        let headroom_kb = kb("CommitLimit")?.saturating_sub(kb("Committed_AS")?);
        let vmstat = std::fs::read_to_string("/proc/vmstat")?;
        let majfault: u64 = vmstat
            .lines()
            .find_map(|line| line.strip_prefix("pgmajfault ")?.trim().parse().ok())
            .unwrap_or(0);
        let now = RealClock.mono_ms();
        let paging_kb_s = match self.faults.replace((majfault, now)) {
            Some((before, then)) if now > then => {
                let pages = majfault.saturating_sub(before);
                // Each major fault reads at least one 4 KB page.
                u32::try_from(pages.saturating_mul(4).saturating_mul(1000) / (now - then))
                    .unwrap_or(u32::MAX)
            }
            Some(_) | None => 0,
        };
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

#[cfg(test)]
mod tests {
    use super::{FakeMemory, MemorySample, MemorySource, SystemMemory, pages_to_kb_s};

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
}

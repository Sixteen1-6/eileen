//! The CPU's busy share from `/proc/stat`, for `MachineEvent::Load` (BUILD.md 2.16: every 30 s,
//! with the power state). Windows reads `GetSystemTimes`; `/proc/stat`'s first line holds the
//! same cumulative times, in clock ticks, so the share is a ratio of differences and needs no
//! clock of its own.

/// The whole machine's CPU time, in clock ticks since boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CpuTimes {
    /// Ticks spent on anything but idle and waiting for I/O.
    pub(crate) busy: u64,
    /// All ticks.
    pub(crate) total: u64,
}

/// Reads the `cpu` line of `/proc/stat`'s text: user, nice, system, idle, iowait, irq,
/// softirq and steal. The guest columns that follow are already inside user and nice, so they
/// are not added again.
pub(crate) fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|line| line.split_whitespace().next() == Some("cpu"))?;
    let ticks: Vec<u64> =
        line.split_whitespace().skip(1).take(8).map(str::parse).collect::<Result<_, _>>().ok()?;
    let &[user, nice, system, idle, iowait, irq, softirq, steal] = ticks.as_slice() else {
        return None;
    };
    let total = [user, nice, system, idle, iowait, irq, softirq, steal]
        .iter()
        .fold(0_u64, |sum, ticks| sum.saturating_add(*ticks));
    Some(CpuTimes { busy: total.saturating_sub(idle.saturating_add(iowait)), total })
}

/// The busy share between two readings, in percent, rounded; `None` when no tick passed.
pub(crate) fn busy_pct(before: CpuTimes, now: CpuTimes) -> Option<u8> {
    let total = now.total.checked_sub(before.total).filter(|ticks| *ticks > 0)?;
    let busy = now.busy.saturating_sub(before.busy).min(total);
    u8::try_from(busy.saturating_mul(100).saturating_add(total / 2) / total).ok()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{CpuTimes, busy_pct, parse_proc_stat};

    #[test]
    fn the_cpu_line_gives_busy_and_total_ticks() {
        let text = "cpu  4705 356 584 3699 23 0 23 0 100 0\ncpu0 1393 280 307 2191 9 0 21 0 0 0\n\
                    intr 114930548\n";
        let times = parse_proc_stat(text).unwrap();
        assert_eq!(times.total, 4705 + 356 + 584 + 3699 + 23 + 23, "eight columns, no guest");
        assert_eq!(times.busy, 4705 + 356 + 584 + 23, "everything but idle and iowait");
        assert_eq!(parse_proc_stat("cpu0 1 2 3 4 5 6 7 8\n"), None, "a single CPU's line only");
        assert_eq!(parse_proc_stat("cpu 1 2 3\n"), None, "too few columns");
    }

    #[test]
    fn the_busy_share_is_a_ratio_of_differences() {
        let before = CpuTimes { busy: 1_000, total: 4_000 };
        assert_eq!(busy_pct(before, CpuTimes { busy: 1_250, total: 5_000 }), Some(25), "a quarter");
        assert_eq!(busy_pct(before, CpuTimes { busy: 2_000, total: 5_000 }), Some(100), "all busy");
        assert_eq!(busy_pct(before, before), None, "no tick passed");
        assert_eq!(
            busy_pct(before, CpuTimes { busy: 900, total: 3_000 }),
            None,
            "counters went back"
        );
        assert_eq!(busy_pct(before, CpuTimes { busy: 1_002, total: 4_003 }), Some(67), "rounded");
    }

    /// A reading: small counters, as soon after boot, or any at all.
    fn reading() -> impl Strategy<Value = CpuTimes> {
        prop_oneof![(0..10_000_u64, 0..10_000_u64), any::<(u64, u64)>()]
            .prop_map(|(busy, total)| CpuTimes { busy, total })
    }

    proptest! {
        #[test]
        fn the_busy_share_is_at_most_100(before in reading(), now in reading()) {
            let pct = busy_pct(before, now);
            prop_assert!(pct.is_none_or(|pct| pct <= 100), "{:?}", pct);
        }
    }
}

//! `/proc/meminfo`, and commit headroom as Linux's overcommit policy defines it.
//!
//! Windows enforces its commit limit: past it, allocations fail, so "commit limit minus commit
//! charge" is how much more programs may commit. Linux enforces `CommitLimit` only under strict
//! overcommit (`vm.overcommit_memory = 2`). In the default heuristic mode (0) and in mode 1 it
//! is a statistic: `Committed_AS` can pass it on a busy desktop, because browsers and runtimes
//! reserve far more than they touch, and what bounds programs is the memory that can still be
//! backed, `MemAvailable` plus `SwapFree`. Reading `CommitLimit - Committed_AS` there can
//! report no headroom on a PC with gigabytes free.

/// The `/proc/meminfo` fields Eileen reads, in kB as the kernel writes them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_field_names,
    reason = "the unit ends every name, as it does across the workspace's memory figures"
)]
pub(crate) struct Meminfo {
    /// `MemTotal`.
    pub(crate) total_kb: Option<u64>,
    /// `MemAvailable`: memory available for new work without swapping (free memory plus the
    /// page cache that can be dropped), the counterpart of Windows' available memory.
    pub(crate) available_kb: Option<u64>,
    /// `SwapFree`.
    pub(crate) swap_free_kb: Option<u64>,
    /// `CommitLimit`: swap plus the overcommit ratio's share of memory.
    pub(crate) commit_limit_kb: Option<u64>,
    /// `Committed_AS`: the memory all processes have committed.
    pub(crate) committed_kb: Option<u64>,
}

/// Reads `/proc/meminfo`'s text.
pub(crate) fn parse_meminfo(text: &str) -> Meminfo {
    let field = |name: &str| -> Option<u64> {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    Meminfo {
        total_kb: field("MemTotal"),
        available_kb: field("MemAvailable"),
        swap_free_kb: field("SwapFree"),
        commit_limit_kb: field("CommitLimit"),
        committed_kb: field("Committed_AS"),
    }
}

/// `vm.overcommit_memory`: whether the kernel enforces `CommitLimit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Overcommit {
    /// 0, the default: a heuristic refuses only absurd single requests.
    Heuristic,
    /// 1: every request succeeds.
    Always,
    /// 2: requests past `CommitLimit` fail, as on Windows.
    Strict,
}

/// Reads `/proc/sys/vm/overcommit_memory`'s text; `None` when it is not 0, 1 or 2.
pub(crate) fn parse_overcommit(text: &str) -> Option<Overcommit> {
    match text.trim() {
        "0" => Some(Overcommit::Heuristic),
        "1" => Some(Overcommit::Always),
        "2" => Some(Overcommit::Strict),
        _ => None,
    }
}

/// How much more memory programs may commit before allocations fail or the OOM killer acts,
/// kB: `CommitLimit - Committed_AS` under strict overcommit, else `MemAvailable + SwapFree`.
/// `None` when a field it needs is missing.
pub(crate) fn commit_headroom_kb(meminfo: &Meminfo, overcommit: Overcommit) -> Option<u64> {
    match overcommit {
        Overcommit::Strict => Some(meminfo.commit_limit_kb?.saturating_sub(meminfo.committed_kb?)),
        Overcommit::Heuristic | Overcommit::Always => {
            Some(meminfo.available_kb?.saturating_add(meminfo.swap_free_kb?))
        }
    }
}

/// kB to whole MB, saturating.
pub(crate) fn kb_to_mb(kb: u64) -> u32 {
    u32::try_from(kb / 1024).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{Overcommit, commit_headroom_kb, kb_to_mb, parse_meminfo, parse_overcommit};

    const MEMINFO: &str = "MemTotal:       16337548 kB\nMemFree:        13000000 kB\n\
                           MemAvailable:   14404764 kB\nSwapTotal:       4194304 kB\n\
                           SwapFree:        4194304 kB\nCommitLimit:    12363076 kB\n\
                           Committed_AS:   14883076 kB\n";

    #[test]
    fn fields_come_from_their_own_lines() {
        let meminfo = parse_meminfo(MEMINFO);
        assert_eq!(meminfo.total_kb, Some(16_337_548), "MemTotal");
        assert_eq!(meminfo.available_kb, Some(14_404_764), "MemAvailable, not MemFree");
        assert_eq!(meminfo.swap_free_kb, Some(4_194_304), "SwapFree, not SwapTotal");
        assert_eq!(meminfo.commit_limit_kb, Some(12_363_076), "CommitLimit");
        assert_eq!(meminfo.committed_kb, Some(14_883_076), "Committed_AS");
        assert_eq!(kb_to_mb(16_337_548), 15_954, "MB, rounded down");
    }

    #[test]
    fn headroom_follows_the_overcommit_policy() {
        let meminfo = parse_meminfo(MEMINFO);
        assert_eq!(
            commit_headroom_kb(&meminfo, Overcommit::Strict),
            Some(0),
            "strict: Committed_AS past CommitLimit leaves nothing, as on Windows"
        );
        assert_eq!(
            commit_headroom_kb(&meminfo, Overcommit::Heuristic),
            Some(14_404_764 + 4_194_304),
            "the default: what can still be backed"
        );
        assert_eq!(
            commit_headroom_kb(&meminfo, Overcommit::Always),
            commit_headroom_kb(&meminfo, Overcommit::Heuristic),
            "mode 1 enforces nothing either"
        );
    }

    #[test]
    fn missing_lines_are_none_not_zero() {
        let meminfo = parse_meminfo("MemFree: 1 kB\n");
        assert_eq!(meminfo.total_kb, None, "no MemTotal");
        assert_eq!(commit_headroom_kb(&meminfo, Overcommit::Strict), None, "no CommitLimit");
        assert_eq!(commit_headroom_kb(&meminfo, Overcommit::Heuristic), None, "no MemAvailable");
    }

    #[test]
    fn overcommit_modes_parse_and_huge_values_saturate() {
        assert_eq!(parse_overcommit("0\n"), Some(Overcommit::Heuristic), "0");
        assert_eq!(parse_overcommit("2"), Some(Overcommit::Strict), "2");
        assert_eq!(parse_overcommit("7"), None, "unknown");
        assert_eq!(kb_to_mb(u64::MAX), u32::MAX, "saturates");
    }
}

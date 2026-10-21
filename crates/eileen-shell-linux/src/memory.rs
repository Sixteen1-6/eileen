//! Memory totals from `/proc/meminfo`. The figures that change (available memory, commit
//! headroom, paging) are the memory sampler's, which the build guard already reads on Linux.

/// The totals `/proc/meminfo` gives, in kB as the kernel writes them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MemTotals {
    /// `MemTotal`.
    pub(crate) total_kb: Option<u64>,
    /// `CommitLimit`: swap plus the overcommit ratio's share of memory.
    pub(crate) commit_limit_kb: Option<u64>,
}

/// Reads `MemTotal` and `CommitLimit` from `/proc/meminfo`'s text.
pub(crate) fn parse_meminfo(text: &str) -> MemTotals {
    let field = |name: &str| -> Option<u64> {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse().ok()
        })
    };
    MemTotals { total_kb: field("MemTotal"), commit_limit_kb: field("CommitLimit") }
}

/// kB to whole MB, saturating.
pub(crate) fn kb_to_mb(kb: u64) -> u32 {
    u32::try_from(kb / 1024).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{kb_to_mb, parse_meminfo};

    #[test]
    fn totals_come_from_their_own_lines() {
        let text = "MemTotal:       16337548 kB\nMemFree:        13000000 kB\n\
                    MemAvailable:   14404764 kB\nCommitLimit:    12363076 kB\n\
                    Committed_AS:    2466752 kB\n";
        let totals = parse_meminfo(text);
        assert_eq!(totals.total_kb, Some(16_337_548), "MemTotal");
        assert_eq!(totals.commit_limit_kb, Some(12_363_076), "CommitLimit, not Committed_AS");
        assert_eq!(kb_to_mb(16_337_548), 15_954, "MB, rounded down");
    }

    #[test]
    fn a_missing_line_is_none_not_zero() {
        let totals = parse_meminfo("MemFree: 1 kB\n");
        assert_eq!(totals.total_kb, None, "no MemTotal");
        assert_eq!(totals.commit_limit_kb, None, "no CommitLimit");
    }

    #[test]
    fn huge_values_saturate() {
        assert_eq!(kb_to_mb(u64::MAX), u32::MAX, "saturates");
    }
}

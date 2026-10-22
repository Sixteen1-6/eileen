//! The processor: its name, vendor and features from `/proc/cpuinfo`, its online cores and
//! their sibling threads from `/sys/devices/system/cpu`, and those cores grouped into
//! efficiency classes.
//!
//! The classes come from the first of three sources that answers:
//!
//! 1. Intel's hybrid PMU lists (`cpu_core` for the performance cores, `cpu_atom` and
//!    `cpu_lowpower` for the others), which give the same two classes Windows reports (the
//!    owner's Core Ultra 7 155H: 6 performance cores and 10 others).
//! 2. Distinct `cpu_capacity` values (Arm big.LITTLE and any kernel that publishes them).
//! 3. One class with every core.

use std::collections::{BTreeMap, BTreeSet};

use eileen_contracts::ids::Untrusted;

use crate::SysRoot;
use crate::facts::{CoreClass, CpuFacts, CpuFeature, CpuVendor};

/// The most CPUs a list may name: a list that names a CPU numbered this or higher is malformed.
const MAX_CPUS: u32 = 8192;

/// What `/proc/cpuinfo` says: the first value of each key it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CpuInfo {
    /// `model name`, or on some Arm kernels `Hardware`.
    pub(crate) name: Option<String>,
    /// From `vendor_id`, or Arm when `CPU implementer` is present.
    pub(crate) vendor: CpuVendor,
    /// From `flags` (x86) or `Features` (Arm).
    pub(crate) features: BTreeSet<CpuFeature>,
    /// How many `processor` entries it lists.
    pub(crate) processors: u32,
}

/// Reads `/proc/cpuinfo` in its x86 or Arm layout. Only the keys above are read: never
/// `Serial`, which some Arm boards print.
pub(crate) fn parse_cpuinfo(text: &str) -> CpuInfo {
    let mut values: BTreeMap<&str, &str> = BTreeMap::new();
    let mut processors = 0_u32;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key == "processor" {
            processors = processors.saturating_add(1);
        }
        if matches!(key, "model name" | "Hardware" | "vendor_id" | "CPU implementer")
            || matches!(key, "flags" | "Features")
        {
            values.entry(key).or_insert_with(|| value.trim());
        }
    }
    let vendor = match values.get("vendor_id").copied() {
        Some("GenuineIntel") => CpuVendor::Intel,
        Some("AuthenticAMD") => CpuVendor::Amd,
        None if values.contains_key("CPU implementer") => CpuVendor::Arm,
        _ => CpuVendor::Other,
    };
    let name = values
        .get("model name")
        .or_else(|| values.get("Hardware"))
        .map(|name| (*name).to_owned())
        .filter(|name| !name.is_empty());
    let mut features = BTreeSet::new();
    let flags = values.get("flags").into_iter().chain(values.get("Features"));
    for token in flags.flat_map(|line| line.split_whitespace()) {
        let feature = match token {
            "avx2" => CpuFeature::Avx2,
            "avx512f" => CpuFeature::Avx512,
            "avx_vnni" | "avx512_vnni" => CpuFeature::AvxVnni,
            "asimd" => CpuFeature::Neon,
            "asimddp" => CpuFeature::DotProd,
            _ => continue,
        };
        features.insert(feature);
    }
    CpuInfo { name, vendor, features, processors }
}

/// Parses a kernel CPU list such as `0-3,8,10-11` (as in `online` and the PMUs' `cpus`).
/// `None` when it is malformed or names a CPU numbered [`MAX_CPUS`] or higher.
pub(crate) fn parse_cpu_list(text: &str) -> Option<BTreeSet<u32>> {
    let mut cpus = BTreeSet::new();
    let text = text.trim();
    if text.is_empty() {
        return Some(cpus);
    }
    for part in text.split(',') {
        let (first, last): (u32, u32) = if let Some((first, last)) = part.split_once('-') {
            (first.trim().parse().ok()?, last.trim().parse().ok()?)
        } else {
            let only = part.trim().parse().ok()?;
            (only, only)
        };
        if first > last || last >= MAX_CPUS {
            return None;
        }
        cpus.extend(first..=last);
    }
    Some(cpus)
}

/// A count as the facts store it.
fn count(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// Reads the processor. `cpuinfo` is the text of `/proc/cpuinfo`; everything else comes from
/// `root`, and what cannot be read is named in `degraded`.
pub(crate) fn read_cpu(root: &SysRoot, cpuinfo: &str, degraded: &mut Vec<String>) -> CpuFacts {
    let info = parse_cpuinfo(cpuinfo);
    let name = info.name.or_else(|| soc_name(root)).unwrap_or_else(|| {
        degraded.push("cpu name: /proc/cpuinfo and the SoC files name no processor".to_owned());
        "unknown processor".to_owned()
    });
    let online = root
        .read_trimmed("sys/devices/system/cpu/online")
        .and_then(|text| parse_cpu_list(&text))
        .filter(|cpus| !cpus.is_empty())
        .unwrap_or_else(|| {
            degraded.push(
                "cpu online list: unreadable; counting /proc/cpuinfo's processors".to_owned(),
            );
            (0..info.processors.max(1)).collect()
        });
    let cores = core_siblings(root, &online);
    if cores.is_none() {
        degraded.push("cpu topology: unreadable; physical cores counted as logical".to_owned());
    }
    let physical_of = |cpus: &BTreeSet<u32>| -> u16 {
        cores.as_ref().map_or_else(
            || count(cpus.len()),
            |cores| {
                let distinct: BTreeSet<&BTreeSet<u32>> =
                    cpus.iter().filter_map(|cpu| cores.get(cpu)).collect();
                count(distinct.len())
            },
        )
    };
    let classes = class_groups(root, &online)
        .into_iter()
        .filter_map(|(class, cpus)| {
            let cpus: BTreeSet<u32> = cpus.intersection(&online).copied().collect();
            (!cpus.is_empty()).then(|| CoreClass {
                efficiency_class: class,
                physical: physical_of(&cpus),
                logical: count(cpus.len()),
            })
        })
        .rev()
        .collect();
    CpuFacts {
        name: Untrusted::new(name),
        vendor: info.vendor,
        logical: count(online.len()),
        physical: physical_of(&online),
        classes,
        features: info.features,
    }
}

/// An Arm system-on-chip's name, for kernels whose `/proc/cpuinfo` names no processor: the
/// family and machine its driver reports (`Snapdragon X1E80100`), or the last, least specific
/// entry of the device tree's `compatible` list (`rockchip,rk3588`). Never the chip's serial
/// number or id.
fn soc_name(root: &SysRoot) -> Option<String> {
    if let Some(machine) = root.read_trimmed("sys/devices/soc0/machine").filter(|m| !m.is_empty()) {
        return Some(match root.read_trimmed("sys/devices/soc0/family") {
            Some(family) if !family.is_empty() => format!("{family} {machine}"),
            _ => machine,
        });
    }
    let compatible = root.read("sys/firmware/devicetree/base/compatible").ok()?;
    compatible.split('\0').map(str::trim).rfind(|entry| !entry.is_empty()).map(str::to_owned)
}

/// Each online CPU's core, as the set of CPUs that share it (`core_cpus_list`, or its older
/// name `thread_siblings_list`); `None` when any is unreadable. Counting distinct sets counts
/// physical cores on every architecture, where `core_id` restarts in each Arm cluster.
fn core_siblings(root: &SysRoot, online: &BTreeSet<u32>) -> Option<BTreeMap<u32, BTreeSet<u32>>> {
    online
        .iter()
        .map(|cpu| {
            let dir = format!("sys/devices/system/cpu/cpu{cpu}/topology");
            let text = root
                .read_trimmed(&format!("{dir}/core_cpus_list"))
                .or_else(|| root.read_trimmed(&format!("{dir}/thread_siblings_list")))?;
            Some((*cpu, parse_cpu_list(&text)?))
        })
        .collect()
}

/// The CPUs of each class, in ascending class order (the slowest first).
fn class_groups(root: &SysRoot, online: &BTreeSet<u32>) -> Vec<(u8, BTreeSet<u32>)> {
    let pmu = |name: &str| -> Option<BTreeSet<u32>> {
        parse_cpu_list(&root.read_trimmed(&format!("sys/bus/event_source/devices/{name}/cpus"))?)
    };
    if let (Some(performance), Some(mut others)) = (pmu("cpu_core"), pmu("cpu_atom")) {
        others.extend(pmu("cpu_lowpower").unwrap_or_default());
        return vec![(0, others), (1, performance)];
    }
    let capacities: Option<BTreeMap<u32, u32>> = online
        .iter()
        .map(|cpu| {
            let text =
                root.read_trimmed(&format!("sys/devices/system/cpu/cpu{cpu}/cpu_capacity"))?;
            Some((*cpu, text.parse().ok()?))
        })
        .collect();
    if let Some(capacities) = capacities {
        let distinct: BTreeSet<u32> = capacities.values().copied().collect();
        if distinct.len() > 1 {
            return distinct
                .iter()
                .enumerate()
                .map(|(class, capacity)| {
                    let cpus = capacities
                        .iter()
                        .filter(|(_, c)| *c == capacity)
                        .map(|(cpu, _)| *cpu)
                        .collect();
                    (u8::try_from(class).unwrap_or(u8::MAX), cpus)
                })
                .collect();
        }
    }
    vec![(0, online.clone())]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::{CpuFeature, CpuVendor, MAX_CPUS, parse_cpu_list, parse_cpuinfo};

    #[test]
    fn cpu_lists_take_ranges_singles_and_spaces() {
        assert_eq!(parse_cpu_list("0-3,8,10-11").unwrap().len(), 7, "seven CPUs");
        assert_eq!(
            parse_cpu_list(" 0 - 1 , 5 ").unwrap().into_iter().collect::<Vec<_>>(),
            [0, 1, 5]
        );
        assert!(parse_cpu_list("").unwrap().is_empty(), "empty is no CPUs, not an error");
    }

    #[test]
    fn malformed_or_huge_cpu_lists_are_refused() {
        assert!(parse_cpu_list("3-1").is_none(), "backwards range");
        assert!(parse_cpu_list("a-b").is_none(), "not numbers");
        assert!(parse_cpu_list("0-4294967295").is_none(), "too many CPUs");
        assert!(parse_cpu_list("0,,1").is_none(), "empty part");
    }

    #[test]
    fn a_cpu_list_may_name_cpus_up_to_one_below_the_bound() {
        let last = MAX_CPUS - 1;
        assert_eq!(parse_cpu_list(&last.to_string()), Some(BTreeSet::from([last])), "the last");
        assert_eq!(
            parse_cpu_list(&format!("{}-{last}", last - 1)),
            Some(BTreeSet::from([last - 1, last])),
            "a range up to it"
        );
        assert!(parse_cpu_list(&MAX_CPUS.to_string()).is_none(), "the bound itself");
        assert!(parse_cpu_list(&format!("0-{MAX_CPUS}")).is_none(), "a range that reaches it");
    }

    /// `cpus` as the kernel writes a CPU list: each run as a range, `0-3,8,10-11`.
    fn cpu_list_text(cpus: &BTreeSet<u32>) -> String {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for &cpu in cpus {
            match runs.last_mut() {
                Some((_, last)) if *last + 1 == cpu => *last = cpu,
                _ => runs.push((cpu, cpu)),
            }
        }
        let run = |&(first, last): &(u32, u32)| {
            if first == last { first.to_string() } else { format!("{first}-{last}") }
        };
        runs.iter().map(run).collect::<Vec<_>>().join(",")
    }

    /// A CPU a list may name, often one of the last few below the bound.
    fn named_cpu() -> impl Strategy<Value = u32> {
        prop_oneof![0..MAX_CPUS, MAX_CPUS - 4..MAX_CPUS]
    }

    /// Any CPU number, often one beside the bound, where an off-by-one would show.
    fn cpu_number() -> impl Strategy<Value = u32> {
        prop_oneof![0..2 * MAX_CPUS, MAX_CPUS - 2..=MAX_CPUS + 1]
    }

    proptest! {
        #[test]
        fn a_written_cpu_list_parses_back(cpus in prop::collection::btree_set(named_cpu(), 0..48)) {
            prop_assert_eq!(parse_cpu_list(&cpu_list_text(&cpus)), Some(cpus.clone()));
            let singles: Vec<String> = cpus.iter().map(u32::to_string).collect();
            prop_assert_eq!(parse_cpu_list(&singles.join(",")), Some(cpus), "one number each");
        }

        #[test]
        fn a_cpu_list_never_names_a_cpu_past_the_bound(
            text in "[0-9 ,-]{0,24}",
            first in cpu_number(),
            last in cpu_number(),
        ) {
            for text in [text, format!("{first}-{last}"), format!("0,{first},{last}")] {
                let cpus = parse_cpu_list(&text).unwrap_or_default();
                prop_assert!(cpus.iter().all(|cpu| *cpu < MAX_CPUS), "{}", text);
            }
        }
    }

    #[test]
    fn x86_cpuinfo_gives_the_name_vendor_and_features() {
        let text = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Core(TM) Ultra 7 155H\n\
                    flags\t\t: fpu sse2 avx2 avx_vnni fma\n\nprocessor\t: 1\nvendor_id\t: GenuineIntel\n";
        let info = parse_cpuinfo(text);
        assert_eq!(info.vendor, CpuVendor::Intel, "vendor");
        assert_eq!(info.name.as_deref(), Some("Intel(R) Core(TM) Ultra 7 155H"), "name");
        assert_eq!(info.processors, 2, "two processors");
        assert!(info.features.contains(&CpuFeature::Avx2), "avx2");
        assert!(info.features.contains(&CpuFeature::AvxVnni), "avx_vnni");
        assert!(!info.features.contains(&CpuFeature::Avx512), "no avx512");
    }

    #[test]
    fn avx512_vnni_counts_as_vnni_and_avx512f_as_avx512() {
        let info = parse_cpuinfo("vendor_id : AuthenticAMD\nflags : avx2 avx512f avx512_vnni\n");
        assert_eq!(info.vendor, CpuVendor::Amd, "vendor");
        let expected = [CpuFeature::Avx2, CpuFeature::Avx512, CpuFeature::AvxVnni];
        assert_eq!(info.features.into_iter().collect::<Vec<_>>(), expected, "features");
    }

    #[test]
    fn arm_cpuinfo_gives_neon_and_dot_product_and_never_reads_the_serial() {
        let text = "processor\t: 0\nBogoMIPS\t: 48.00\nFeatures\t: fp asimd evtstrm asimddp\n\
                    CPU implementer\t: 0x41\nCPU part\t: 0xd0b\n\nHardware\t: Example board\n\
                    Serial\t\t: 10000000abcdef01\n";
        let info = parse_cpuinfo(text);
        assert_eq!(info.vendor, CpuVendor::Arm, "vendor");
        assert_eq!(info.name.as_deref(), Some("Example board"), "the Hardware line");
        let expected = [CpuFeature::Neon, CpuFeature::DotProd];
        assert_eq!(info.features.into_iter().collect::<Vec<_>>(), expected, "features");
        assert!(!format!("{:?}", parse_cpuinfo(text)).contains("abcdef01"), "no serial kept");
    }
}

//! The probe over each fixture machine: every fact, and which facts it names as degraded.

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

use eileen_contracts::connectors::Platform;
use eileen_contracts::ids::Untrusted;
use eileen_shell_linux::facts::{
    AdapterFacts, AdapterKind, CoreClass, CpuFacts, CpuFeature, CpuVendor, DiskFacts, DiskMedia,
    MachineFacts, PowerState,
};
use eileen_shell_linux::{LinuxMachineProbe, ReadError, SysRoot, Volume};

use crate::tree;

/// The data folder every fixture measures: under a home folder, as on a real install.
const DATA: &str = "/home/sam/.local/share/eileen";

fn volume(at: &str, major: u32, minor: u32) -> Volume {
    Volume { at: PathBuf::from(at), major, minor, free_mb: 200_000, total_mb: 950_000 }
}

fn facts(fixture: &str, test: &str, volume: Volume) -> MachineFacts {
    let root = tree::build(fixture, test);
    LinuxMachineProbe::new(SysRoot::at(root), "/unused").facts_on(Ok(volume)).unwrap()
}

/// What each degraded entry is about: its text before the first ": ".
fn degraded(facts: &MachineFacts) -> Vec<&str> {
    facts
        .degraded
        .iter()
        .map(|note| note.split_once(": ").map_or(note.as_str(), |(what, _)| what))
        .collect()
}

fn adapter(
    name: &str,
    kind: AdapterKind,
    dedicated_mb: u32,
    shared_mb: u32,
    driver: &str,
) -> AdapterFacts {
    AdapterFacts {
        name: Untrusted::new(name.to_owned()),
        kind,
        dedicated_mb,
        shared_mb,
        driver: driver.to_owned(),
    }
}

fn class(efficiency_class: u8, physical: u16, logical: u16) -> CoreClass {
    CoreClass { efficiency_class, physical, logical }
}

fn disk(media: DiskMedia) -> DiskFacts {
    DiskFacts { data_free_mb: 200_000, data_total_mb: 950_000, media }
}

#[test]
fn wsl_reads_the_cpu_and_memory_and_names_what_the_host_keeps() {
    let facts = facts("wsl-ryzen-5600x", "wsl", volume(DATA, 8, 48));
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: "ubuntu 24.04, kernel 6.18.33.2-microsoft-standard-WSL2".to_owned(),
        memory_total_mb: 15_954,
        commit_limit_mb: 12_073,
        cpu: CpuFacts {
            name: Untrusted::new("AMD Ryzen 5 5600X 6-Core Processor".to_owned()),
            vendor: CpuVendor::Amd,
            logical: 12,
            physical: 6,
            classes: vec![class(0, 6, 12)],
            features: BTreeSet::from([CpuFeature::Avx2]),
        },
        adapters: vec![],
        disk: disk(DiskMedia::Unknown),
        power: PowerState::default(),
        monitors: 0,
        degraded: facts.degraded.clone(),
    };
    assert_eq!(facts, expected, "the facts");
    assert_eq!(degraded(&facts), ["gpu", "disk media", "power", "monitors"], "what WSL hides");
    assert_eq!(facts.cpu.performance_cores(), 6, "one class: every core");
}

#[test]
fn the_owners_laptop_shape_reads_six_performance_cores_its_npu_and_battery_saver() {
    let facts = facts("intel-hybrid-laptop", "intel_hybrid", volume(DATA, 259, 2));
    let kernel = "kernel 7.0.0-14-generic";
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: format!("ubuntu 26.04, {kernel}"),
        memory_total_mb: 15_737,
        commit_limit_mb: 11_964,
        cpu: CpuFacts {
            name: Untrusted::new("Intel(R) Core(TM) Ultra 7 155H".to_owned()),
            vendor: CpuVendor::Intel,
            logical: 22,
            physical: 16,
            classes: vec![class(1, 6, 12), class(0, 10, 10)],
            features: BTreeSet::from([CpuFeature::Avx2, CpuFeature::AvxVnni]),
        },
        adapters: vec![
            adapter(
                "Intel Corporation Meteor Lake-P [Intel Arc Graphics]",
                AdapterKind::IntegratedGpu,
                0,
                0,
                &format!("i915, {kernel}"),
            ),
            adapter(
                "Intel Corporation Meteor Lake NPU",
                AdapterKind::Npu,
                0,
                0,
                &format!("intel_vpu, {kernel}"),
            ),
        ],
        disk: disk(DiskMedia::Ssd),
        power: PowerState { has_battery: true, on_battery: true, battery_saver: true },
        monitors: 1,
        degraded: facts.degraded.clone(),
    };
    assert_eq!(facts, expected, "the facts");
    assert_eq!(facts.cpu.performance_cores(), 6, "BUILD.md 2.16: the owner's shape reads 6");
    assert_eq!(degraded(&facts), ["gpu 0000:00:02.0"], "i915 reports no memory");
}

#[test]
fn an_amd_desktop_tells_its_apu_from_its_discrete_gpu_and_finds_btrfs_on_nvme() {
    let facts = facts("amd-desktop-btrfs", "amd_btrfs", volume(DATA, 0, 41));
    let driver = "amdgpu, kernel 6.17.4-300.fc43.x86_64";
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: "fedora 43, kernel 6.17.4-300.fc43.x86_64".to_owned(),
        memory_total_mb: 62_481,
        commit_limit_mb: 39_432,
        cpu: CpuFacts {
            name: Untrusted::new("AMD Ryzen 7 7700X 8-Core Processor".to_owned()),
            vendor: CpuVendor::Amd,
            logical: 16,
            physical: 8,
            classes: vec![class(0, 8, 16)],
            features: BTreeSet::from([CpuFeature::Avx2, CpuFeature::Avx512, CpuFeature::AvxVnni]),
        },
        adapters: vec![
            adapter(
                "Advanced Micro Devices, Inc. [AMD/ATI] Raphael",
                AdapterKind::IntegratedGpu,
                0,
                31_241,
                driver,
            ),
            adapter(
                "Advanced Micro Devices, Inc. [AMD/ATI] Navi 31 [Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M]",
                AdapterKind::DiscreteGpu,
                24_560,
                31_241,
                driver,
            ),
        ],
        disk: disk(DiskMedia::Ssd),
        power: PowerState::default(),
        monitors: 2,
        degraded: vec![],
    };
    assert_eq!(facts, expected, "the facts; the mouse's battery is not the PC's");
    assert!(!format!("{facts:?}").contains("/home/sam"), "the data folder's path stays out");
}

#[test]
fn a_data_folder_on_the_hard_disk_or_on_tmpfs_is_told_apart() {
    let on_hdd = facts("amd-desktop-btrfs", "amd_hdd", volume("/srv/media/eileen", 8, 1));
    assert_eq!(on_hdd.disk.media, DiskMedia::Hdd, "ext4 on sda1");
    let on_tmpfs = facts("amd-desktop-btrfs", "amd_tmpfs", volume("/tmp/eileen", 0, 38));
    assert_eq!(on_tmpfs.disk.media, DiskMedia::Unknown, "tmpfs has no disk");
    assert_eq!(
        on_tmpfs.degraded,
        ["disk media: the data folder is on tmpfs, which has no disk of its own"],
        "named"
    );
}

#[test]
fn an_nvidia_desktop_reads_the_proprietary_driver_and_lvm_on_a_hard_disk() {
    let facts = facts("nvidia-desktop-lvm", "nvidia_lvm", volume(DATA, 252, 0));
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: "ubuntu 24.04, kernel 6.8.0-87-generic".to_owned(),
        memory_total_mb: 31_999,
        commit_limit_mb: 18_047,
        cpu: CpuFacts {
            name: Untrusted::new("Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz".to_owned()),
            vendor: CpuVendor::Intel,
            logical: 8,
            physical: 8,
            classes: vec![class(0, 8, 8)],
            features: BTreeSet::from([CpuFeature::Avx2]),
        },
        adapters: vec![
            adapter(
                "Intel Corporation CoffeeLake-S GT2 [UHD Graphics 630]",
                AdapterKind::IntegratedGpu,
                0,
                0,
                "i915, kernel 6.8.0-87-generic",
            ),
            adapter("NVIDIA GeForce RTX 4070", AdapterKind::DiscreteGpu, 0, 0, "nvidia 580.82.07"),
        ],
        disk: disk(DiskMedia::Hdd),
        power: PowerState::default(),
        monitors: 1,
        degraded: facts.degraded.clone(),
    };
    assert_eq!(facts, expected, "the facts");
    assert_eq!(
        degraded(&facts),
        ["gpu 0000:00:02.0", "gpu 0000:01:00.0"],
        "neither driver reports memory"
    );
    assert!(!format!("{facts:?}").contains("GPU-0"), "the GPU's UUID is never kept");
}

#[test]
fn an_arm_board_reads_its_big_and_little_cores_and_its_soc_gpu_and_npu() {
    let facts = facts("arm-rk3588-board", "arm", volume(DATA, 179, 2));
    let kernel = "kernel 6.18.0-1004-rockchip";
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: format!("ubuntu 24.04, {kernel}"),
        memory_total_mb: 15_742,
        commit_limit_mb: 15_742,
        cpu: CpuFacts {
            name: Untrusted::new("rockchip,rk3588".to_owned()),
            vendor: CpuVendor::Arm,
            logical: 8,
            physical: 8,
            classes: vec![class(1, 4, 4), class(0, 4, 4)],
            features: BTreeSet::from([CpuFeature::Neon, CpuFeature::DotProd]),
        },
        adapters: vec![
            adapter(
                "rockchip,rk3588-mali",
                AdapterKind::IntegratedGpu,
                0,
                0,
                &format!("panthor, {kernel}"),
            ),
            adapter(
                "rockchip,rk3588-rknn-core",
                AdapterKind::Npu,
                0,
                0,
                &format!("rocket, {kernel}"),
            ),
        ],
        disk: disk(DiskMedia::Ssd),
        power: PowerState::default(),
        monitors: 1,
        degraded: vec![],
    };
    assert_eq!(facts, expected, "the facts; the display controller is not an adapter");
    assert_eq!(facts.cpu.performance_cores(), 4, "the A76s");
}

#[test]
fn a_virtual_machine_lists_no_gpu_and_does_not_call_its_disk_a_hard_disk() {
    let facts = facts("ubuntu-vm-qemu", "vm", volume(DATA, 252, 0));
    let expected = MachineFacts {
        os: Platform::Linux,
        os_build: "ubuntu 26.04, kernel 7.0.0-14-generic".to_owned(),
        memory_total_mb: 7_939,
        commit_limit_mb: 8_065,
        cpu: CpuFacts {
            name: Untrusted::new("Intel(R) Core(TM) Ultra 7 155H".to_owned()),
            vendor: CpuVendor::Intel,
            logical: 4,
            physical: 4,
            classes: vec![class(0, 4, 4)],
            features: BTreeSet::from([CpuFeature::Avx2, CpuFeature::AvxVnni]),
        },
        adapters: vec![],
        disk: disk(DiskMedia::Unknown),
        power: PowerState::default(),
        monitors: 1,
        degraded: facts.degraded.clone(),
    };
    assert_eq!(facts, expected, "the facts");
    assert_eq!(
        degraded(&facts),
        ["disk media"],
        "QEMU's disk under LVM is named, not called rotational"
    );
}

#[test]
fn a_root_without_meminfo_is_an_error_and_an_unreadable_volume_is_degraded() {
    let empty = tree::build("ubuntu-vm-qemu", "errors");
    std::fs::remove_file(empty.join("proc/meminfo")).unwrap();
    let probe = LinuxMachineProbe::new(SysRoot::at(&empty), "/unused");
    let err = probe.facts_on(Ok(volume(DATA, 252, 0))).unwrap_err();
    assert!(matches!(err, ReadError::Io { what: "/proc/meminfo", .. }), "{err}");

    let root = tree::build("intel-hybrid-laptop", "unreadable_volume");
    let probe = LinuxMachineProbe::new(SysRoot::at(root), "/unused");
    let facts = probe.facts_on(Err(io::Error::from(io::ErrorKind::PermissionDenied))).unwrap();
    assert_eq!(
        facts.disk,
        DiskFacts { data_free_mb: 0, data_total_mb: 0, media: DiskMedia::Unknown },
        "unknown"
    );
    assert!(degraded(&facts).contains(&"disk"), "and named: {:?}", facts.degraded);
}

#[test]
fn the_load_is_the_busy_share_since_the_previous_call_with_the_power_state() {
    let root = tree::build("intel-hybrid-laptop", "load");
    let probe = LinuxMachineProbe::new(SysRoot::at(&root), "/unused");
    let on_battery_saver = PowerState { has_battery: true, on_battery: true, battery_saver: true };
    assert_eq!(probe.load().unwrap(), (60, on_battery_saver), "since boot: 5,668 of 9,390 ticks");
    std::fs::write(root.join("proc/stat"), "cpu  5705 356 784 4699 23 0 23 0 0 0\n").unwrap();
    assert_eq!(probe.load().unwrap().0, 55, "1,200 busy of 2,200 ticks since the first call");
    assert_eq!(probe.load().unwrap().0, 55, "no tick since: the previous share");
}

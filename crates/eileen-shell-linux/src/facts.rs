//! The shapes of the machine's facts, field for field as BUILD.md section 3.3.17 gives them.
//!
//! `eileen-contracts::machine` does not exist yet: WP0 phase B's `wp0/machine-contracts` pull
//! request adds it, pinned. Until then these mirror it, so the Linux readers can be built and
//! tested now; the pull request that brings the contract replaces this module with re-exports.
//! Four fields of the contract's `MachineFacts` are not the probe's to fill and are left out:
//! `read_at` and `probe_ms` (the runtime's clock), `execution_providers` (the speech process)
//! and `audio` (the audio component).

use std::collections::BTreeSet;

use eileen_contracts::connectors::Platform;
use eileen_contracts::ids::Untrusted;

/// What the Linux probe read about this PC. Never a serial number, a hardware id, the machine's
/// name, a user name or an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineFacts {
    /// Always [`Platform::Linux`] here.
    pub os: Platform,
    /// The distribution, its version and the kernel, such as
    /// `ubuntu 24.04, kernel 6.8.0-45-generic`.
    pub os_build: String,
    /// Total physical memory, MB (`MemTotal`).
    pub memory_total_mb: u32,
    /// The commit limit, MB (`CommitLimit`).
    pub commit_limit_mb: u32,
    /// The processor.
    pub cpu: CpuFacts,
    /// GPUs and NPUs that compute, not display-only or virtual adapters.
    pub adapters: Vec<AdapterFacts>,
    /// The volume that holds the data folder.
    pub disk: DiskFacts,
    /// The power source.
    pub power: PowerState,
    /// Connected displays, as the kernel's DRM connectors report them.
    pub monitors: u8,
    /// Facts that could not be read, each with the reason: unknown, never reported as zero.
    pub degraded: Vec<String>,
}

/// The processor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuFacts {
    /// The processor name string.
    pub name: Untrusted<String>,
    /// Who made it.
    pub vendor: CpuVendor,
    /// Online logical processors.
    pub logical: u16,
    /// Physical cores among them.
    pub physical: u16,
    /// Cores per efficiency class, the highest (fastest) class first.
    pub classes: Vec<CoreClass>,
    /// The instruction-set features int8 inference uses.
    pub features: BTreeSet<CpuFeature>,
}

impl CpuFacts {
    /// The physical cores of the highest class, or of all of them on a CPU with one class.
    #[must_use]
    pub fn performance_cores(&self) -> u16 {
        self.classes.first().map_or(self.physical, |class| class.physical)
    }
}

/// Who made the processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CpuVendor {
    /// Intel.
    Intel,
    /// AMD.
    Amd,
    /// An Arm design (any licensee).
    Arm,
    /// Anyone else, or unknown.
    Other,
}

/// The cores of one efficiency class. A higher class is faster, as Windows' efficiency classes
/// are; a CPU with one core type has one class, numbered 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreClass {
    /// The class: 0 is the slowest.
    pub efficiency_class: u8,
    /// Physical cores in the class.
    pub physical: u16,
    /// Logical processors in the class.
    pub logical: u16,
}

/// An instruction-set feature int8 inference uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CpuFeature {
    /// AVX2.
    Avx2,
    /// AVX-512 (the foundation, `avx512f`).
    Avx512,
    /// VNNI dot-product instructions, VEX-encoded (`avx_vnni`) or with AVX-512 (`avx512_vnni`).
    AvxVnni,
    /// Arm's Advanced SIMD (`asimd`).
    Neon,
    /// Arm's dot-product extension (`asimddp`).
    DotProd,
}

/// A GPU or an NPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterFacts {
    /// Its name: the driver's model string, or the PCI ID database's vendor and device names.
    pub name: Untrusted<String>,
    /// What kind of adapter it is.
    pub kind: AdapterKind,
    /// Dedicated memory, MB: a discrete GPU's own, or the carve-out the firmware reserves for an
    /// integrated one, as Windows reports it; 0 for an NPU, or where the driver does not say.
    pub dedicated_mb: u32,
    /// System memory it may use, MB, where the driver reports it.
    pub shared_mb: u32,
    /// The driver and its version.
    pub driver: String,
}

/// What kind of adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdapterKind {
    /// A GPU in the processor package.
    IntegratedGpu,
    /// A GPU of its own.
    DiscreteGpu,
    /// A neural processing unit.
    Npu,
}

/// The data folder's volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskFacts {
    /// Free space an unprivileged user may use, MB.
    pub data_free_mb: u64,
    /// The volume's size, MB.
    pub data_total_mb: u64,
    /// Solid-state or rotational.
    pub media: DiskMedia,
}

/// What the data folder's disk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiskMedia {
    /// Solid-state.
    Ssd,
    /// Rotational.
    Hdd,
    /// Not known: a virtual, stacked or network file system.
    Unknown,
}

/// The power source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PowerState {
    /// The PC has a system battery.
    pub has_battery: bool,
    /// It runs on that battery now.
    pub on_battery: bool,
    /// A power-saving profile is on (power-profiles-daemon's `power-saver`, or the ACPI
    /// platform profile `low-power`).
    pub battery_saver: bool,
}

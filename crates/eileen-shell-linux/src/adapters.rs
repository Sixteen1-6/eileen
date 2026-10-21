//! GPUs and NPUs from the kernel's `drm` and `accel` device classes.
//!
//! Each PCI adapter is read from its `uevent` (driver, vendor and device ids, slot). The kind:
//!
//! - NVIDIA: always discrete.
//! - Intel: integrated at the platform's fixed graphics slot, `0000:00:02.0`; discrete elsewhere.
//! - AMD: integrated when amdgpu reports at most [`APU_CARVE_OUT_MB`] of VRAM and more GTT
//!   (system memory) than VRAM, as an APU's carve-out does; discrete otherwise.
//! - A GPU built into an Arm system-on-chip (a platform device with no PCI id, such as Mali
//!   under panthor): integrated, named by its device-tree `compatible` entry.
//! - An `accel` device: an NPU.
//!
//! Display-only and virtual adapters (a server's BMC VGA, a virtual machine's display, a
//! system-on-chip's display controller) are not listed, because they compute nothing.

use std::collections::BTreeSet;

use eileen_contracts::ids::Untrusted;

use crate::SysRoot;
use crate::facts::{AdapterFacts, AdapterKind};
use crate::pci::{parse_uevent, pci_name};

/// NVIDIA's PCI vendor id.
const NVIDIA: u16 = 0x10de;
/// AMD's PCI vendor id.
const AMD: u16 = 0x1002;
/// Intel's PCI vendor id.
const INTEL: u16 = 0x8086;

/// Where Intel platforms put the integrated GPU.
const INTEL_IGPU_SLOT: &str = "0000:00:02.0";

/// An AMD GPU with at most this much VRAM, and more GTT than VRAM, is an APU's own.
pub(crate) const APU_CARVE_OUT_MB: u32 = 2048;

/// Drivers of adapters that only display: BMC VGA, virtual machines' displays, firmware
/// framebuffers, USB display adapters and the display controllers of systems-on-chip.
const DISPLAY_ONLY: [&str; 21] = [
    "ast",
    "mgag200",
    "bochs-drm",
    "bochs",
    "cirrus",
    "cirrus-qemu",
    "qxl",
    "virtio-pci",
    "virtio_gpu",
    "vmwgfx",
    "hyperv_drm",
    "simpledrm",
    "vboxvideo",
    "udl",
    "rockchip-drm",
    "vc4-drm",
    "sun4i-drm",
    "meson-drm",
    "imx-drm",
    "mediatek-drm",
    "exynos-drm",
];

/// Drivers of GPUs built into Arm systems-on-chip: platform devices with no PCI id.
const SOC_GPUS: [&str; 8] =
    ["panthor", "panfrost", "lima", "v3d", "etnaviv", "powervr", "asahi", "msm"];

/// Whether a `drm` entry is a card (`card0`), not a connector (`card0-DP-1`) or a render node.
fn is_numbered(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Bytes in a sysfs file to whole MB.
fn sysfs_mb(root: &SysRoot, path: &str) -> Option<u32> {
    let bytes: u64 = root.read_trimmed(path)?.parse().ok()?;
    Some(u32::try_from(bytes / (1024 * 1024)).unwrap_or(u32::MAX))
}

/// One PCI device's identity as read from its `uevent`.
struct Pci<'a> {
    /// The device folder, relative to the root.
    dir: &'a str,
    /// Vendor id.
    vendor: u16,
    /// Device id.
    device: u16,
    /// The PCI slot.
    slot: &'a str,
    /// The driver bound to it.
    driver: &'a str,
}

/// Reads the adapters; what cannot be read is named in `degraded`.
pub(crate) fn read_adapters(
    root: &SysRoot,
    kernel: &str,
    degraded: &mut Vec<String>,
) -> Vec<AdapterFacts> {
    let mut adapters = Vec::new();
    let mut slots = BTreeSet::new();
    for card in root.entries("sys/class/drm").iter().filter(|name| is_numbered(name, "card")) {
        let dir = format!("sys/class/drm/{card}/device");
        let Ok(text) = root.read(&format!("{dir}/uevent")) else {
            degraded.push(format!("gpu {card}: no device uevent"));
            continue;
        };
        let uevent = parse_uevent(&text);
        let driver = uevent.driver.unwrap_or_default();
        if DISPLAY_ONLY.contains(&driver.as_str()) {
            continue;
        }
        let (Some((vendor, device)), Some(slot)) = (uevent.pci_id, uevent.slot) else {
            if SOC_GPUS.contains(&driver.as_str()) {
                adapters.push(AdapterFacts {
                    name: Untrusted::new(uevent.of_compatible.unwrap_or_else(|| driver.clone())),
                    kind: AdapterKind::IntegratedGpu,
                    dedicated_mb: 0,
                    shared_mb: 0,
                    driver: format!("{driver}, kernel {kernel}"),
                });
            } else {
                degraded.push(format!(
                    "gpu {card}: {driver} is neither a PCI device nor a known SoC GPU; not listed"
                ));
            }
            continue;
        };
        if !slots.insert(slot.clone()) {
            continue;
        }
        let pci = Pci { dir: &dir, vendor, device, slot: &slot, driver: &driver };
        adapters.push(gpu(root, &pci, kernel, degraded));
    }
    for accel in root.entries("sys/class/accel").iter().filter(|name| is_numbered(name, "accel")) {
        let Ok(text) = root.read(&format!("sys/class/accel/{accel}/device/uevent")) else {
            degraded.push(format!("npu {accel}: no device uevent"));
            continue;
        };
        let uevent = parse_uevent(&text);
        let driver = uevent.driver.unwrap_or_else(|| "unknown driver".to_owned());
        let name = uevent
            .pci_id
            .and_then(|(vendor, device)| pci_name(root, vendor, device))
            .or(uevent.of_compatible)
            .unwrap_or_else(|| format!("NPU ({driver})"));
        adapters.push(AdapterFacts {
            name: Untrusted::new(name),
            kind: AdapterKind::Npu,
            dedicated_mb: 0,
            shared_mb: 0,
            driver: format!("{driver}, kernel {kernel}"),
        });
    }
    if adapters.is_empty() && root.exists("dev/dxg") {
        degraded.push(
            "gpu: under WSL the GPUs are reached through /dev/dxg (DXCore), which this probe \
             does not query"
                .to_owned(),
        );
    }
    adapters
}

/// One GPU.
fn gpu(root: &SysRoot, pci: &Pci<'_>, kernel: &str, degraded: &mut Vec<String>) -> AdapterFacts {
    let listed = || {
        pci_name(root, pci.vendor, pci.device)
            .unwrap_or_else(|| format!("PCI {:04x}:{:04x}", pci.vendor, pci.device))
    };
    let in_tree = format!("{}, kernel {kernel}", pci.driver);
    let (name, kind, dedicated_mb, shared_mb, driver) = match pci.vendor {
        NVIDIA => {
            let model = root
                .read(&format!("proc/driver/nvidia/gpus/{}/information", pci.slot))
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        Some(line.strip_prefix("Model:")?.trim().to_owned())
                            .filter(|model| !model.is_empty())
                    })
                });
            let driver = if pci.driver == "nvidia" {
                root.read("proc/driver/nvidia/version")
                    .ok()
                    .and_then(|text| nvidia_version(&text))
                    .map_or_else(|| "nvidia".to_owned(), |version| format!("nvidia {version}"))
            } else {
                in_tree
            };
            degraded.push(format!(
                "gpu {}: the {} driver does not report memory in sysfs",
                pci.slot, pci.driver
            ));
            (model.unwrap_or_else(listed), AdapterKind::DiscreteGpu, 0, 0, driver)
        }
        AMD => {
            let vram = sysfs_mb(root, &format!("{}/mem_info_vram_total", pci.dir));
            let gtt = sysfs_mb(root, &format!("{}/mem_info_gtt_total", pci.dir));
            let kind = match (vram, gtt) {
                (Some(vram), Some(gtt)) if vram <= APU_CARVE_OUT_MB && gtt > vram => {
                    AdapterKind::IntegratedGpu
                }
                (Some(_), _) => AdapterKind::DiscreteGpu,
                (None, _) => {
                    degraded.push(format!(
                        "gpu {}: no amdgpu memory figures; kind assumed discrete",
                        pci.slot
                    ));
                    AdapterKind::DiscreteGpu
                }
            };
            let dedicated = if kind == AdapterKind::IntegratedGpu { 0 } else { vram.unwrap_or(0) };
            (listed(), kind, dedicated, gtt.unwrap_or(0), in_tree)
        }
        INTEL => {
            let kind = if pci.slot == INTEL_IGPU_SLOT {
                AdapterKind::IntegratedGpu
            } else {
                AdapterKind::DiscreteGpu
            };
            degraded.push(format!(
                "gpu {}: the {} driver does not report memory in sysfs",
                pci.slot, pci.driver
            ));
            (listed(), kind, 0, 0, in_tree)
        }
        vendor => {
            degraded.push(format!(
                "gpu {}: unknown vendor {vendor:04x}; kind assumed discrete, memory unknown",
                pci.slot
            ));
            (listed(), AdapterKind::DiscreteGpu, 0, 0, in_tree)
        }
    };
    AdapterFacts { name: Untrusted::new(name), kind, dedicated_mb, shared_mb, driver }
}

/// The version in `/proc/driver/nvidia/version`'s first line, such as `580.82.07` from
/// `NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82.07  Wed Aug 27 ...`.
pub(crate) fn nvidia_version(text: &str) -> Option<String> {
    let line = text.lines().next()?;
    let (_, after) = line.split_once("Kernel Module")?;
    after.split_whitespace().next().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{is_numbered, nvidia_version};

    #[test]
    fn only_numbered_cards_count() {
        assert!(is_numbered("card0", "card"), "a card");
        assert!(is_numbered("card12", "card"), "two digits");
        assert!(!is_numbered("card0-DP-1", "card"), "a connector");
        assert!(!is_numbered("renderD128", "card"), "a render node");
        assert!(!is_numbered("card", "card"), "no number");
        assert!(is_numbered("accel0", "accel"), "an accel device");
    }

    #[test]
    fn the_nvidia_version_comes_from_the_kernel_module_line() {
        let text = "NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82.07  Wed Aug 27 20:32:11 UTC 2025\n\
                    GCC version:  gcc version 13.3.0\n";
        assert_eq!(nvidia_version(text).as_deref(), Some("580.82.07"), "version");
        assert_eq!(nvidia_version("garbage"), None, "no version");
    }
}

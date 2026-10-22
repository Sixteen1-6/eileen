//! GPUs and NPUs from the kernel's `drm` and `accel` device classes.
//!
//! Each PCI adapter is read from its `uevent` (driver, vendor and device ids, slot). The kind:
//!
//! - NVIDIA: always discrete. NVIDIA's own driver also lists its GPUs under
//!   `/proc/driver/nvidia/gpus`, which finds them when `nvidia-drm` has made no DRM card.
//! - Intel: integrated at the platform's fixed graphics slot, `0000:00:02.0`; discrete elsewhere.
//! - AMD: integrated only when the processor is AMD's own. Then, when the processor's name
//!   mentions Radeon (`w/ Radeon 780M Graphics`, `Radeon 8060S`), the AMD GPU with the least
//!   VRAM is the integrated one, however large its carve-out; otherwise one whose VRAM is a
//!   carve-out of at most [`APU_CARVE_OUT_MB`] with more GTT (system memory) than VRAM, as on
//!   desktop Ryzen 7000 and 9000 parts, whose names do not mention their small Radeon.
//! - A GPU built into an Arm system-on-chip (a platform device with no PCI id, such as Mali
//!   under panthor): integrated, named by its device-tree `compatible` entry.
//! - An `accel` device: an NPU.
//!
//! An integrated GPU's dedicated memory is the carve-out the firmware reserves for it, as
//! Windows reports it. Display-only and virtual adapters (a server's BMC VGA, a virtual
//! machine's display, a firmware framebuffer, a system-on-chip's display controller) are not
//! listed, because they compute nothing.
//!
//! # Traps
//!
//! - An AMD GPU's VRAM size alone does not tell an APU: a Strix Halo's carve-out can be 96 GB
//!   and a small discrete card has 2 GB. The processor decides first.
//! - `simpledrm` registers as the platform driver `simple-framebuffer`: a firmware framebuffer,
//!   not a GPU.
//! - Under WSL the GPU is `/dev/dxg` with no DRM card, which is degraded, never "no GPU".

use std::collections::BTreeSet;

use eileen_contracts::ids::Untrusted;

use crate::SysRoot;
use crate::facts::{AdapterFacts, AdapterKind, CpuFacts, CpuVendor};
use crate::pci::{parse_uevent, pci_name};

/// NVIDIA's PCI vendor id.
const NVIDIA: u16 = 0x10de;
/// AMD's PCI vendor id.
const AMD: u16 = 0x1002;
/// Intel's PCI vendor id.
const INTEL: u16 = 0x8086;

/// Where Intel platforms put the integrated GPU.
const INTEL_IGPU_SLOT: &str = "0000:00:02.0";

/// An AMD GPU with at most this much VRAM, and more GTT than VRAM, is an APU's own when the
/// processor's name does not say so.
pub(crate) const APU_CARVE_OUT_MB: u32 = 2048;

/// Drivers of adapters that only display: BMC VGA, virtual machines' displays, firmware
/// framebuffers (as the platform drivers `simpledrm`, `efidrm` and `vesadrm` register), USB
/// display adapters and the display controllers of systems-on-chip.
const DISPLAY_ONLY: [&str; 24] = [
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
    "simple-framebuffer",
    "efi-framebuffer",
    "vesa-framebuffer",
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

/// A PCI GPU, found before it is classified.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Card {
    /// Vendor id.
    vendor: u16,
    /// Device id; 0 when only NVIDIA's own listing names the GPU and sysfs gives no id.
    device: u16,
    /// The PCI slot.
    slot: String,
    /// The driver bound to it.
    driver: String,
    /// amdgpu's VRAM, MB.
    vram_mb: Option<u32>,
    /// amdgpu's GTT, the system memory it may map, MB.
    gtt_mb: Option<u32>,
}

/// Reads the adapters; what cannot be read is named in `degraded`. `cpu` decides which AMD
/// GPU, if any, is the processor's own.
pub(crate) fn read_adapters(
    root: &SysRoot,
    kernel: &str,
    cpu: &CpuFacts,
    degraded: &mut Vec<String>,
) -> Vec<AdapterFacts> {
    let mut adapters = Vec::new();
    let mut cards: Vec<Card> = Vec::new();
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
        let (vram_mb, gtt_mb) = if vendor == AMD {
            (
                sysfs_mb(root, &format!("{dir}/mem_info_vram_total")),
                sysfs_mb(root, &format!("{dir}/mem_info_gtt_total")),
            )
        } else {
            (None, None)
        };
        cards.push(Card { vendor, device, slot, driver, vram_mb, gtt_mb });
    }
    cards.extend(nvidia_without_cards(root, &mut slots));
    let apu = apu_slot(&cards, cpu);
    for card in &cards {
        let integrated = apu.is_some_and(|slot| slot == card.slot);
        adapters.push(gpu(root, card, integrated, kernel, degraded));
    }
    adapters.extend(npus(root, kernel, degraded));
    if adapters.is_empty() && root.exists("dev/dxg") {
        degraded.push(
            "gpu: under WSL the GPUs are reached through /dev/dxg (DXCore), which this probe \
             does not query"
                .to_owned(),
        );
    }
    adapters
}

/// The GPUs NVIDIA's own driver lists under `/proc/driver/nvidia/gpus` that have no DRM card,
/// as when `nvidia-drm` is not loaded; each newly seen slot joins `slots`.
fn nvidia_without_cards(root: &SysRoot, slots: &mut BTreeSet<String>) -> Vec<Card> {
    root.entries("proc/driver/nvidia/gpus")
        .into_iter()
        .filter(|slot| slots.insert(slot.clone()))
        .map(|slot| {
            let device = root
                .read(&format!("sys/bus/pci/devices/{slot}/uevent"))
                .ok()
                .and_then(|text| parse_uevent(&text).pci_id)
                .map_or(0, |(_, device)| device);
            let driver = "nvidia".to_owned();
            Card { vendor: NVIDIA, device, slot, driver, vram_mb: None, gtt_mb: None }
        })
        .collect()
}

/// The NPUs of the `accel` class.
fn npus(root: &SysRoot, kernel: &str, degraded: &mut Vec<String>) -> Vec<AdapterFacts> {
    let mut adapters = Vec::new();
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
    adapters
}

/// The slot of the AMD GPU built into the processor, if there is one (the module's rules).
fn apu_slot<'c>(cards: &'c [Card], cpu: &CpuFacts) -> Option<&'c str> {
    if cpu.vendor != CpuVendor::Amd {
        return None;
    }
    let amd = || cards.iter().filter(|card| card.vendor == AMD);
    let apu = if cpu.name.get().contains("Radeon") {
        amd().filter(|card| card.vram_mb.is_some()).min_by_key(|card| card.vram_mb)
    } else {
        amd().find(|card| match (card.vram_mb, card.gtt_mb) {
            (Some(vram), Some(gtt)) => vram <= APU_CARVE_OUT_MB && gtt > vram,
            _ => false,
        })
    };
    apu.map(|card| card.slot.as_str())
}

/// One GPU, classified.
fn gpu(
    root: &SysRoot,
    card: &Card,
    integrated: bool,
    kernel: &str,
    degraded: &mut Vec<String>,
) -> AdapterFacts {
    let listed = || {
        pci_name(root, card.vendor, card.device)
            .unwrap_or_else(|| format!("PCI {:04x}:{:04x}", card.vendor, card.device))
    };
    let in_tree = format!("{}, kernel {kernel}", card.driver);
    let no_memory =
        || format!("gpu {}: the {} driver does not report memory in sysfs", card.slot, card.driver);
    let (name, kind, dedicated_mb, shared_mb, driver) = match card.vendor {
        NVIDIA => {
            let model = root
                .read(&format!("proc/driver/nvidia/gpus/{}/information", card.slot))
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        Some(line.strip_prefix("Model:")?.trim().to_owned())
                            .filter(|model| !model.is_empty())
                    })
                });
            let driver = if card.driver == "nvidia" {
                root.read("proc/driver/nvidia/version")
                    .ok()
                    .and_then(|text| nvidia_version(&text))
                    .map_or_else(|| "nvidia".to_owned(), |version| format!("nvidia {version}"))
            } else {
                in_tree
            };
            degraded.push(no_memory());
            (model.unwrap_or_else(listed), AdapterKind::DiscreteGpu, 0, 0, driver)
        }
        AMD => {
            if card.vram_mb.is_none() {
                degraded.push(format!("gpu {}: no amdgpu memory figures", card.slot));
            }
            let kind =
                if integrated { AdapterKind::IntegratedGpu } else { AdapterKind::DiscreteGpu };
            let (vram, gtt) = (card.vram_mb.unwrap_or(0), card.gtt_mb.unwrap_or(0));
            (listed(), kind, vram, gtt, in_tree)
        }
        INTEL => {
            let kind = if card.slot == INTEL_IGPU_SLOT {
                AdapterKind::IntegratedGpu
            } else {
                AdapterKind::DiscreteGpu
            };
            degraded.push(no_memory());
            (listed(), kind, 0, 0, in_tree)
        }
        vendor => {
            degraded.push(format!(
                "gpu {}: unknown vendor {vendor:04x}; kind assumed discrete, memory unknown",
                card.slot
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

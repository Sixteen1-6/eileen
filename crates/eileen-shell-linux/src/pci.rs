//! PCI devices as sysfs describes them: a device's `uevent` (its driver, vendor and device ids
//! and slot), and names from the PCI ID database (`pci.ids`).

use crate::SysRoot;

/// Where distributions keep the PCI ID database, in the order tried.
const PCI_IDS: [&str; 2] = ["usr/share/misc/pci.ids", "usr/share/hwdata/pci.ids"];

/// The fields of a device's `uevent` file that the probe uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Uevent {
    /// `DRIVER`.
    pub(crate) driver: Option<String>,
    /// `PCI_ID`: vendor and device.
    pub(crate) pci_id: Option<(u16, u16)>,
    /// `PCI_SLOT_NAME`, such as `0000:00:02.0`.
    pub(crate) slot: Option<String>,
    /// `OF_COMPATIBLE_0`: a device-tree device's most specific `compatible` entry, such as
    /// `rockchip,rk3588-mali`.
    pub(crate) of_compatible: Option<String>,
}

/// Reads a `uevent` file's `KEY=value` lines.
pub(crate) fn parse_uevent(text: &str) -> Uevent {
    let mut uevent = Uevent::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "DRIVER" => uevent.driver = Some(value.to_owned()),
            "PCI_SLOT_NAME" => uevent.slot = Some(value.to_owned()),
            "OF_COMPATIBLE_0" => uevent.of_compatible = Some(value.to_owned()),
            "PCI_ID" => {
                uevent.pci_id = value.split_once(':').and_then(|(vendor, device)| {
                    Some((
                        u16::from_str_radix(vendor, 16).ok()?,
                        u16::from_str_radix(device, 16).ok()?,
                    ))
                });
            }
            _ => {}
        }
    }
    uevent
}

/// The vendor and device names for `vendor:device`, as "vendor device", from the first PCI ID
/// database found under `root`.
pub(crate) fn pci_name(root: &SysRoot, vendor: u16, device: u16) -> Option<String> {
    PCI_IDS.iter().find_map(|path| lookup(&root.read(path).ok()?, vendor, device))
}

/// Looks `vendor:device` up in the text of a `pci.ids` file. Vendor lines start with four hex
/// digits, their devices with one tab, subsystems with two; the class section (`C ...`) at the
/// end is not read.
fn lookup(db: &str, vendor: u16, device: u16) -> Option<String> {
    let vendor_hex = format!("{vendor:04x}");
    let device_hex = format!("{device:04x}");
    let mut vendor_name: Option<&str> = None;
    for line in db.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if line.starts_with("C ") {
            break;
        }
        if let Some(rest) = line.strip_prefix('\t') {
            if rest.starts_with('\t') {
                continue;
            }
            if let (Some(name), Some(device_name)) = (vendor_name, rest.strip_prefix(&device_hex)) {
                return Some(format!("{name} {}", device_name.trim()));
            }
            continue;
        }
        if vendor_name.is_some() {
            return None;
        }
        if let Some(name) = line.strip_prefix(&vendor_hex) {
            vendor_name = Some(name.trim());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Uevent, lookup, parse_uevent};

    const DB: &str = "# A comment\n10de  NVIDIA Corporation\n\t2504  GA106 [GeForce RTX 3060 Lite Hash Rate]\n\
                      \t\t1462 397d  A subsystem\n\t2786  AD104 [GeForce RTX 4070]\n\
                      8086  Intel Corporation\n\t7d55  Meteor Lake-P [Intel Arc Graphics]\n\
                      C 03  Display controller\n\t00  VGA compatible controller\n";

    #[test]
    fn a_device_is_named_with_its_vendor() {
        assert_eq!(
            lookup(DB, 0x10de, 0x2504).as_deref(),
            Some("NVIDIA Corporation GA106 [GeForce RTX 3060 Lite Hash Rate]"),
            "found under its vendor"
        );
        assert_eq!(
            lookup(DB, 0x8086, 0x7d55).as_deref(),
            Some("Intel Corporation Meteor Lake-P [Intel Arc Graphics]"),
            "second vendor"
        );
    }

    #[test]
    fn devices_never_match_across_vendors_or_in_subsystems() {
        assert_eq!(lookup(DB, 0x8086, 0x2504), None, "the id belongs to another vendor");
        assert_eq!(lookup(DB, 0x10de, 0x397d), None, "a subsystem line is not a device");
        assert_eq!(lookup(DB, 0x1002, 0x744c), None, "unknown vendor");
    }

    #[test]
    fn uevent_lines_give_the_driver_ids_and_slot() {
        let text = "DRIVER=amdgpu\nPCI_CLASS=30000\nPCI_ID=1002:744C\nPCI_SUBSYS_ID=1002:0E3B\n\
                    PCI_SLOT_NAME=0000:03:00.0\nMODALIAS=pci:v00001002d0000744C\n";
        assert_eq!(
            parse_uevent(text),
            Uevent {
                driver: Some("amdgpu".to_owned()),
                pci_id: Some((0x1002, 0x744c)),
                slot: Some("0000:03:00.0".to_owned()),
                of_compatible: None,
            },
            "fields"
        );
        assert_eq!(parse_uevent("PCI_ID=zzzz:1\n").pci_id, None, "bad hex is no id");
        let soc = parse_uevent(
            "DRIVER=panthor\nOF_NAME=gpu\nOF_COMPATIBLE_0=rockchip,rk3588-mali\n\
                                OF_COMPATIBLE_1=arm,mali-valhall-csf\n",
        );
        assert_eq!(soc.of_compatible.as_deref(), Some("rockchip,rk3588-mali"), "the first entry");
        assert_eq!(soc.pci_id, None, "a platform device has no PCI id");
    }
}

//! The data folder's volume: free and total space from `statvfs`, and whether its disk is
//! solid-state from the block device's `queue/rotational` (the answer Windows' seek-penalty
//! query gives).
//!
//! The block device is found by the volume's device number in `/sys/block`: a whole disk
//! (`sys/block/nvme0n1/dev`), one of its partitions (`sys/block/nvme0n1/nvme0n1p2/dev`), or a
//! stacked device (`dm-0` for LVM and LUKS, `md0` for software RAID), whose `rotational` the
//! kernel derives from the disks beneath it. A file system without a device number of its own
//! (a btrfs subvolume, whose number matches no device and not even its own line in
//! `mountinfo`) is found through the mount that holds the folder and the device that mount
//! names.
//!
//! # Traps
//!
//! - Virtual disks call themselves rotational, or solid-state for a network volume. A guest is
//!   known by WSL's kernel release, the CPU's `hypervisor` flag or Xen's `/sys/hypervisor`,
//!   and, where none says so (an Arm guest), by virtio and Xen disk names and models such as
//!   `QEMU HARDDISK` and `Virtual Disk`. Its disk media is `Unknown` and degraded, never `Hdd`.
//! - A btrfs subvolume's device number matches no block device and not even its own line in
//!   `mountinfo`: the data folder's disk is found through the mount that holds the folder.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::Personal;

use crate::SysRoot;
use crate::facts::DiskMedia;
use crate::os::Guest;

/// Bytes in a MB.
const MB: u64 = 1024 * 1024;

/// How deep a stack of devices (LUKS on LVM on RAID) is followed to its disks.
const MAX_STACK: usize = 4;

/// Words in a disk's model or vendor string that mark a virtual machine's disk.
const VIRTUAL_MODELS: [&str; 4] = ["virtual", "qemu", "vbox", "vmware"];

/// The volume a folder is on, as `stat` and `statvfs` give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// The folder measured: the data folder, or its nearest existing parent before setup has
    /// created it, with symbolic links resolved. It is under the user's home folder, so it is
    /// personal.
    pub at: Personal<PathBuf>,
    /// The device number's major half (0 for a file system without a block device of its own).
    pub major: u32,
    /// Its minor half.
    pub minor: u32,
    /// Free space an unprivileged user may use, MB.
    pub free_mb: u64,
    /// The volume's size, MB.
    pub total_mb: u64,
}

/// Measures the volume `dir` is on. A folder that does not exist yet is measured at its nearest
/// existing parent, so the facts can be read before setup creates the data folder; a relative
/// `dir` is taken from the current folder.
pub fn read_volume(dir: &Path) -> io::Result<Volume> {
    let dir = std::path::absolute(dir)?;
    let mut at = dir.as_path();
    let meta = loop {
        match std::fs::metadata(at) {
            Ok(meta) => break meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => match at.parent() {
                Some(parent) => at = parent,
                None => return Err(err),
            },
            Err(err) => return Err(err),
        }
    };
    let stat = rustix::fs::statvfs(at)?;
    let unit = if stat.f_frsize > 0 { stat.f_frsize } else { stat.f_bsize };
    let mb = |blocks: u64| blocks.saturating_mul(unit) / MB;
    Ok(Volume {
        at: Personal::new(std::fs::canonicalize(at).unwrap_or_else(|_| at.to_path_buf())),
        major: rustix::fs::major(meta.dev()),
        minor: rustix::fs::minor(meta.dev()),
        free_mb: mb(stat.f_bavail),
        total_mb: mb(stat.f_blocks),
    })
}

/// A block device in `/sys/block`: a disk, or a partition of one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    /// The disk's entry in `/sys/block`, such as `nvme0n1` or `dm-0`.
    disk: String,
    /// The device itself: the disk, or its partition such as `nvme0n1p2`.
    name: String,
}

/// One mount from `/proc/self/mountinfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mount {
    /// Where it is mounted, with the kernel's octal escapes resolved.
    pub(crate) point: String,
    /// The file system type, such as `ext4`, `btrfs` or `fuse.sshfs`, as the kernel writes it,
    /// escapes and all (`fuse.my\040fs`): a FUSE subtype is the user's choice, and kept escaped
    /// it holds no space, tab or newline when a `degraded` message names it.
    pub(crate) fstype: String,
    /// What was mounted, such as `/dev/nvme0n1p2`, `/dev/mapper/root` or `none`, with the
    /// kernel's octal escapes resolved.
    pub(crate) source: String,
}

/// Reads `/proc/self/mountinfo`: `id parent major:minor root point options [optional...] -
/// fstype source super-options` per line.
pub(crate) fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let point = fields.nth(4)?;
            let mut rest = fields.skip_while(|field| *field != "-").skip(1);
            Some(Mount {
                point: unescape(point),
                fstype: rest.next()?.to_owned(),
                source: unescape(rest.next()?),
            })
        })
        .collect()
}

/// Resolves the kernel's octal escapes in mount fields (`\040` for a space, `\134` for a
/// backslash): a backslash and three octal digits, up to `\377`. Any other backslash is kept.
pub(crate) fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(at) = rest.find('\\') {
        let (before, after) = rest.split_at(at);
        out.push_str(before);
        // `from_str_radix` alone would also take a sign: `\+12` is not an escape.
        let code = after
            .get(1..4)
            .filter(|digits| digits.bytes().all(|digit| matches!(digit, b'0'..=b'7')))
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        if let Some(code) = code {
            out.push(char::from(code));
            rest = after.get(4..).unwrap_or_default();
        } else {
            out.push('\\');
            rest = after.get(1..).unwrap_or_default();
        }
    }
    out.push_str(rest);
    out
}

/// The mount that holds `at`: the longest mount point that contains it, the last one listed
/// when several are mounted at the same point.
fn mount_of<'m>(mounts: &'m [Mount], at: &Path) -> Option<&'m Mount> {
    mounts
        .iter()
        .filter(|mount| at.starts_with(&mount.point))
        .max_by_key(|mount| Path::new(&mount.point).components().count())
}

/// Whether the data folder's disk is solid-state; why it is not known is named in `degraded`.
pub(crate) fn read_media(
    root: &SysRoot,
    volume: &Volume,
    guest: Guest,
    degraded: &mut Vec<String>,
) -> DiskMedia {
    const VM_DISK: &str =
        "the data folder is on a virtual machine's disk; the host's disk is unknown";
    let unknown = |degraded: &mut Vec<String>, reason: &str| {
        degraded.push(format!("disk media: {reason}"));
        DiskMedia::Unknown
    };
    match guest {
        Guest::Wsl => {
            return unknown(
                degraded,
                "under WSL the data folder is on a virtual disk; the host's disk is unknown",
            );
        }
        Guest::VirtualMachine => return unknown(degraded, VM_DISK),
        Guest::BareMetal => {}
    }
    let block = match locate(root, volume) {
        Ok(block) => block,
        Err(reason) => return unknown(degraded, &reason),
    };
    if let Some(kind) = virtual_kind(&block.disk) {
        return unknown(degraded, &format!("the data folder is on {kind}"));
    }
    // A guest the CPU flag does not reveal (an Arm virtual machine) still shows in its disks.
    if stack_disks(root, &block.disk, 0).iter().any(|disk| is_vm_disk(root, disk)) {
        return unknown(degraded, VM_DISK);
    }
    match root.read_trimmed(&format!("sys/block/{}/queue/rotational", block.disk)).as_deref() {
        Some("0") => DiskMedia::Ssd,
        Some("1") => DiskMedia::Hdd,
        _ => unknown(degraded, &format!("{} does not say whether it rotates", block.disk)),
    }
}

/// The block device under the data folder, or why there is none.
fn locate(root: &SysRoot, volume: &Volume) -> Result<Block, String> {
    if volume.major != 0
        && let Some(block) = find_by_number(root, volume.major, volume.minor)
    {
        return Ok(block);
    }
    let not_found = || "the data folder's block device was not found".to_owned();
    let text = root.read("proc/self/mountinfo").map_err(|_| not_found())?;
    let mounts = parse_mountinfo(&text);
    let mount = mount_of(&mounts, volume.at.get()).ok_or_else(not_found)?;
    if mount.source.starts_with("/dev/") {
        find_by_source(root, &mount.source).ok_or_else(not_found)
    } else {
        Err(format!("the data folder is on {}, which has no disk of its own", mount.fstype))
    }
}

/// The disk or partition whose `dev` file holds `major:minor`.
fn find_by_number(root: &SysRoot, major: u32, minor: u32) -> Option<Block> {
    let wanted = format!("{major}:{minor}");
    let holds = |rel: &str| {
        root.read_trimmed(&format!("sys/block/{rel}/dev")).is_some_and(|dev| dev == wanted)
    };
    root.entries("sys/block").into_iter().find_map(|disk| {
        if holds(&disk) {
            return Some(Block { name: disk.clone(), disk });
        }
        partitions(root, &disk)
            .into_iter()
            .find(|part| holds(&format!("{disk}/{part}")))
            .map(|name| Block { disk, name })
    })
}

/// The partitions of `disk`: its sub-folders named after it that have a `dev` file.
fn partitions(root: &SysRoot, disk: &str) -> Vec<String> {
    root.entries(&format!("sys/block/{disk}"))
        .into_iter()
        .filter(|entry| {
            entry.starts_with(disk) && root.exists(&format!("sys/block/{disk}/{entry}/dev"))
        })
        .collect()
}

/// The disk or partition called `name` (`nvme0n1`, `sda3`).
fn find_by_name(root: &SysRoot, name: &str) -> Option<Block> {
    if name.is_empty() || name.contains('/') || name == "." || name == ".." {
        return None;
    }
    if root.exists(&format!("sys/block/{name}/dev")) {
        return Some(Block { disk: name.to_owned(), name: name.to_owned() });
    }
    root.entries("sys/block").into_iter().find_map(|disk| {
        root.exists(&format!("sys/block/{disk}/{name}/dev"))
            .then(|| Block { disk, name: name.to_owned() })
    })
}

/// The block device a mount's source names: `/dev/<name>`, or `/dev/mapper/<name>` for a
/// device-mapper device (LVM, LUKS), whose `/sys/block/dm-N/dm/name` holds that name.
fn find_by_source(root: &SysRoot, source: &str) -> Option<Block> {
    if let Some(mapped) = source.strip_prefix("/dev/mapper/") {
        return root.entries("sys/block").into_iter().find_map(|disk| {
            (disk.starts_with("dm-")
                && root.read_trimmed(&format!("sys/block/{disk}/dm/name")).as_deref()
                    == Some(mapped))
            .then(|| Block { name: disk.clone(), disk })
        });
    }
    find_by_name(root, source.strip_prefix("/dev/")?)
}

/// What a virtual block device is, for the reason in `degraded`.
fn virtual_kind(disk: &str) -> Option<&'static str> {
    if disk.starts_with("loop") {
        Some("a loop device (a disk image)")
    } else if disk.starts_with("zram") || disk.starts_with("ram") {
        Some("a RAM disk")
    } else if disk.starts_with("nbd") {
        Some("a network block device")
    } else {
        None
    }
}

/// The disks beneath `disk`: itself, or for a stacked device the disks its `slaves` hold.
fn stack_disks(root: &SysRoot, disk: &str, depth: usize) -> Vec<String> {
    let below = root.entries(&format!("sys/block/{disk}/slaves"));
    if below.is_empty() || depth >= MAX_STACK {
        return vec![disk.to_owned()];
    }
    below
        .iter()
        .filter_map(|name| find_by_name(root, name))
        .flat_map(|block| stack_disks(root, &block.disk, depth + 1))
        .collect()
}

/// Whether `disk` is a virtual machine's disk: virtio and Xen disks by name, the rest by the
/// model or vendor the hypervisor gives them (`Msft Virtual Disk`, `QEMU HARDDISK`,
/// `VBOX HARDDISK`, `VMware Virtual disk`).
fn is_vm_disk(root: &SysRoot, disk: &str) -> bool {
    if disk.starts_with("vd") || disk.starts_with("xvd") {
        return true;
    }
    ["model", "vendor"].iter().any(|file| {
        root.read_trimmed(&format!("sys/block/{disk}/device/{file}")).is_some_and(|text| {
            let text = text.to_ascii_lowercase();
            VIRTUAL_MODELS.iter().any(|word| text.contains(word))
        })
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Mount, mount_of, parse_mountinfo, unescape, virtual_kind};

    const MOUNTINFO: &str = "\
22 1 0:21 / /proc rw,nosuid shared:12 - proc proc rw
28 1 0:26 /@ / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw,ssd,space_cache=v2,subvol=/@
29 28 0:26 /@home /home rw,relatime shared:2 - btrfs /dev/nvme0n1p3 rw,ssd,subvol=/@home
40 29 0:44 / /home/sam/My\\040Disk rw - ext4 /dev/sdb1 rw
41 1 0:45 / /tmp rw - tmpfs tmpfs rw
";

    #[test]
    fn mountinfo_lines_give_the_point_type_and_source() {
        let mounts = parse_mountinfo(MOUNTINFO);
        assert_eq!(mounts.len(), 5, "every line");
        assert_eq!(
            mounts[1],
            Mount {
                point: "/".to_owned(),
                fstype: "btrfs".to_owned(),
                source: "/dev/nvme0n1p3".to_owned(),
            },
            "the optional fields before the separator are skipped"
        );
        assert_eq!(mounts[3].point, "/home/sam/My Disk", "octal escapes resolved");
    }

    #[test]
    fn the_longest_mount_point_holds_the_folder() {
        let mounts = parse_mountinfo(MOUNTINFO);
        let of = |at: &str| mount_of(&mounts, Path::new(at)).map(|mount| mount.source.as_str());
        assert_eq!(of("/home/sam/.local/share/eileen"), Some("/dev/nvme0n1p3"), "/home");
        assert_eq!(of("/home/sam/My Disk/eileen"), Some("/dev/sdb1"), "a nested mount");
        assert_eq!(of("/home/sam/My Diskette"), Some("/dev/nvme0n1p3"), "components, not text");
        assert_eq!(of("/tmp/x"), Some("tmpfs"), "tmpfs");
        assert_eq!(of("/var/lib"), Some("/dev/nvme0n1p3"), "the root");
    }

    #[test]
    fn escapes_that_are_not_octal_stay_as_written() {
        assert_eq!(unescape("C:\\134"), "C:\\", "a backslash");
        assert_eq!(unescape("a\\9zz"), "a\\9zz", "not octal");
        assert_eq!(unescape("a\\+12"), "a\\+12", "a sign is not an octal digit");
        assert_eq!(unescape("a\\400"), "a\\400", "past \\377");
        assert_eq!(unescape("end\\"), "end\\", "a trailing backslash");
    }

    #[test]
    fn the_file_system_type_keeps_the_kernels_escapes() {
        let line = "52 29 0:51 / /home/sam/remote rw - fuse.my\\040fs a\\040src rw\n";
        let mounts = parse_mountinfo(line);
        assert_eq!(mounts[0].fstype, "fuse.my\\040fs", "a FUSE subtype, as written");
        assert_eq!(mounts[0].source, "a src", "the source's escapes resolved");
    }

    #[test]
    fn loop_ram_and_network_devices_are_virtual() {
        assert!(virtual_kind("loop3").is_some(), "loop");
        assert!(virtual_kind("zram0").is_some(), "zram");
        assert!(virtual_kind("nbd0").is_some(), "nbd");
        assert!(virtual_kind("nvme0n1").is_none(), "a real disk");
        assert!(virtual_kind("sda").is_none(), "a real disk");
    }
}

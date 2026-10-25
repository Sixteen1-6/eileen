//! Disk admission: no build starts below 20 GB free.

use std::path::Path;

/// Builds refuse to start below this much free disk, GB.
pub const MIN_FREE_DISK_GB: u64 = 20;

/// Free space on the volume holding `path`, whole GB, as the calling user may use it; `None`
/// where the guard cannot read it (off Windows, where std has no call for it).
#[must_use]
pub fn free_disk_gb(path: &Path) -> Option<u64> {
    free_disk_bytes(path).map(|bytes| bytes / (1024 * 1024 * 1024))
}

/// Free bytes on the volume holding `path`, as the calling user may use them.
#[cfg(windows)]
fn free_disk_bytes(path: &Path) -> Option<u64> {
    crate::ffi::free_disk_bytes(path).ok()
}

/// Off Windows std has no call for free space, so the guard does not know it.
#[cfg(not(windows))]
const fn free_disk_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::free_disk_gb;

    #[cfg(windows)]
    #[test]
    fn the_system_volume_reports_free_space() {
        let free = free_disk_gb(&std::env::temp_dir());
        assert!(free.is_some(), "readable");
    }
}

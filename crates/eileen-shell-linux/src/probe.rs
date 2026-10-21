//! [`LinuxMachineProbe`]: every fact in one pass over `/proc`, `/sys`, `/etc`, `/usr/share` and
//! `/var/lib`, plus the data folder's volume.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use eileen_contracts::connectors::Platform;

use crate::SysRoot;
use crate::adapters::read_adapters;
use crate::cpu::read_cpu;
use crate::disk::{Volume, read_media, read_volume};
use crate::facts::{DiskFacts, DiskMedia, MachineFacts};
use crate::memory::{kb_to_mb, parse_meminfo};
use crate::monitors::read_monitors;
use crate::os::{is_wsl, read_os};
use crate::power::read_power;

/// Why no facts could be read at all: a file every Linux kernel provides is missing or
/// unreadable. Everything else that cannot be read is a `degraded` entry, not an error. It
/// becomes `PlatformError::Io` when the probe implements `MachineProbe`.
#[derive(Debug, thiserror::Error)]
pub enum FactsError {
    /// The file could not be read, or lacks the line the facts need.
    #[error("{what} is unreadable: {detail}")]
    Unreadable {
        /// The file, such as `/proc/meminfo`.
        what: &'static str,
        /// What went wrong.
        detail: String,
    },
}

/// Reads the machine's facts on Linux (BUILD.md section 2.16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxMachineProbe {
    root: SysRoot,
    data_dir: PathBuf,
}

impl LinuxMachineProbe {
    /// A probe that reads under `root` and measures the volume of `data_dir`.
    #[must_use]
    pub fn new(root: SysRoot, data_dir: impl Into<PathBuf>) -> Self {
        Self { root, data_dir: data_dir.into() }
    }

    /// A probe of the real machine, measuring the volume of `data_dir`.
    #[must_use]
    pub fn system(data_dir: impl Into<PathBuf>) -> Self {
        Self::new(SysRoot::system(), data_dir)
    }

    /// Reads every fact. The data folder's volume is measured on the real file system.
    pub fn facts(&self) -> Result<MachineFacts, FactsError> {
        self.facts_on(read_volume(&self.data_dir))
    }

    /// Reads every fact under the root, with the data folder's volume as given: a fixture tree
    /// has no volume of its own to measure.
    pub fn facts_on(&self, volume: io::Result<Volume>) -> Result<MachineFacts, FactsError> {
        let root = &self.root;
        let unreadable = |what: &'static str| {
            move |err: io::Error| FactsError::Unreadable { what, detail: err.kind().to_string() }
        };
        let meminfo = root.read("proc/meminfo").map_err(unreadable("/proc/meminfo"))?;
        let cpuinfo = root.read("proc/cpuinfo").map_err(unreadable("/proc/cpuinfo"))?;
        let totals = parse_meminfo(&meminfo);
        let total_kb = totals.total_kb.ok_or_else(|| FactsError::Unreadable {
            what: "/proc/meminfo",
            detail: "no MemTotal line".to_owned(),
        })?;
        let mut degraded = Vec::new();
        let (os_build, kernel) = read_os(root, &mut degraded);
        let wsl = is_wsl(&kernel);
        let commit_limit_mb = totals.commit_limit_kb.map_or_else(
            || {
                degraded.push("commit limit: /proc/meminfo has no CommitLimit line".to_owned());
                0
            },
            kb_to_mb,
        );
        let cpu = read_cpu(root, &cpuinfo, &mut degraded);
        let adapters = read_adapters(root, &kernel, &mut degraded);
        let disk = match volume {
            Ok(volume) => DiskFacts {
                data_free_mb: volume.free_mb,
                data_total_mb: volume.total_mb,
                media: read_media(root, &volume, wsl, &mut degraded),
            },
            Err(err) => {
                degraded
                    .push(format!("disk: the data folder's volume is unreadable ({})", err.kind()));
                DiskFacts { data_free_mb: 0, data_total_mb: 0, media: DiskMedia::Unknown }
            }
        };
        let power = read_power(root, wsl, &mut degraded);
        let monitors = read_monitors(root, wsl, &mut degraded);
        Ok(MachineFacts {
            os: Platform::Linux,
            os_build,
            memory_total_mb: kb_to_mb(total_kb),
            commit_limit_mb,
            cpu,
            adapters,
            disk,
            power,
            monitors,
            degraded,
        })
    }
}

/// Where Eileen's data folder is by default on Linux: `$XDG_DATA_HOME/eileen`, or
/// `~/.local/share/eileen` when `XDG_DATA_HOME` is unset, empty or relative (the XDG base
/// directory rules; docs/LINUX.md section 5). `None` without a home folder. `EILEEN_DATA_DIR`
/// and `--data-dir` are the runtime's to apply.
#[must_use]
pub fn default_data_dir() -> Option<PathBuf> {
    data_dir_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

/// [`default_data_dir`] from the two variables' values.
fn data_dir_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let absolute =
        |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
    absolute(xdg_data_home)
        .or_else(|| absolute(home).map(|home| home.join(".local").join("share")))
        .map(|base| base.join("eileen"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::data_dir_from;

    #[test]
    fn the_data_folder_follows_the_xdg_rules() {
        let some = |text: &str| Some(text.into());
        assert_eq!(
            data_dir_from(some("/data/xdg"), some("/home/sam")),
            Some(PathBuf::from("/data/xdg/eileen")),
            "XDG_DATA_HOME first"
        );
        assert_eq!(
            data_dir_from(None, some("/home/sam")),
            Some(PathBuf::from("/home/sam/.local/share/eileen")),
            "the default under HOME"
        );
        assert_eq!(
            data_dir_from(some("relative/dir"), some("/home/sam")),
            Some(PathBuf::from("/home/sam/.local/share/eileen")),
            "a relative XDG_DATA_HOME is ignored"
        );
        assert_eq!(
            data_dir_from(some(""), some("/home/sam")),
            Some(PathBuf::from("/home/sam/.local/share/eileen")),
            "an empty XDG_DATA_HOME is ignored"
        );
        assert_eq!(data_dir_from(None, None), None, "no home");
    }
}

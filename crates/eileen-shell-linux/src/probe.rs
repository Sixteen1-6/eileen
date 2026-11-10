//! [`LinuxMachineProbe`]: `eileen-platform`'s `MachineProbe` on Linux (BUILD.md sections 2.16
//! and 3.4). It reads every fact in one pass over `/proc`, `/sys`, `/etc`, `/usr/share` and
//! `/var/lib`, plus the data folder's volume, into `eileen-contracts::machine`'s shapes.
//!
//! - `facts` runs the pass on a named thread of its own and waits at most the caller's deadline
//!   (the `bounded` module); past it the answer is `DeadlineExceeded` and the pass is left to its
//!   thread. Two kinds of read can hold it. Measuring the data folder's volume (its `stat`, the
//!   `statvfs` and resolving its path) waits on whoever serves the home folder, an NFS server or
//!   a FUSE daemon on some PCs, and a hard NFS mount whose server is gone waits for good. Some
//!   sysfs files ask a driver or the firmware: a battery's `status` runs the embedded
//!   controller's ACPI methods, a disk's `device/model` and a GPU's `mem_info_*` ask its driver,
//!   and a device that has hung holds the read. The audio endpoints and the execution providers
//!   are left empty, and `read_at` and `probe_ms` 0: their own components report the first two,
//!   and the caller stamps the others from its clock around the call.
//! - `load` is [`LinuxMachineProbe::read_load`] on the caller's thread: `/proc/stat` and the power
//!   files, which no other process serves, and the trait gives it no deadline.
//! - `watch` follows the power daemons on the system bus of the bridge
//!   [`LinuxMachineProbe::with_bridge`] gave, through [`MachineChanges`](crate::MachineChanges),
//!   whose module says which changes Linux reports. Without a bridge it is `Unavailable`.
//!
//! # Traps
//!
//! - A pass held past its deadline keeps its thread until the read returns, if ever, and each
//!   such call leaves one more; the runtime reads the facts at start and on a change only.
//! - `facts` and `load` read the power state from sysfs (the `power` module), which does not see
//!   a program's hold on power-profiles-daemon's `power-saver`, while `watch` reports the hold's
//!   `BatterySaver`: until the facts read the daemons over D-Bus, the facts read after that change
//!   can still say battery saver is off.
//! - `watch`'s sink runs on the bridge's thread, so it must not block, nor call the bridge.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use eileen_contracts::connectors::Platform;
use eileen_contracts::ids::WallTime;
use eileen_contracts::machine::{AudioFacts, DiskFacts, DiskMedia, MachineFacts, PowerState};
use eileen_dbus_linux::{Bridge, Buses, DEFAULT_DEADLINE};
use eileen_platform::{Deadline, MachineChange, MachineProbe, PlatformError, Subscription};

use crate::adapters::read_adapters;
use crate::cpu::read_cpu;
use crate::disk::{Volume, read_media, read_volume};
use crate::load::{CpuTimes, busy_pct, parse_proc_stat};
use crate::memory::{kb_to_mb, parse_meminfo};
use crate::monitors::read_monitors;
use crate::os::{Guest, guest, read_os};
use crate::power::read_power;
use crate::{LinuxPower, MachineChanges, ReadError, SysRoot, bounded};

/// What `facts` past its deadline names, as the fake probe names it.
const FACTS: &str = "MachineProbe::facts";
/// What `watch`'s errors name.
const WATCH: &str = "machine-change watch";

/// Reads the machine's facts on Linux (BUILD.md section 2.16), its load, and, given a bridge,
/// its changes.
#[derive(Debug)]
pub struct LinuxMachineProbe {
    root: SysRoot,
    data_dir: PathBuf,
    /// The CPU times at the previous [`read_load`](Self::read_load), and the share it reported.
    cpu: Mutex<Option<(CpuTimes, u8)>>,
    /// The bridge `watch` follows the power daemons on.
    bridge: Option<Bridge>,
}

impl LinuxMachineProbe {
    /// A probe that reads under `root` and measures the volume of `data_dir`. Its `watch` is
    /// `Unavailable` until [`Self::with_bridge`] gives it a bridge.
    #[must_use]
    pub fn new(root: SysRoot, data_dir: impl Into<PathBuf>) -> Self {
        Self { root, data_dir: data_dir.into(), cpu: Mutex::new(None), bridge: None }
    }

    /// A probe of the real machine, measuring the volume of `data_dir`.
    #[must_use]
    pub fn system(data_dir: impl Into<PathBuf>) -> Self {
        Self::new(SysRoot::system(), data_dir)
    }

    /// The probe, with `watch` following the power daemons on `bridge`'s system bus.
    #[must_use]
    pub fn with_bridge(mut self, bridge: Bridge) -> Self {
        self.bridge = Some(bridge);
        self
    }

    /// The CPU's busy share since the previous call, in percent, and the power state: what
    /// `MachineProbe::load` gives every 30 s. The first call's share covers the time since boot;
    /// a call before another clock tick has passed repeats the previous share. Unreadable power
    /// facts are not reported here: the facts' `degraded` list names them.
    pub fn read_load(&self) -> Result<(u8, PowerState), ReadError> {
        let text = self.root.read("proc/stat").map_err(ReadError::io("/proc/stat"))?;
        let now = parse_proc_stat(&text)
            .ok_or(ReadError::Missing { what: "/proc/stat", field: "cpu" })?;
        let pct = {
            let mut previous = self.cpu.lock().unwrap_or_else(PoisonError::into_inner);
            let (before, last_pct) = previous.unwrap_or((CpuTimes { busy: 0, total: 0 }, 0));
            match busy_pct(before, now) {
                Some(pct) => {
                    *previous = Some((now, pct));
                    pct
                }
                None => last_pct,
            }
        };
        Ok((pct, read_power(&self.root, false, &mut Vec::new())))
    }

    /// Reads every fact under the root on the caller's thread, with the data folder's volume as
    /// given: a fixture tree has no volume of its own to measure. The fields `MachineProbe::facts`
    /// leaves to others are left empty, and `read_at` and `probe_ms` 0, as there.
    pub fn facts_on(&self, volume: io::Result<Volume>) -> Result<MachineFacts, ReadError> {
        read_facts(&self.root, volume)
    }
}

impl MachineProbe for LinuxMachineProbe {
    /// Every fact, the data folder's volume measured on the real file system, read on a thread
    /// of its own within `deadline`. A file every kernel provides that is unreadable is
    /// `PlatformError::Io`, naming it; past the deadline, `DeadlineExceeded`.
    fn facts(&self, deadline: Deadline) -> Result<MachineFacts, PlatformError> {
        let (root, data_dir) = (self.root.clone(), self.data_dir.clone());
        Ok(bounded::within(FACTS, deadline, move || read_facts(&root, read_volume(&data_dir)))??)
    }

    /// [`LinuxMachineProbe::read_load`].
    fn load(&self) -> Result<(u8, PowerState), PlatformError> {
        Ok(self.read_load()?)
    }

    /// The power source's and battery saver's changes, from the power daemons on the bridge's
    /// system bus, waiting at most [`DEFAULT_DEADLINE`] for the subscriptions and the first
    /// reading. `Unavailable` without a bridge.
    fn watch(
        &self,
        sink: Box<dyn Fn(MachineChange) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        let bridge = self.bridge.as_ref().ok_or(PlatformError::Unavailable { what: WATCH })?;
        let open = |buses: Buses| async move {
            let power = LinuxPower::new(&buses.system().await?).changes().await?;
            Ok::<_, PlatformError>(MachineChanges::new(power))
        };
        bridge.watch(WATCH, DEFAULT_DEADLINE, open, sink)
    }
}

/// Reads every fact under `root`, with the data folder's volume as given.
fn read_facts(root: &SysRoot, volume: io::Result<Volume>) -> Result<MachineFacts, ReadError> {
    let meminfo = root.read("proc/meminfo").map_err(ReadError::io("/proc/meminfo"))?;
    let cpuinfo = root.read("proc/cpuinfo").map_err(ReadError::io("/proc/cpuinfo"))?;
    let totals = parse_meminfo(&meminfo);
    let total_kb =
        totals.total_kb.ok_or(ReadError::Missing { what: "/proc/meminfo", field: "MemTotal" })?;
    let mut degraded = Vec::new();
    let (os_build, kernel) = read_os(root, &mut degraded);
    let guest = guest(root, &kernel, &cpuinfo);
    let wsl = guest == Guest::Wsl;
    let commit_limit_mb = totals.commit_limit_kb.map_or_else(
        || {
            degraded.push("commit limit: /proc/meminfo has no CommitLimit line".to_owned());
            0
        },
        kb_to_mb,
    );
    let cpu = read_cpu(root, &cpuinfo, &mut degraded);
    let adapters = read_adapters(root, &kernel, &cpu, &mut degraded);
    let disk = match volume {
        Ok(volume) => DiskFacts {
            data_free_mb: volume.free_mb,
            data_total_mb: volume.total_mb,
            media: read_media(root, &volume, guest, &mut degraded),
        },
        Err(err) => {
            degraded.push(format!("disk: the data folder's volume is unreadable ({})", err.kind()));
            DiskFacts { data_free_mb: 0, data_total_mb: 0, media: DiskMedia::Unknown }
        }
    };
    let power = read_power(root, wsl, &mut degraded);
    let monitors = read_monitors(root, wsl, &mut degraded);
    Ok(MachineFacts {
        read_at: WallTime(0),
        probe_ms: 0,
        os: Platform::Linux,
        os_build,
        memory_total_mb: kb_to_mb(total_kb),
        commit_limit_mb,
        cpu,
        adapters,
        execution_providers: Vec::new(),
        disk,
        power,
        audio: AudioFacts::default(),
        monitors,
        degraded,
    })
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

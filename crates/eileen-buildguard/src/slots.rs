//! The two build slots and the desktop lock: OS file locks with holder records.
//!
//! Each lock is a file under the machine root (`build-slot-0.lock`, `build-slot-1.lock`,
//! `desktop.lock`) held with [`File::try_lock`], so the OS releases it when its holder dies.
//! Windows' file locks are mandatory, so nobody else could read a record written inside the
//! locked file; the holder's record (PID, worktree, command and start time) is written beside
//! it instead, as `<name>.holder`, and removed when the lock is released. A record whose lock
//! is free belongs to a holder that died; `cargo xtask slots --kill-stale` removes it.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// One of the two build slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// The slot every build tries first.
    Zero,
    /// The second slot, for heavy-build days with memory to spare.
    One,
}

impl Slot {
    /// Both slots, in the order a build tries them.
    pub const ALL: [Self; 2] = [Self::Zero, Self::One];

    /// 0 or 1.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Zero => "build-slot-0",
            Self::One => "build-slot-1",
        }
    }
}

/// The desktop lock's name: `desktop.lock` and `desktop.holder`.
pub const DESKTOP: &str = "desktop";

/// Who holds a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The holder's process id.
    pub pid: u32,
    /// The worktree it builds in.
    pub worktree: String,
    /// The command it runs.
    pub command: String,
    /// When it took the lock, Unix milliseconds.
    pub started_ms: u64,
}

impl Holder {
    fn to_text(&self) -> String {
        let one_line = |text: &str| text.replace(['\r', '\n'], " ");
        format!(
            "pid={}\nworktree={}\ncommand={}\nstarted_ms={}\n",
            self.pid,
            one_line(&self.worktree),
            one_line(&self.command),
            self.started_ms
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let get = |key: &str| {
            text.lines()
                .find_map(|line| line.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
        };
        Some(Self {
            pid: get("pid")?.parse().ok()?,
            worktree: get("worktree")?.to_owned(),
            command: get("command")?.to_owned(),
            started_ms: get("started_ms")?.parse().ok()?,
        })
    }
}

/// A held lock. Dropping it removes the holder record, then releases the lock.
#[derive(Debug)]
pub struct Held {
    holder_path: PathBuf,
    holder: Holder,
    // Dropped last, after the record is gone.
    _file: File,
}

impl Held {
    /// The record written for this hold.
    #[must_use]
    pub const fn holder(&self) -> &Holder {
        &self.holder
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // A missing record is fine: `kill_stale` may have run while the holder was wedged.
        let _ignored: io::Result<()> = fs::remove_file(&self.holder_path);
    }
}

fn paths(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    (root.join(format!("{name}.lock")), root.join(format!("{name}.holder")))
}

fn open_lock(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)
}

/// Takes the lock `name` under `root` if it is free, writing `holder`'s record.
pub fn try_take(root: &Path, name: &str, holder: &Holder) -> io::Result<Option<Held>> {
    let (lock_path, holder_path) = paths(root, name);
    let file = open_lock(&lock_path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e),
    }
    let temp = root.join(format!("{name}.holder.tmp"));
    fs::write(&temp, holder.to_text())?;
    fs::rename(&temp, &holder_path)?;
    Ok(Some(Held { holder_path, holder: holder.clone(), _file: file }))
}

/// Takes a build slot if it is free.
pub fn try_take_slot(root: &Path, slot: Slot, holder: &Holder) -> io::Result<Option<Held>> {
    try_take(root, slot.name(), holder)
}

/// Whether the lock `name` is held by anyone now (a brief probe of the lock).
pub fn is_held(root: &Path, name: &str) -> io::Result<bool> {
    let (lock_path, _) = paths(root, name);
    let file = match OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Whether a build slot is held by anyone now.
pub fn is_slot_held(root: &Path, slot: Slot) -> io::Result<bool> {
    is_held(root, slot.name())
}

/// The holder record of the lock `name`, if one is there.
#[must_use]
pub fn holder(root: &Path, name: &str) -> Option<Holder> {
    let (_, holder_path) = paths(root, name);
    Holder::parse(&fs::read_to_string(holder_path).ok()?)
}

/// The holder record of a build slot, if one is there.
#[must_use]
pub fn slot_holder(root: &Path, slot: Slot) -> Option<Holder> {
    holder(root, slot.name())
}

/// One slot, as `cargo xtask slots` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotStatus {
    /// Which slot.
    pub slot: Slot,
    /// Whether its lock is held now.
    pub held: bool,
    /// Its holder record, if any.
    pub holder: Option<Holder>,
    /// Whether the recorded holder process is still running.
    pub holder_alive: Option<bool>,
}

/// Both slots and their holders.
pub fn list(root: &Path) -> io::Result<Vec<SlotStatus>> {
    Slot::ALL
        .iter()
        .map(|&slot| {
            let holder = slot_holder(root, slot);
            let holder_alive = holder.as_ref().map(|h| crate::procs::process_alive(h.pid));
            Ok(SlotStatus { slot, held: is_held(root, slot.name())?, holder, holder_alive })
        })
        .collect()
}

/// Whether either build slot is held now.
#[must_use]
pub fn any_slot_held(root: &Path) -> bool {
    Slot::ALL.iter().any(|slot| is_held(root, slot.name()).unwrap_or(false))
}

/// A slot whose stale record `kill_stale` removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freed {
    /// Which slot.
    pub slot: Slot,
    /// The holder that is gone.
    pub holder: Holder,
}

/// Frees every slot whose holder is gone: its lock is free (the OS released it when the
/// holder died) but its record is still there. Returns what it removed.
pub fn kill_stale(root: &Path) -> io::Result<Vec<Freed>> {
    let mut freed = Vec::new();
    for slot in Slot::ALL {
        let Some(holder) = slot_holder(root, slot) else { continue };
        if is_held(root, slot.name())? {
            continue;
        }
        let (_, holder_path) = paths(root, slot.name());
        match fs::remove_file(holder_path) {
            Ok(()) => freed.push(Freed { slot, holder }),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::{
        Holder, Slot, any_slot_held, holder, is_held, kill_stale, list, slot_holder, try_take_slot,
    };

    fn root(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("eileen-buildguard-slots-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn me() -> Holder {
        Holder {
            pid: std::process::id(),
            worktree: "wt".to_owned(),
            command: "cargo test".to_owned(),
            started_ms: 5,
        }
    }

    #[test]
    fn a_slot_is_taken_once_and_records_its_holder() {
        let dir = root("take");
        let held = try_take_slot(&dir, Slot::Zero, &me()).unwrap().unwrap();
        assert!(try_take_slot(&dir, Slot::Zero, &me()).unwrap().is_none(), "a second hold fails");
        assert!(is_held(&dir, "build-slot-0").unwrap(), "held");
        assert!(any_slot_held(&dir), "any");
        assert_eq!(slot_holder(&dir, Slot::Zero), Some(me()), "record");
        let status = list(&dir).unwrap();
        assert_eq!(status[0].holder_alive, Some(true), "this process is alive");
        assert!(!status[1].held, "slot 1 free");
        drop(held);
        assert!(!is_held(&dir, "build-slot-0").unwrap(), "released");
        assert_eq!(holder(&dir, "build-slot-0"), None, "record removed");
        let _ignored = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kill_stale_removes_a_record_whose_lock_is_free() {
        let dir = root("stale");
        let ghost = Holder { pid: u32::MAX - 1, ..me() };
        std::fs::write(dir.join("build-slot-1.holder"), ghost.to_text()).unwrap();
        let held = try_take_slot(&dir, Slot::Zero, &me()).unwrap().unwrap();
        let freed = kill_stale(&dir).unwrap();
        assert_eq!(freed.len(), 1, "only the stale one");
        assert_eq!(freed[0].slot, Slot::One, "slot 1");
        assert_eq!(slot_holder(&dir, Slot::Zero), Some(me()), "a live hold is kept");
        drop(held);
        let _ignored = std::fs::remove_dir_all(&dir);
    }
}

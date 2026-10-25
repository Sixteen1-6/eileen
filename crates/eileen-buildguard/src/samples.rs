//! The machine-wide memory files: their only writer and reader (BUILD.md section 6.1).
//!
//! Three files under [`machine_root`], whatever `EILEEN_DATA_DIR` says, because they describe
//! the PC rather than one data directory:
//!
//! - `machine/memory-samples.jsonl`: 5 s samples from every guard process and the runtime, one
//!   `{"at_ms": <Unix ms>, "avail_mb": u32, "headroom_mb": u32, "paging_kb_s": u32,
//!   "slot_held": bool, "src": "guard" | "runtime"}` per line; lines older than 24 h are
//!   dropped by the next writer that finds the file over 5 MB.
//! - `machine/paging-30s.jsonl`: one `{"at_ms": <the window's end>, "paging_kb_s": u32,
//!   "slot_held": bool, "src": "runtime"}` per 30 s window, written only by a runtime reading
//!   real memory, and only when the newest line ends at least 30 s before the new window
//!   does, so two runtimes never write one window twice; lines older than 7 days are dropped
//!   by the next writer that finds the file over 2 MB.
//! - `buildguard.toml`: the baseline's thresholds, written by `cargo xtask slots --baseline`.
//!
//! Both line files are appended under a short [`std::fs::File::try_lock`] on
//! `machine/memory-samples.lock`, so lines never interleave.
//!
//! The runtime needs nothing else from the guard: [`slot_held_now`] (and [`Store::slot_held`])
//! probes the two build slot locks for the `slot_held` flag every sample and window carries.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::clock::{Clock, RealClock, millis};
use crate::json;

/// The memory samples, relative to the machine root.
pub const SAMPLES_FILE: &str = "machine/memory-samples.jsonl";
/// The 30 s paging windows, relative to the machine root.
pub const PAGING_FILE: &str = "machine/paging-30s.jsonl";
/// The lock both line files are appended under, relative to the machine root.
pub const LOCK_FILE: &str = "machine/memory-samples.lock";
/// The baseline's thresholds, relative to the machine root.
pub const BASELINE_FILE: &str = "buildguard.toml";

/// The samples file is trimmed once it passes this size.
pub const SAMPLES_TRIM_BYTES: u64 = 5 * 1024 * 1024;
/// How long samples are kept when the file is trimmed.
pub const SAMPLES_KEEP: Duration = Duration::from_hours(24);
/// The paging file is trimmed once it passes this size.
pub const PAGING_TRIM_BYTES: u64 = 2 * 1024 * 1024;
/// How long paging windows are kept when the file is trimmed.
pub const PAGING_KEEP: Duration = Duration::from_hours(7 * 24);
/// The length of one paging window.
pub const WINDOW: Duration = Duration::from_secs(30);

/// The `auto` paging limit's clamp, KB/s (section 2.5 rule 3).
pub const AUTO_LIMIT_MIN_KB_S: u32 = 2_000;
/// The `auto` paging limit's clamp, KB/s.
pub const AUTO_LIMIT_MAX_KB_S: u32 = 40_000;
/// The slot-0 paging limit until a baseline exists, KB/s: admission's own default, so the
/// runtime's `auto` limit and the guard can never drift apart.
pub const DEFAULT_PAGING_LIMIT_KB_S: u32 = crate::admission::DEFAULT_SLOT0_PAGING_KB_S;
/// How much time the runtime's paging windows must cover before the `auto` limit is learned
/// from them rather than from the baseline (section 2.5 rule 3).
pub const AUTO_LIMIT_COVERAGE: Duration = Duration::from_hours(24);

/// How long an append waits for the lock before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// Who wrote a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// A guarded command (waiting, or a running build's re-check).
    Guard,
    /// Eileen's own memory sampler.
    Runtime,
}

impl Source {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Guard => "guard",
            Self::Runtime => "runtime",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "guard" => Some(Self::Guard),
            "runtime" => Some(Self::Runtime),
            _ => None,
        }
    }
}

/// One memory sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// When it was taken, Unix milliseconds.
    pub at_ms: u64,
    /// Available physical memory (free plus standby), MB.
    pub avail_mb: u32,
    /// Commit headroom (the commit limit minus the commit charge), MB.
    pub headroom_mb: u32,
    /// The hard-fault paging rate, KB/s.
    pub paging_kb_s: u32,
    /// Whether a build slot lock was held when it was taken.
    pub slot_held: bool,
    /// Who took it.
    pub src: Source,
}

impl Sample {
    fn to_line(self) -> String {
        json::Writer::default()
            .num("at_ms", self.at_ms)
            .num("avail_mb", u64::from(self.avail_mb))
            .num("headroom_mb", u64::from(self.headroom_mb))
            .num("paging_kb_s", u64::from(self.paging_kb_s))
            .bool("slot_held", self.slot_held)
            .str("src", self.src.as_str())
            .finish()
    }

    fn from_line(line: &str) -> Option<Self> {
        let o = json::parse(line)?;
        Some(Self {
            at_ms: o.u64("at_ms")?,
            avail_mb: o.u32("avail_mb")?,
            headroom_mb: o.u32("headroom_mb")?,
            paging_kb_s: o.u32("paging_kb_s")?,
            slot_held: o.bool("slot_held")?,
            src: Source::parse(o.str("src")?)?,
        })
    }
}

/// One 30 s paging window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagingWindow {
    /// The window's end, Unix milliseconds.
    pub at_ms: u64,
    /// The window's average paging rate, KB/s.
    pub paging_kb_s: u32,
    /// Whether a build slot lock was held at any moment of the window.
    pub slot_held: bool,
    /// Who wrote it (always the runtime).
    pub src: Source,
}

impl PagingWindow {
    fn to_line(self) -> String {
        json::Writer::default()
            .num("at_ms", self.at_ms)
            .num("paging_kb_s", u64::from(self.paging_kb_s))
            .bool("slot_held", self.slot_held)
            .str("src", self.src.as_str())
            .finish()
    }

    fn from_line(line: &str) -> Option<Self> {
        let o = json::parse(line)?;
        Some(Self {
            at_ms: o.u64("at_ms")?,
            paging_kb_s: o.u32("paging_kb_s")?,
            slot_held: o.bool("slot_held")?,
            src: Source::parse(o.str("src")?)?,
        })
    }
}

/// The thresholds `cargo xtask slots --baseline` measured, as `buildguard.toml` holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// The first day sampled, `YYYY-MM-DD` (UTC).
    pub measured_from: String,
    /// The last day sampled, `YYYY-MM-DD` (UTC).
    pub measured_to: String,
    /// Samples used.
    pub samples: u64,
    /// Samples left out because a build slot was held.
    pub slot_held_excluded: u64,
    /// The slot-0 paging limit: the 75th percentile of the 30 s averages plus 25 percent.
    pub slot0_paging_kb_s: u32,
    /// The re-check limit: the 95th percentile plus 25 percent, at least twice slot 0's.
    pub recheck_paging_kb_s: u32,
    /// The one headroom floor: the lower of 2 GB and the 10th percentile, at least 1 GB.
    pub headroom_floor_mb: u32,
}

impl Baseline {
    fn to_toml(&self) -> String {
        let mut text = String::from(
            "# The build guard's thresholds, written by `cargo xtask slots --baseline`\n\
             # (BUILD.md section 6.1). The runtime reads slot0_paging_kb_s for memory.max_paging_kb_s = auto.\n",
        );
        let _infallible = write!(
            text,
            "measured_from = \"{}\"\nmeasured_to = \"{}\"\nsamples = {}\nslot_held_excluded = {}\n\
             slot0_paging_kb_s = {}\nrecheck_paging_kb_s = {}\nheadroom_floor_mb = {}\n",
            self.measured_from,
            self.measured_to,
            self.samples,
            self.slot_held_excluded,
            self.slot0_paging_kb_s,
            self.recheck_paging_kb_s,
            self.headroom_floor_mb,
        );
        text
    }

    /// Reads the subset of TOML the file uses: `key = value` lines, strings in double quotes,
    /// unsigned integers and comments. `None` unless every key is present and well formed.
    #[must_use]
    pub fn from_toml(text: &str) -> Option<Self> {
        let mut values: Vec<(&str, &str)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            values.push((key.trim(), value.trim()));
        }
        let get = |key: &str| values.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let string = |key: &str| {
            let v = get(key)?;
            let inner = v.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.contains('"') && !inner.contains('\\')).then(|| inner.to_owned())
        };
        let number = |key: &str| get(key)?.parse::<u64>().ok();
        let small = |key: &str| u32::try_from(number(key)?).ok();
        Some(Self {
            measured_from: string("measured_from")?,
            measured_to: string("measured_to")?,
            samples: number("samples")?,
            slot_held_excluded: number("slot_held_excluded")?,
            slot0_paging_kb_s: small("slot0_paging_kb_s")?,
            recheck_paging_kb_s: small("recheck_paging_kb_s")?,
            headroom_floor_mb: small("headroom_floor_mb")?,
        })
    }
}

/// Which source the `auto` paging limit came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitSource {
    /// The runtime's own 30 s windows, once they cover 24 hours.
    Windows,
    /// The baseline's slot-0 limit.
    Baseline,
    /// The code default, 5,000 KB/s.
    Default,
}

/// The effective `memory.max_paging_kb_s = auto`, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagingLimit {
    /// The limit, KB/s.
    pub kb_s: u32,
    /// Its source.
    pub source: LimitSource,
}

/// The machine root: `%LOCALAPPDATA%\eileen`, from the `LOCALAPPDATA` variable so a test can
/// point it at a temporary folder, and never from `EILEEN_DATA_DIR`.
///
/// On Windows there is no fallback: a process whose environment lacks `LOCALAPPDATA` (one a
/// harness built from scratch, a trimmed shell) gets [`ErrorKind::NotFound`] rather than a
/// second set of slot locks that are always free. Off Windows the variable still wins when it
/// is set (the tests set it on every OS); otherwise the root is `$XDG_STATE_HOME/eileen`, else
/// `$HOME/.local/state/eileen`, where docs/LINUX.md section 5 keeps the machine-wide files.
pub fn machine_root() -> io::Result<PathBuf> {
    machine_root_from(std::env::var_os)
}

/// [`machine_root`] over a variable getter, so a test covers each branch without changing the
/// process's environment. An empty variable counts as unset.
fn machine_root_from(var: impl Fn(&'static str) -> Option<OsString>) -> io::Result<PathBuf> {
    let set = |key: &'static str| var(key).filter(|value| !value.is_empty()).map(PathBuf::from);
    if let Some(local) = set("LOCALAPPDATA") {
        return Ok(local.join("eileen"));
    }
    #[cfg(not(windows))]
    {
        if let Some(state) = set("XDG_STATE_HOME") {
            return Ok(state.join("eileen"));
        }
        if let Some(home) = set("HOME") {
            return Ok(home.join(".local").join("state").join("eileen"));
        }
    }
    let missing = if cfg!(windows) {
        "LOCALAPPDATA is not set"
    } else {
        "none of LOCALAPPDATA, XDG_STATE_HOME and HOME is set"
    };
    Err(io::Error::new(ErrorKind::NotFound, missing))
}

/// The machine-wide files under one root: [`Store::machine`] for the real one, [`Store::at`]
/// for a test's temporary folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
    machine: bool,
}

impl Store {
    /// The real machine root.
    pub fn machine() -> io::Result<Self> {
        Ok(Self { root: machine_root()?, machine: true })
    }

    /// Another root (a test's temporary folder).
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), machine: false }
    }

    /// The root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// True for the real machine root, which a fake memory source must never write.
    #[must_use]
    pub const fn is_machine(&self) -> bool {
        self.machine
    }

    /// Whether either build slot's lock (`build-slot-0.lock`, `build-slot-1.lock` under this
    /// root) is held now: the `slot_held` of a [`Sample`], and of a [`PagingWindow`] when any
    /// of its samples saw it. A brief probe of each lock; a lock file that is missing or cannot
    /// be read counts as free.
    #[must_use]
    pub fn slot_held(&self) -> bool {
        crate::slots::any_slot_held(&self.root)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Runs `f` under the append lock, waiting at most two seconds of `clock`'s time for it.
    fn locked<T>(&self, clock: &dyn Clock, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        let lock_path = self.path(LOCK_FILE);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path)?;
        let deadline = clock.mono_ms().saturating_add(millis(LOCK_WAIT));
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if clock.mono_ms() < deadline => {
                    clock.sleep(Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(io::Error::new(ErrorKind::TimedOut, "memory-samples.lock is busy"));
                }
                Err(TryLockError::Error(e)) => return Err(e),
            }
        }
        let result = f();
        // Unlocked before the handle closes, which Windows may finish late.
        let unlocked = lock.unlock();
        let value = result?;
        unlocked?;
        Ok(value)
    }

    /// Appends one sample, trimming the file first when it has passed 5 MB. A busy lock is
    /// waited for on `clock`.
    pub fn append(&self, sample: &Sample, clock: &dyn Clock) -> io::Result<()> {
        let path = self.path(SAMPLES_FILE);
        self.locked(clock, || {
            let keep_from = sample.at_ms.saturating_sub(millis(SAMPLES_KEEP));
            trim(&path, SAMPLES_TRIM_BYTES, |line| {
                Sample::from_line(line).is_some_and(|s| s.at_ms >= keep_from)
            })?;
            append_line(&path, &sample.to_line())
        })
    }

    /// Every readable sample, in file order.
    pub fn samples(&self) -> io::Result<Vec<Sample>> {
        read_lines(&self.path(SAMPLES_FILE), Sample::from_line)
    }

    /// The samples taken in the `window` before `now_ms`, oldest first.
    pub fn recent_at(&self, now_ms: u64, window: Duration) -> io::Result<Vec<Sample>> {
        let from = now_ms.saturating_sub(millis(window));
        let mut samples: Vec<Sample> =
            self.samples()?.into_iter().filter(|s| s.at_ms >= from && s.at_ms <= now_ms).collect();
        samples.sort_by_key(|s| s.at_ms);
        Ok(samples)
    }

    /// Appends one paging window unless the file's newest line ends less than 30 s before
    /// it does (checked under the lock). Returns whether it was written. A busy lock is waited
    /// for on `clock`.
    pub fn append_paging(&self, window: &PagingWindow, clock: &dyn Clock) -> io::Result<bool> {
        let path = self.path(PAGING_FILE);
        self.locked(clock, || {
            let newest =
                read_lines(&path, PagingWindow::from_line)?.into_iter().map(|w| w.at_ms).max();
            if newest.is_some_and(|at| at.saturating_add(millis(WINDOW)) > window.at_ms) {
                return Ok(false);
            }
            let keep_from = window.at_ms.saturating_sub(millis(PAGING_KEEP));
            trim(&path, PAGING_TRIM_BYTES, |line| {
                PagingWindow::from_line(line).is_some_and(|w| w.at_ms >= keep_from)
            })?;
            append_line(&path, &window.to_line())?;
            Ok(true)
        })
    }

    /// The paging windows of the `days` before `now_ms`, oldest first.
    pub fn paging_windows_at(&self, now_ms: u64, days: u32) -> io::Result<Vec<PagingWindow>> {
        let from = now_ms.saturating_sub(u64::from(days) * 86_400_000);
        let mut windows: Vec<PagingWindow> =
            read_lines(&self.path(PAGING_FILE), PagingWindow::from_line)?
                .into_iter()
                .filter(|w| w.at_ms >= from && w.at_ms <= now_ms)
                .collect();
        windows.sort_by_key(|w| w.at_ms);
        Ok(windows)
    }

    /// The parsed `buildguard.toml`; `Ok(None)` when it is missing or malformed.
    pub fn baseline(&self) -> io::Result<Option<Baseline>> {
        match fs::read_to_string(self.path(BASELINE_FILE)) {
            Ok(text) => Ok(Baseline::from_toml(&text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Writes `buildguard.toml`, replacing it in one rename.
    pub fn write_baseline(&self, baseline: &Baseline) -> io::Result<()> {
        let path = self.path(BASELINE_FILE);
        fs::create_dir_all(&self.root)?;
        let temp = self.path("buildguard.toml.tmp");
        fs::write(&temp, baseline.to_toml())?;
        fs::rename(temp, path)
    }
}

fn append_line(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    file.write_all(&bytes)
}

fn read_lines<T>(path: &Path, parse: impl Fn(&str) -> Option<T>) -> io::Result<Vec<T>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter_map(parse).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Rewrites `path` keeping only the lines `keep` accepts, when it has passed `limit` bytes.
fn trim(path: &Path, limit: u64, keep: impl Fn(&str) -> bool) -> io::Result<()> {
    let size = match fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if size <= limit {
        return Ok(());
    }
    let text = fs::read_to_string(path)?;
    let mut kept = String::with_capacity(text.len() / 2);
    for line in text.lines().filter(|l| keep(l)) {
        kept.push_str(line);
        kept.push('\n');
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let mut file = File::create(&temp)?;
    file.write_all(kept.as_bytes())?;
    drop(file);
    fs::rename(temp, path)
}

/// Appends a sample to the machine root.
pub fn append(sample: &Sample) -> io::Result<()> {
    Store::machine()?.append(sample, &RealClock)
}

/// The machine root's samples from the last `window`.
pub fn recent(window: Duration) -> io::Result<Vec<Sample>> {
    Store::machine()?.recent_at(RealClock.now_ms(), window)
}

/// Appends a paging window to the machine root (see [`Store::append_paging`]).
pub fn append_paging(window: &PagingWindow) -> io::Result<bool> {
    Store::machine()?.append_paging(window, &RealClock)
}

/// The machine root's paging windows from the last `days`.
pub fn paging_windows(days: u32) -> io::Result<Vec<PagingWindow>> {
    Store::machine()?.paging_windows_at(RealClock.now_ms(), days)
}

/// Whether either build slot is held now on this PC (see [`Store::slot_held`]); false when
/// the machine root is unknown.
#[must_use]
pub fn slot_held_now() -> bool {
    Store::machine().is_ok_and(|store| store.slot_held())
}

/// The machine root's `buildguard.toml`, parsed; `None` when it is missing, unreadable or
/// malformed.
#[must_use]
pub fn baseline() -> Option<Baseline> {
    Store::machine().ok()?.baseline().ok().flatten()
}

/// The value at `pct` percent by the nearest-rank method; `None` for no values.
#[must_use]
pub fn percentile(values: &[u32], pct: u32) -> Option<u32> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = u64::try_from(sorted.len()).ok()?;
    if n == 0 {
        return None;
    }
    let rank = (u64::from(pct.min(100)) * n).div_ceil(100).max(1);
    sorted.get(usize::try_from(rank - 1).ok()?).copied()
}

/// A value plus 25 percent, rounded to the nearest whole number.
#[must_use]
pub fn plus_25(value: u32) -> u32 {
    u32::try_from((u64::from(value) * 5 + 2) / 4).unwrap_or(u32::MAX)
}

/// The 75th percentile plus 25 percent: the baseline's slot-0 limit and the runtime's
/// `auto` limit both use it.
#[must_use]
pub fn p75_plus_25(values: &[u32]) -> Option<u32> {
    percentile(values, 75).map(plus_25)
}

/// Section 2.5 rule 3's `auto` limit: [`p75_plus_25`] over the windows not marked
/// `slot_held`, clamped to 2,000 to 40,000 KB/s, once those windows cover
/// [`AUTO_LIMIT_COVERAGE`] (24 hours); else the baseline's `slot0_paging_kb_s` under the same
/// clamp; else [`DEFAULT_PAGING_LIMIT_KB_S`].
#[must_use]
pub fn auto_paging_limit(windows: &[PagingWindow], baseline: Option<&Baseline>) -> PagingLimit {
    let clamp = |v: u32| v.clamp(AUTO_LIMIT_MIN_KB_S, AUTO_LIMIT_MAX_KB_S);
    let free: Vec<u32> = windows.iter().filter(|w| !w.slot_held).map(|w| w.paging_kb_s).collect();
    let covered = u64::try_from(free.len()).unwrap_or(u64::MAX).saturating_mul(millis(WINDOW));
    if covered >= millis(AUTO_LIMIT_COVERAGE)
        && let Some(limit) = p75_plus_25(&free)
    {
        return PagingLimit { kb_s: clamp(limit), source: LimitSource::Windows };
    }
    match baseline {
        Some(b) => PagingLimit { kb_s: clamp(b.slot0_paging_kb_s), source: LimitSource::Baseline },
        None => PagingLimit { kb_s: DEFAULT_PAGING_LIMIT_KB_S, source: LimitSource::Default },
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{self, ErrorKind};
    use std::path::PathBuf;

    use super::{
        Baseline, LimitSource, PagingWindow, Sample, Source, Store, auto_paging_limit,
        machine_root_from, p75_plus_25, percentile, plus_25,
    };
    use crate::slots::{Holder, Slot, try_take_slot};

    /// The machine root a process with exactly these variables would use.
    fn root_with(vars: &[(&'static str, &'static str)]) -> io::Result<PathBuf> {
        let vars = vars.to_vec();
        machine_root_from(move |key| {
            vars.iter().find(|(name, _)| *name == key).map(|(_, value)| OsString::from(*value))
        })
    }

    #[test]
    fn the_machine_root_follows_each_platforms_rule() {
        assert_eq!(
            root_with(&[("LOCALAPPDATA", "local"), ("XDG_STATE_HOME", "state"), ("HOME", "home")])
                .unwrap(),
            PathBuf::from("local").join("eileen"),
            "LOCALAPPDATA wins on every OS"
        );
        let state = root_with(&[("LOCALAPPDATA", ""), ("XDG_STATE_HOME", "state"), ("HOME", "h")]);
        let home = root_with(&[("XDG_DATA_HOME", "data"), ("HOME", "home")]);
        let nothing = root_with(&[("XDG_DATA_HOME", "data")]);
        if cfg!(windows) {
            for (what, found) in
                [("XDG_STATE_HOME", &state), ("HOME", &home), ("nothing", &nothing)]
            {
                assert_eq!(
                    found.as_ref().map_err(io::Error::kind),
                    Err(ErrorKind::NotFound),
                    "without LOCALAPPDATA there is no root on Windows, whatever {what} says"
                );
            }
        } else {
            assert_eq!(
                state.unwrap(),
                PathBuf::from("state").join("eileen"),
                "an empty LOCALAPPDATA counts as unset, and XDG_STATE_HOME comes next"
            );
            assert_eq!(
                home.unwrap(),
                PathBuf::from("home").join(".local").join("state").join("eileen"),
                "then the state folder under HOME, never the data folder"
            );
            assert_eq!(
                nothing.as_ref().map_err(io::Error::kind),
                Err(ErrorKind::NotFound),
                "nothing set"
            );
        }
    }

    #[test]
    fn lines_round_trip_in_the_documented_format() {
        let sample = Sample {
            at_ms: 1_761_000_000_000,
            avail_mb: 1_300,
            headroom_mb: 5_500,
            paging_kb_s: 400,
            slot_held: true,
            src: Source::Runtime,
        };
        let line = sample.to_line();
        assert_eq!(
            line,
            r#"{"at_ms": 1761000000000, "avail_mb": 1300, "headroom_mb": 5500, "paging_kb_s": 400, "slot_held": true, "src": "runtime"}"#,
            "sample line",
        );
        assert_eq!(Sample::from_line(&line), Some(sample), "round trip");
        let window =
            PagingWindow { at_ms: 30_000, paging_kb_s: 9, slot_held: false, src: Source::Runtime };
        assert_eq!(PagingWindow::from_line(&window.to_line()), Some(window), "window round trip");
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let values: Vec<u32> = (1..=100).collect();
        assert_eq!(percentile(&values, 75), Some(75), "p75 of 1..=100");
        assert_eq!(percentile(&values, 10), Some(10), "p10");
        assert_eq!(percentile(&[7], 95), Some(7), "one value");
        assert_eq!(percentile(&[], 50), None, "none");
        assert_eq!(plus_25(4_000), 5_000, "plus 25 percent");
        assert_eq!(p75_plus_25(&values), Some(94), "75 plus 25 percent, rounded");
    }

    #[test]
    fn auto_limit_prefers_a_day_of_windows_then_the_baseline_then_the_default() {
        let window =
            |kb| PagingWindow { at_ms: 0, paging_kb_s: kb, slot_held: false, src: Source::Runtime };
        let baseline = Baseline::from_toml(
            "measured_from = \"2026-10-22\"\nmeasured_to = \"2026-10-22\"\nsamples = 1\n\
             slot_held_excluded = 0\nslot0_paging_kb_s = 90000\nrecheck_paging_kb_s = 1\nheadroom_floor_mb = 1024\n",
        )
        .unwrap();
        let short: Vec<_> = (0..100).map(|_| window(8_000)).collect();
        assert_eq!(auto_paging_limit(&short, None).source, LimitSource::Default, "default");
        assert_eq!(auto_paging_limit(&short, None).kb_s, 5_000, "default value");
        let from_baseline = auto_paging_limit(&short, Some(&baseline));
        assert_eq!(
            (from_baseline.kb_s, from_baseline.source),
            (40_000, LimitSource::Baseline),
            "clamped"
        );
        let mut day: Vec<_> = (0..2_880).map(|_| window(8_000)).collect();
        day.push(PagingWindow { slot_held: true, ..window(1_000_000) });
        let learned = auto_paging_limit(&day, Some(&baseline));
        assert_eq!(
            (learned.kb_s, learned.source),
            (10_000, LimitSource::Windows),
            "a day of windows"
        );
        day.remove(0);
        assert_eq!(
            auto_paging_limit(&day, None).source,
            LimitSource::Default,
            "2,879 free windows and a slot-held one do not cover a day",
        );
    }

    #[test]
    fn the_slot_probe_sees_a_held_and_a_free_slot() {
        let dir = std::env::temp_dir()
            .join(format!("eileen-buildguard-samples-{}-probe", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::at(&dir);
        assert!(!store.slot_held(), "no lock files: free");
        let holder = Holder {
            pid: std::process::id(),
            worktree: "wt".to_owned(),
            command: "cargo xtask test".to_owned(),
            started_ms: 0,
        };
        let held = try_take_slot(&dir, Slot::One, &holder).unwrap().unwrap();
        assert!(store.slot_held(), "slot 1 held");
        drop(held);
        assert!(!store.slot_held(), "released: free again");
        let _ignored = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_baseline_needs_every_key() {
        assert!(Baseline::from_toml("samples = 1\n").is_none(), "keys missing");
        assert!(Baseline::from_toml("not toml").is_none(), "malformed");
    }
}

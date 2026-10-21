//! Temporary roots, prefilled samples and the helper launcher.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eileen_buildguard::clock::{Clock, RealClock};
use eileen_buildguard::samples::{Sample, Source, Store};
use eileen_buildguard::{CargoChild, CargoInvocation, spawn_cargo};

/// A fresh, empty folder for one test, removed when dropped.
pub(crate) struct TempRoot(PathBuf);

impl TempRoot {
    pub(crate) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir()
            .join(format!("eileen-buildguard-it-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The machine root a helper started with `LOCALAPPDATA` set to this folder uses.
    pub(crate) fn machine(&self) -> PathBuf {
        self.0.join("eileen")
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

/// Seven shared samples, one every 5 s, ending at `now_ms`, as Eileen's runtime writes them.
pub(crate) fn prefill(
    store: &Store,
    now_ms: u64,
    avail_mb: u32,
    headroom_mb: u32,
    paging_kb_s: u32,
) {
    for i in 0..7 {
        store
            .append(&Sample {
                at_ms: now_ms - (6 - i) * 5_000,
                avail_mb,
                headroom_mb,
                paging_kb_s,
                slot_held: false,
                src: Source::Runtime,
            })
            .unwrap();
    }
}

/// Starts this test binary as a helper in `mode`, with its machine root under `root`.
pub(crate) fn helper(mode: &str, root: &TempRoot, extra: &[(&str, String)]) -> CargoChild {
    let exe = std::env::current_exe().unwrap();
    let mut env: Vec<(String, String)> = vec![
        ("EILEEN_BG_HELPER".to_owned(), mode.to_owned()),
        ("EILEEN_BG_DIR".to_owned(), root.path().display().to_string()),
        ("LOCALAPPDATA".to_owned(), root.path().display().to_string()),
    ];
    env.extend(extra.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    let invocation =
        CargoInvocation::program(exe, ["helper::entry", "--exact", "--nocapture", "--quiet"])
            .with_env(env)
            .quiet();
    spawn_cargo(&invocation).unwrap()
}

/// Polls `condition` until it holds or `limit` passes; never a fixed delay.
pub(crate) fn wait_for(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let clock = RealClock;
    let deadline = clock.mono_ms() + u64::try_from(limit.as_millis()).unwrap();
    loop {
        if condition() {
            return true;
        }
        if clock.mono_ms() >= deadline {
            return false;
        }
        clock.sleep(Duration::from_millis(20));
    }
}

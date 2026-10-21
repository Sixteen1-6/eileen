//! Process tests: this unit-test binary runs itself again as a helper child
//! (`tests::helper_entry --exact`, with `EILEEN_SV_HELPER` naming what to do; without it the
//! entry returns at once). Helpers that must stay alive block on `block.lock`, which the test
//! holds, instead of sleeping.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use eileen_stdx::clock::{self, Clock as _};
use eileen_trace::{names, test_support};

use crate::{ChildSpec, spawn};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("eileen-supervisor-{}-{name}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn lock_file(dir: &Path) -> File {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("block.lock"))
        .unwrap()
}

/// Holds `block.lock` so the blocking helpers stay alive until it is dropped.
#[cfg(windows)]
fn hold_block(dir: &Path) -> File {
    let file = lock_file(dir);
    file.try_lock().unwrap();
    file
}

fn helper(mode: &str, dir: &Path) -> ChildSpec {
    ChildSpec::new("test.helper", std::env::current_exe().unwrap())
        .args(["tests::helper_entry", "--exact", "--nocapture", "--quiet"])
        .envs([("EILEEN_SV_HELPER", mode.to_owned()), ("EILEEN_SV_DIR", dir.display().to_string())])
}

fn current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// Polls `condition` until it holds or `limit` passes; never a fixed delay.
#[cfg(windows)]
fn wait_for(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = clock::system().mono().0 + u64::try_from(limit.as_nanos()).unwrap();
    while clock::system().mono().0 < deadline {
        if condition() {
            return true;
        }
        eileen_stdx::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

fn write_pids(dir: &Path, text: &str) {
    std::fs::write(dir.join("pids.tmp"), text).unwrap();
    std::fs::rename(dir.join("pids.tmp"), dir.join("pids")).unwrap();
}

#[cfg(windows)]
fn read_pids(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("pids"))
        .unwrap()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

#[test]
fn helper_entry() {
    let Some(mode) = std::env::var_os("EILEEN_SV_HELPER") else {
        return;
    };
    let dir = PathBuf::from(std::env::var_os("EILEEN_SV_DIR").unwrap());
    match mode.to_str().unwrap() {
        "stderr" => {
            eprintln!("eileen helper says hello");
            panic!("the helper exits with a failure on purpose");
        }
        "block" => lock_file(&dir).lock().unwrap(),
        // A supervised child that starts a grandchild the plain way (the guard-style spawn,
        // which a supervised job refuses to let break away) and waits.
        "grandparent" => {
            let grandchild = crate::spawn_detached(&helper("block", &dir)).unwrap();
            write_pids(&dir, &format!("{} {}", grandchild.pid, grandchild.broke_away));
            lock_file(&dir).lock().unwrap();
        }
        // A parent outside every kill-on-close job that starts the guard-style child and exits.
        "guard-parent" => {
            let guard = crate::spawn_detached(&helper("block", &dir)).unwrap();
            write_pids(&dir, &guard.pid.to_string());
        }
        other => panic!("unknown helper {other}"),
    }
}

#[test]
fn stderr_lines_become_child_stderr_events() {
    let dir = TempDir::new("stderr");
    let ((), captured) = test_support::capture(|| {
        current_thread().block_on(async {
            let mut child = spawn(&helper("stderr", &dir.0)).unwrap();
            let forwarding = child.forward_stderr().unwrap();
            let status = child.wait().await.unwrap();
            forwarding.await.unwrap();
            assert_eq!(status.code(), Some(101), "the helper's failure comes back");
        });
    });
    let lines = captured.events_named(names::CHILD_STDERR);
    assert!(lines.iter().any(|e| e.fields["line"] == "eileen helper says hello"), "{lines:?}");
    assert_eq!(captured.events_named(names::CHILD_SPAWNED).len(), 1, "spawned");
    assert_eq!(captured.events_named(names::CHILD_EXITED).len(), 1, "exited");
}

#[cfg(windows)]
#[test]
fn killing_the_job_kills_the_grandchild() {
    use crate::ffi::process::alive;

    let dir = TempDir::new("grandchild");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut child = spawn(&helper("grandparent", &dir.0)).unwrap();
        let child_pid = child.id().unwrap();
        assert!(
            wait_for(Duration::from_secs(30), || dir.0.join("pids").exists()),
            "the grandchild started"
        );
        let pids = read_pids(&dir.0);
        let grandchild: u32 = pids[0].parse().unwrap();
        assert_eq!(pids[1], "false", "a supervised job lets nothing break away");
        assert!(alive(child_pid) && alive(grandchild), "both run");
        child.start_kill().unwrap();
        child.wait().await.unwrap();
        assert!(
            wait_for(Duration::from_secs(10), || !alive(child_pid) && !alive(grandchild)),
            "the job took the grandchild with it"
        );
    });
    drop(block);
}

#[cfg(windows)]
#[test]
fn the_guard_spawn_outlives_its_parent() {
    use crate::ffi::process::{alive, terminate};

    let dir = TempDir::new("guard");
    let block = hold_block(&dir.0);
    let parent = crate::spawn_detached(&helper("guard-parent", &dir.0)).unwrap();
    assert!(
        wait_for(Duration::from_secs(30), || dir.0.join("pids").exists()),
        "the parent started the guard"
    );
    let guard: u32 = read_pids(&dir.0)[0].parse().unwrap();
    assert!(wait_for(Duration::from_secs(30), || !alive(parent.pid)), "the parent exited");
    assert!(alive(guard), "the guard-style child outlived its parent");
    assert!(terminate(guard), "killed at the end of the test");
    assert!(wait_for(Duration::from_secs(10), || !alive(guard)), "gone");
    drop(block);
}

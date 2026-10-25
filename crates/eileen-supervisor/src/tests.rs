//! Process tests: this unit-test binary runs itself again as a helper child
//! (`tests::helper_entry --exact`, with `EILEEN_SV_HELPER` naming what to do; without it the
//! entry returns at once). Helpers that must stay alive block on `block.lock`, which the test
//! holds, instead of sleeping. A supervised helper says `ready ...` on its stdout, which the
//! test reads, and the test waits on process handles for exits, so nothing polls.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::Duration;

use eileen_trace::{names, test_support};

use crate::{ChildSpec, MAX_STDERR_LINE, spawn};

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

/// Says `ready <text>` on stdout, which a supervised helper's parent reads.
fn say_ready(text: &str) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "ready {text}").unwrap();
    out.flush().unwrap();
}

/// Blocks until a supervised child writes its `ready ...` line, skipping the test harness's
/// own lines, and returns the rest of it with the reader, which the test keeps while the child
/// runs. A child that ends without one fails the test.
#[cfg(windows)]
async fn read_ready(
    child: &mut crate::Child,
) -> (String, tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>) {
    use tokio::io::AsyncBufReadExt as _;

    let mut lines = tokio::io::BufReader::new(child.take_stdout().unwrap()).lines();
    loop {
        let line = lines.next_line().await.unwrap().expect("the helper ended without ready");
        if let Some(rest) = line.trim_end().strip_prefix("ready") {
            return (rest.trim().to_owned(), lines);
        }
    }
}

fn write_pids(dir: &Path, text: &str) {
    std::fs::write(dir.join("pids.tmp"), text).unwrap();
    std::fs::rename(dir.join("pids.tmp"), dir.join("pids")).unwrap();
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
        // Stderr a hostile tool writes: bytes that are not UTF-8, then a megabyte without a
        // newline, then an ordinary line.
        "stderr-hostile" => {
            let mut err = std::io::stderr().lock();
            err.write_all(b"not utf-8: \xff\xfe here\n").unwrap();
            err.write_all(&vec![b'x'; 1024 * 1024]).unwrap();
            err.write_all(b"\nafter the long line\n").unwrap();
            err.flush().unwrap();
        }
        "block" => lock_file(&dir).lock().unwrap(),
        // A supervised child that starts a grandchild the plain way (the guard-style spawn,
        // which a supervised job refuses to let break away), says both, and waits.
        "grandparent" => {
            let grandchild = crate::spawn_detached(&helper("block", &dir)).unwrap();
            say_ready(&format!("{} {}", grandchild.pid, grandchild.broke_away));
            lock_file(&dir).lock().unwrap();
        }
        // A parent outside every kill-on-close job that starts the guard-style child, writes
        // its pid, and exits.
        "guard-parent" => {
            let guard = crate::spawn_detached(&helper("block", &dir)).unwrap();
            write_pids(&dir, &guard.pid.to_string());
        }
        // Blocks until released, then exits with 259, which is STILL_ACTIVE.
        #[cfg(windows)]
        "exit-259" => {
            lock_file(&dir).lock().unwrap();
            crate::ffi::process::exit_now(259)
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
    assert!(
        lines.iter().any(|e| e.fields["pii.line"] == "eileen helper says hello"),
        "the line travels as pii.line, which the files reduce: {lines:?}"
    );
    assert!(lines.iter().all(|e| !e.fields.contains_key("line")), "no plain line field");
    assert_eq!(captured.events_named(names::CHILD_SPAWNED).len(), 1, "spawned");
    assert_eq!(captured.events_named(names::CHILD_EXITED).len(), 1, "exited");
}

#[test]
fn hostile_stderr_keeps_forwarding_with_lines_cut() {
    let dir = TempDir::new("stderr-hostile");
    let ((), captured) = test_support::capture(|| {
        current_thread().block_on(async {
            let mut child = spawn(&helper("stderr-hostile", &dir.0)).unwrap();
            let forwarding = child.forward_stderr().unwrap();
            let status = child.wait().await.unwrap();
            forwarding.await.unwrap();
            assert!(status.success(), "every write reached the pipe: {status:?}");
        });
    });
    let lines: Vec<String> = captured
        .events_named(names::CHILD_STDERR)
        .iter()
        .map(|e| e.fields["pii.line"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        lines.iter().any(|l| l == "not utf-8: \u{fffd}\u{fffd} here"),
        "decoded with replacements"
    );
    let long: Vec<&String> = lines.iter().filter(|l| l.starts_with("xxx")).collect();
    assert_eq!(long.len(), 1, "one long line");
    assert_eq!(long[0].len(), MAX_STDERR_LINE, "cut to the limit");
    assert!(
        lines.iter().any(|l| l == "after the long line"),
        "forwarding went on: {:?}",
        lines.last()
    );
}

#[test]
fn a_spec_never_shows_its_environment_values_in_debug() {
    let spec = ChildSpec::new("claude.worker", "claude")
        .envs([("EILEEN_SESSION_TOKEN", "tok-canary-4f7q2zk"), ("EILEEN_JOB_ID", "job_canary")]);
    let shown = format!("{spec:?}");
    assert!(shown.contains("EILEEN_SESSION_TOKEN"), "the names are listed: {shown}");
    assert!(
        !shown.contains("tok-canary") && !shown.contains("job_canary"),
        "the values are not: {shown}"
    );
}

#[cfg(windows)]
#[test]
fn a_helper_that_exits_with_259_is_not_alive() {
    use crate::ffi::process::{alive, exit_code, open_for_wait, wait_exit};

    let dir = TempDir::new("exit-259");
    let block = hold_block(&dir.0);
    let exiting = crate::spawn_detached(&helper("exit-259", &dir.0)).unwrap();
    // Opened while it waits for the lock, so the ended process can still be read.
    let process = open_for_wait(exiting.pid).unwrap();
    assert!(alive(exiting.pid), "running while it waits for the lock");
    drop(block);
    assert!(wait_exit(&process, Duration::from_secs(30)), "it ended once released");
    assert_eq!(exit_code(&process), Some(259), "with 259, which is STILL_ACTIVE");
    assert!(!alive(exiting.pid), "an ended process is not alive, whatever its exit code");
}

#[cfg(windows)]
#[test]
fn killing_the_job_kills_the_grandchild() {
    use crate::ffi::process::{alive, open_for_wait, wait_exit};

    let dir = TempDir::new("grandchild");
    let block = hold_block(&dir.0);
    current_thread().block_on(async {
        let mut child = spawn(&helper("grandparent", &dir.0)).unwrap();
        let child_pid = child.id().unwrap();
        let (ready, stdout) = read_ready(&mut child).await;
        let said: Vec<&str> = ready.split_whitespace().collect();
        let grandchild: u32 = said[0].parse().unwrap();
        assert_eq!(said[1], "false", "a supervised job lets nothing break away");
        assert!(alive(child_pid) && alive(grandchild), "both run");
        // Opened while it runs, so its end is waited on rather than polled for.
        let grandchild_handle = open_for_wait(grandchild).unwrap();
        child.start_kill().unwrap();
        child.wait().await.unwrap();
        assert!(!alive(child_pid), "the child is gone");
        assert!(
            wait_exit(&grandchild_handle, Duration::from_secs(10)),
            "the job took the grandchild with it"
        );
        drop(stdout);
    });
    drop(block);
}

#[cfg(windows)]
#[test]
fn the_guard_spawn_outlives_its_parent() {
    use crate::ffi::process::{alive, open_for_wait, terminate, wait_exit};

    let dir = TempDir::new("guard");
    let block = hold_block(&dir.0);
    let parent = crate::spawn_detached(&helper("guard-parent", &dir.0)).unwrap();
    // A parent that has already exited cannot be opened, which is what this waits for anyway.
    if let Some(parent) = open_for_wait(parent.pid) {
        assert!(wait_exit(&parent, Duration::from_secs(30)), "the parent exited");
    }
    // The parent wrote the pid before it exited.
    let pids = std::fs::read_to_string(dir.0.join("pids")).unwrap();
    let guard: u32 = pids.trim().parse().unwrap();
    assert!(alive(guard), "the guard-style child outlived its parent");
    let guard_handle = open_for_wait(guard).unwrap();
    assert!(terminate(guard), "killed at the end of the test");
    assert!(wait_exit(&guard_handle, Duration::from_secs(10)), "gone");
    drop(block);
}

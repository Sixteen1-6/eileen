//! Temporary roots, prefilled samples, the helper launcher, and waits that block on a pipe or
//! a lock instead of polling.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ChildStdout;

use eileen_buildguard::clock::ManualClock;
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
    let clock = ManualClock::new(now_ms);
    for i in 0..7 {
        let sample = Sample {
            at_ms: now_ms - (6 - i) * 5_000,
            avail_mb,
            headroom_mb,
            paging_kb_s,
            slot_held: false,
            src: Source::Runtime,
        };
        store.append(&sample, &clock).unwrap();
    }
}

/// The helper `mode` as an invocation, with its machine root under `root`.
fn helper_invocation(mode: &str, root: &TempRoot, extra: &[(&str, String)]) -> CargoInvocation {
    let exe = std::env::current_exe().unwrap();
    let mut env: Vec<(String, String)> = vec![
        ("EILEEN_BG_HELPER".to_owned(), mode.to_owned()),
        ("EILEEN_BG_DIR".to_owned(), root.path().display().to_string()),
        ("LOCALAPPDATA".to_owned(), root.path().display().to_string()),
    ];
    env.extend(extra.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    CargoInvocation::program(exe, ["helper::entry", "--exact", "--nocapture", "--quiet"])
        .with_env(env)
}

/// Starts this test binary as a helper in `mode`, with null standard handles.
pub(crate) fn helper(mode: &str, root: &TempRoot, extra: &[(&str, String)]) -> CargoChild {
    spawn_cargo(&helper_invocation(mode, root, extra).quiet()).unwrap()
}

/// Starts a helper that says `ready ...` on its stdout, with the reader of that stdout, which
/// the test keeps for as long as the helper runs.
pub(crate) fn ready_helper(mode: &str, root: &TempRoot) -> (CargoChild, BufReader<ChildStdout>) {
    let mut child = spawn_cargo(&helper_invocation(mode, root, &[]).piped_stdout()).unwrap();
    let stdout = BufReader::new(child.take_stdout().unwrap());
    (child, stdout)
}

/// Blocks until a helper writes its `ready ...` line, skipping the test harness's own lines,
/// and returns the rest of it. A helper that ends without one fails the test.
pub(crate) fn read_ready(stdout: &mut impl BufRead) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "the helper ended without saying ready");
        if let Some(rest) = line.trim_end().strip_prefix("ready") {
            return rest.trim().to_owned();
        }
    }
}

/// The abstract socket a stand-in with process id `pid` listens on (Linux).
#[cfg(target_os = "linux")]
fn end_address(pid: u32) -> std::os::unix::net::SocketAddr {
    use std::os::linux::net::SocketAddrExt as _;

    std::os::unix::net::SocketAddr::from_abstract_name(format!("eileen-buildguard-it-{pid}"))
        .unwrap()
}

/// Called by a stand-in on Linux, which keeps what it returns until it ends: a socket that
/// listens under its process id and never accepts, so a test's connection to it ends when the
/// stand-in does ([`Watched`]). On Windows a test opens the process itself.
#[cfg(target_os = "linux")]
pub(crate) fn listen_for_end() -> std::os::unix::net::UnixListener {
    std::os::unix::net::UnixListener::bind_addr(&end_address(std::process::id())).unwrap()
}

/// A stand-in opened so its end can be waited on without polling (Linux; on Windows,
/// `eileen_buildguard::procs::Watched` opens the process itself): a connection to the socket
/// it listens on, which the kernel resets when the listener closes, so when the stand-in ends,
/// however it ends.
#[cfg(target_os = "linux")]
pub(crate) struct Watched(std::os::unix::net::UnixStream);

#[cfg(target_os = "linux")]
impl Watched {
    /// Connects to the stand-in `pid`, which must be listening already.
    pub(crate) fn open(pid: u32) -> std::io::Result<Self> {
        std::os::unix::net::UnixStream::connect_addr(&end_address(pid)).map(Self)
    }

    /// Waits up to `timeout` for the stand-in to end; true once it has, false at the timeout.
    pub(crate) fn wait_exit(&self, timeout: std::time::Duration) -> std::io::Result<bool> {
        use std::io::{ErrorKind, Read as _};

        self.0.set_read_timeout(Some(timeout))?;
        match (&self.0).read(&mut [0_u8; 1]) {
            Ok(0) => Ok(true),
            Err(e) if e.kind() == ErrorKind::ConnectionReset => Ok(true),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(false),
            Ok(_) => Err(std::io::Error::other("a stand-in wrote to the socket it listens on")),
            Err(e) => Err(e),
        }
    }
}

/// Blocks until the lock file `name` under `dir` is free (the OS releases a dead holder's lock
/// on its own schedule), then lets go of it.
pub(crate) fn wait_until_free(dir: &Path, name: &str) {
    let file = OpenOptions::new().read(true).write(true).open(dir.join(name)).unwrap();
    file.lock().unwrap();
    file.unlock().unwrap();
}

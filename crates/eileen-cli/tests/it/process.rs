//! The `eileen` binary itself, started through the supervisor as every child of Eileen is.

use std::time::Duration;

use eileen_supervisor::{ChildSpec, spawn};
use tokio::io::AsyncReadExt as _;
use tokio::time::timeout;

/// A guard against a hung binary, far above anything a working one needs.
const HANG: Duration = Duration::from_secs(20);

/// Runs `eileen` with `args` and returns its exit success, stdout and stderr. Stderr comes as
/// the raw pipe (`raw_stderr`), which the supervisor would otherwise forward into tracing. Both
/// pipes are read at once, so a child that fills one while the test waits on the other cannot
/// deadlock.
async fn eileen(args: &[&str]) -> (bool, String, String) {
    let spec = ChildSpec {
        raw_stderr: true,
        ..ChildSpec::new("eileen", env!("CARGO_BIN_EXE_eileen")).args(args.iter().copied())
    };
    let mut child = spawn(&spec).unwrap();
    let mut stdout_pipe = child.take_stdout().unwrap();
    let mut stderr_pipe = child.take_stderr().unwrap();
    let mut stdout = String::new();
    let mut stderr = String::new();
    let reads = async {
        tokio::try_join!(
            stdout_pipe.read_to_string(&mut stdout),
            stderr_pipe.read_to_string(&mut stderr)
        )
    };
    timeout(HANG, reads).await.expect("eileen hung").unwrap();
    let status = timeout(HANG, child.wait()).await.expect("eileen did not exit").unwrap();
    (status.success(), stdout, stderr)
}

#[tokio::test]
async fn eileen_version_runs() {
    let (success, stdout, _) = eileen(&["--version"]).await;
    assert!(success, "--version exits 0");
    assert_eq!(stdout.trim(), concat!("eileen ", env!("CARGO_PKG_VERSION")), "the version");
}

#[tokio::test]
async fn a_command_says_it_is_not_built_yet_and_fails() {
    let (success, _, stderr) = eileen(&["status"]).await;
    assert!(!success, "a command that is not built yet exits with a failure");
    assert!(stderr.contains("`eileen status` is not built yet"), "it said: {stderr}");
}

//! The `eileen` binary itself, started through the supervisor as every child of Eileen is.

use eileen_supervisor::{ChildSpec, spawn};
use tokio::io::AsyncReadExt as _;

/// Runs `eileen` with `args` and returns its exit success, stdout and stderr.
async fn eileen(args: &[&str]) -> (bool, String, String) {
    let spec = ChildSpec::new("eileen", env!("CARGO_BIN_EXE_eileen")).args(args.iter().copied());
    let mut child = spawn(&spec).unwrap();
    let mut stdout = String::new();
    let mut stderr = String::new();
    child.take_stdout().unwrap().read_to_string(&mut stdout).await.unwrap();
    child.take_stderr().unwrap().read_to_string(&mut stderr).await.unwrap();
    let status = child.wait().await.unwrap();
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

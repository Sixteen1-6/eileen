//! The processes the fake apps of `grounding` and `automation` run in: the test binary's own,
//! and a copy of it under another name, [`OtherProcess`], which serves one app from a process of
//! its own for `a_window_names_its_apps_own_process`.

use std::convert::Infallible;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use eileen_dbus_linux::testing::PrivateBus;

use super::{FRAME, root, window};
use crate::fake::App;

/// The file name of the test binary: every fake app's process.
pub(crate) fn this_process() -> String {
    let exe = std::env::current_exe().unwrap();
    exe.file_name().unwrap().to_str().unwrap().to_owned()
}

/// Where a copy of this binary in another process finds the bus to serve its app on. Set, it
/// makes [`OTHER_TEST`] serve that app rather than check it.
pub(super) const OTHER_BUS: &str = "EILEEN_TEST_OTHER_BUS";
/// That copy's file name, which names its process.
pub(super) const OTHER_EXE: &str = "eileen-fake-app";
/// The test the copy runs, as the test harness names it: the one that starts the copy.
const OTHER_TEST: &str = "grounding::a_window_names_its_apps_own_process";
/// What the copy prints before its app's unique name, after the test harness's own words on the
/// same line.
const OTHER_SAYS: &str = "fake app at ";

/// A copy of this test binary, named [`OTHER_EXE`], running [`OTHER_TEST`] in a process of its
/// own, where [`OTHER_BUS`] makes the test serve its app until it is killed
/// ([`serve_until_killed`]). Its fields drop in order, a panicking test's included: the process
/// is killed and reaped, then its folder removed.
pub(super) struct OtherProcess {
    child: Running,
    _folder: Folder,
    /// The unique name of its app's connection.
    pub(super) name: String,
}

/// A child process, killed and reaped when it drops.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ignored = self.0.kill();
        let _ignored = self.0.wait();
    }
}

/// A folder of the test's own, removed when it drops.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

impl OtherProcess {
    /// Starts the copy, its app on `bus`, once it has said its app's name. The copy is a hard
    /// link where the target folder allows one, else a copy of the file.
    pub(super) fn start(bus: &PrivateBus) -> Self {
        let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let folder = Folder(tmp.join(format!("other-process-{}", std::process::id())));
        let _ignored = std::fs::remove_dir_all(&folder.0);
        std::fs::create_dir_all(&folder.0).unwrap();
        let (this, exe) = (std::env::current_exe().unwrap(), folder.0.join(OTHER_EXE));
        if std::fs::hard_link(&this, &exe).is_err() {
            std::fs::copy(&this, &exe).unwrap();
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "a test starts a copy of itself; the supervisor rule is for product code"
        )]
        let mut command = Command::new(&exe);
        let spawned = command
            .args(["--exact", OTHER_TEST, "--nocapture", "--test-threads=1"])
            .env(OTHER_BUS, bus.address())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = Running(spawned.unwrap());
        let said = BufReader::new(child.0.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok)
            .find_map(|line| line.split_once(OTHER_SAYS).map(|(_, name)| name.to_owned()));
        let name = said.expect("the copy said its app's name");
        Self { child, _folder: folder, name }
    }

    /// The copy's process id.
    pub(super) fn pid(&self) -> u32 {
        self.child.0.id()
    }
}

/// What [`OTHER_TEST`] does in [`OtherProcess`]'s copy: serves an app with one window,
/// `Elsewhere`, on the bus at `address`, says the app's name, and serves it until the process is
/// killed, so the copy's test never ends, let alone passes.
pub(super) fn serve_until_killed(address: &str) -> ! {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let serve = async {
        let app = App::at(address).await;
        let nodes =
            [root(&app, &["/elsewhere"]), ("/elsewhere", window(FRAME, "Elsewhere", &[], vec![]))];
        for (path, node) in nodes {
            app.add(path, node).await;
        }
        println!("{OTHER_SAYS}{}", app.name());
        std::future::pending::<Infallible>().await
    };
    match runtime.block_on(serve) {}
}

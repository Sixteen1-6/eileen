//! `ShellHost` through `LinuxShell` on the session's real services: logind on the system bus
//! and the notification server on the session bus. What the tests share: the shell they test,
//! and a connection of the test's own to the system bus, to see what logind did.
//!
//! - Each test makes its own bridge with `Bridge::start()`, as the core makes one, and the
//!   profile runs the tests one at a time, since they share the session's services.
//! - The test's thread calls the shell as the core does, blocking, while the workers of a runtime
//!   of the test's own read its connection, so a signal sent meanwhile waits in its stream.
//! - No test waits for nothing to arrive. A signal check follows a round trip to its sender
//!   (`testing::round_trip`), and a wait for a signal is `testing::next_message`, which has no
//!   timer: the profile's slow-timeout ends a wait that never ends.
//! - Streams live and drop inside `Session::block_on`, within their runtime's context, a
//!   panicking test's included (the stream traps of `eileen-dbus-linux`).

use std::future::Future;
use std::path::PathBuf;

use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, system_bus};
use eileen_shell_linux::{LinuxShell, LinuxShellSettings, NotifierSettings};

mod keep_awake;
mod lock;
mod toast;

/// logind's name, its manager's object and the manager's interface.
const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

/// The test's own connection to the system bus, apart from the shell's bridge, on a runtime of
/// its own whose two workers read it while the test's thread waits on the shell.
struct Session {
    runtime: tokio::runtime::Runtime,
    /// `None` only while the session drops.
    system: Option<zbus::Connection>,
}

impl Session {
    /// Connects to the system bus as the product does, each call bounded by the method timeout.
    fn new() -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        let runtime = runtime.unwrap();
        let system = runtime.block_on(system_bus()).unwrap();
        Self { runtime, system: Some(system) }
    }

    /// The connection.
    fn system(&self) -> &zbus::Connection {
        self.system.as_ref().unwrap()
    }

    /// Runs `future` to its end on the test's thread, inside the runtime's context.
    fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The connection ends inside its runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.system = None;
    }
}

/// A shell on a bridge of its own to the real buses: no autostart folder, and the deadline of a
/// trait method that takes none.
fn shell() -> LinuxShell {
    LinuxShell::new(Bridge::start().unwrap(), settings())
}

/// The settings the tests give.
fn settings() -> LinuxShellSettings {
    LinuxShellSettings {
        notifier: NotifierSettings::default(),
        autostart_dir: None,
        autostart_id: "eileen".to_owned(),
        program: PathBuf::from("/usr/bin/eileen"),
        deadline: DEFAULT_DEADLINE,
    }
}

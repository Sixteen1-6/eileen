//! The shell crate on the session's real services: `ShellHost` through `LinuxShell`, and the
//! clients behind it where the trait does not show what they did. On the system bus: logind,
//! `UPower` and power-profiles-daemon; on the session bus: the notification server and the
//! Inhibit portal. What the tests share: the shell they test, a runtime of the test's own with a
//! connection to the system bus, to see what logind did, and a guard that sets the power profile
//! back.
//!
//! - A test of the shell makes its own bridge with `Bridge::start()`, as the core makes one, and
//!   a test of a client gives it connections of the test's own. The profile runs the tests one at
//!   a time, since they share the session's services.
//! - The test's thread calls the shell as the core does, blocking, while the workers of a runtime
//!   of the test's own read its connection, so a signal sent meanwhile waits in its stream.
//! - No test waits for nothing to arrive. A signal check follows a round trip to its sender
//!   (`testing::round_trip`), and every wait is for something the test caused, with no timer of
//!   its own (`testing::next_message`, or a client stream's `next`): the profile's slow-timeout
//!   ends a wait that never ends.
//! - Streams and connections live and drop inside `Session::block_on`, within their runtime's
//!   context, a panicking test's included (the stream traps of `eileen-dbus-linux`).

use std::future::Future;
use std::path::PathBuf;

use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, system_bus};
use eileen_shell_linux::{LinuxShell, LinuxShellSettings, NotifierSettings};
use zbus::zvariant::Value;

mod keep_awake;
mod lock;
mod notifications;
mod power;
mod toast;

/// logind's name, its manager's object and the manager's interface.
const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

/// power-profiles-daemon's name, which is also its interface, and its object.
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// The lane's power profile, the daemon's default, which the tests find and leave.
const BALANCED: &str = "balanced";

/// The test's own connection to the system bus, apart from the shell's bridge, on a runtime of
/// its own whose two workers read it while the test's thread waits on the shell. Every test makes
/// one for that runtime, where a client's test runs the client; the tests of logind and the power
/// daemons use the connection too.
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

/// Switches power-profiles-daemon to `profile` with a `Set` of its `ActiveProfile`, as a
/// desktop's power menu does. polkit allows it to the active local session alone
/// (`org.freedesktop.UPower.PowerProfiles.switch-profile` is `allow_active`), as the lane's is.
async fn set_profile(system: &zbus::Connection, profile: &str) -> zbus::Result<()> {
    let property = (PROFILES, "ActiveProfile", Value::from(profile));
    let properties = Some("org.freedesktop.DBus.Properties");
    let reply = system.call_method(Some(PROFILES), PROFILES_PATH, properties, "Set", &property);
    reply.await.map(|_reply| ())
}

/// Sets the power profile back to `balanced` when it drops, a panicking test's included, so the
/// tests after one that switches the profile find the lane's. Make it before the switch and
/// outside `Session::block_on`: it calls through the session's runtime from the test's thread,
/// which the test's `block_on` has returned or unwound from by then, and since it borrows the
/// session it drops first. Best effort: a test that nextest's slow-timeout ends is killed, guard
/// and all, and no other test asserts on the profile.
struct Balanced<'s>(&'s Session);

impl Drop for Balanced<'_> {
    fn drop(&mut self) {
        let _ignored = self.0.block_on(set_profile(self.0.system(), BALANCED));
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

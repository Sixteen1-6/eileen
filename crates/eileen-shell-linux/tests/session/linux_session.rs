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

use std::cell::Cell;
use std::future::Future;
use std::path::PathBuf;

use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, system_bus};
use eileen_shell_linux::{LinuxShell, LinuxShellSettings, NotifierSettings, ShortcutNames};
use zbus::zvariant::{OwnedValue, Value};

mod keep_awake;
mod lock;
mod notifications;
mod power;
mod toast;

/// logind's name, its manager's object and the manager's interface.
const LOGIND: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

/// power-profiles-daemon's name, which is also its interface, its object, and the interface of
/// its properties.
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The test's own connection to the system bus, apart from the shell's bridge, on a runtime of
/// its own whose two workers read it while the test's thread waits on the shell. Each test that
/// runs async code makes one: a client's test runs the client on its runtime, and the tests of
/// logind and the power daemons use the connection too.
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
/// desktop's power menu does. `org.freedesktop.UPower.PowerProfiles.switch-profile` is
/// `allow_active`: polkit allows it to a caller in an active local session, as the tests in the
/// lane's are (the power tests' doc says which session polkit gives a process in none).
async fn set_profile(system: &zbus::Connection, profile: &str) -> zbus::Result<()> {
    let property = (PROFILES, "ActiveProfile", Value::from(profile));
    let reply =
        system.call_method(Some(PROFILES), PROFILES_PATH, Some(PROPERTIES), "Set", &property);
    reply.await.map(|_reply| ())
}

/// power-profiles-daemon's active profile, with a `Get` of its `ActiveProfile`.
async fn active_profile(system: &zbus::Connection) -> zbus::Result<String> {
    let property = (PROFILES, "ActiveProfile");
    let reply =
        system.call_method(Some(PROFILES), PROFILES_PATH, Some(PROPERTIES), "Get", &property);
    let value: OwnedValue = reply.await?.body().deserialize()?;
    Ok(String::try_from(value)?)
}

/// Sets the power profile back to the one a test found if it drops while armed, a panicking
/// test's included. The test arms it once it has switched away from that profile and disarms it
/// once it has switched back, so a test that fails before its switch changes nothing, and the
/// tests after one that fails mid-switch find the profile as it was. Make it outside
/// `Session::block_on`: it calls through the session's runtime from the test's thread, which the
/// test's `block_on` has returned or unwound from by then, and since it borrows the session it
/// drops first. Best effort: a test that nextest's slow-timeout ends is killed, guard and all, and
/// no other test asserts on the profile.
struct ProfileBack<'s> {
    session: &'s Session,
    /// The profile to set back, while armed.
    found: Cell<Option<String>>,
}

impl<'s> ProfileBack<'s> {
    /// A guard on the profile, not yet armed.
    fn new(session: &'s Session) -> Self {
        Self { session, found: Cell::new(None) }
    }

    /// Arms the guard with `found`, the profile the test has just switched away from.
    fn arm(&self, found: &str) {
        self.found.set(Some(found.to_owned()));
    }

    /// Disarms the guard once the test has switched back.
    fn disarm(&self) {
        self.found.set(None);
    }
}

impl Drop for ProfileBack<'_> {
    fn drop(&mut self) {
        if let Some(found) = self.found.take() {
            let _ignored = self.session.block_on(set_profile(self.session.system(), &found));
        }
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
        shortcut_names: ShortcutNames::default(),
    }
}

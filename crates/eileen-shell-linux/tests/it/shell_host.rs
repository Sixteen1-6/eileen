//! `ShellHost` on Linux: `LinuxShell` through a bridge on the private bus, against the fake
//! logind, logind session and notification server of the clients' own tests, and a GNOME session
//! manager that reports what it is asked. The test's thread calls the shell as the core does,
//! blocking; the fakes run on a runtime of their own, whose workers serve them meanwhile.

use std::fs;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use eileen_contracts::core_io::Toast;
use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, Names, name_owner, owner_changes};
use eileen_platform::{Deadline, PlatformError, ShellHost, TrayState, VirtualKey};
use eileen_shell_linux::{LinuxShell, LinuxShellSettings, NotifierSettings};
use tokio::sync::{Notify, mpsc};

use crate::autostart::Scratch;
use crate::keep_awake::{
    COOKIE, FakeGnome, FakeManager, GNOME, GNOME_PATH, LOGIND, MANAGER_PATH, left, still_held,
    watch_end,
};
use crate::notifications::{FakeServer, Hint, NAME, PATH};
use crate::session_lock::{FakeSession, SESSION_PATH};

/// The deadline of a call that cannot answer in time, its fake being silent: it passes whatever
/// its length, and a short one keeps the test short.
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));
/// The reason Eileen gives logind and the desktop for keeping the session awake.
const REASON: &str = "An armed run is in progress";
/// The settings' desktop entry and icon, which differ from the notifier's defaults.
const DESKTOP_ENTRY: &str = "org.eileen.Eileen";
const ICON: &str = "eileen-tray";
/// The program the autostart entry starts.
const PROGRAM: &str = "/opt/eileen/bin/eileen";

/// The fakes, on a multi-thread runtime of their own.
struct Fakes {
    runtime: tokio::runtime::Runtime,
    services: Vec<zbus::Connection>,
}

impl Fakes {
    fn new() -> Self {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build();
        Self { runtime: runtime.unwrap(), services: Vec::new() }
    }

    /// Serves `iface` at `path` under `name` until the fakes drop.
    fn serve<I: zbus::object_server::Interface>(
        &mut self,
        bus: &PrivateBus,
        name: &str,
        path: &str,
        iface: I,
    ) {
        let service = self.runtime.block_on(serve(bus, name, path, iface));
        self.services.push(service);
    }

    /// Serves logind: its `manager`, and `session` as the caller's.
    fn logind(&mut self, bus: &PrivateBus, manager: FakeManager, session: FakeSession) {
        let service = self.runtime.block_on(async {
            let service = serve(bus, LOGIND, MANAGER_PATH, manager).await;
            service.object_server().at(SESSION_PATH, session).await.unwrap();
            service
        });
        self.services.push(service);
    }
}

impl Drop for Fakes {
    fn drop(&mut self) {
        // The connections end inside their runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.services.clear();
    }
}

/// A GNOME session manager that reports each `Inhibit`'s app ID, window, reason and flags, and
/// each `Uninhibit`'s cookie. It answers `Inhibit` with [`COOKIE`] once its gate, if it has one,
/// opens.
#[derive(Debug, Clone)]
struct ReportingGnome {
    gate: Option<Arc<Notify>>,
    inhibits: mpsc::Sender<(String, u32, String, u32)>,
    uninhibits: mpsc::Sender<u32>,
}

/// What a [`ReportingGnome`] reports.
struct GnomeReports {
    inhibits: mpsc::Receiver<(String, u32, String, u32)>,
    uninhibits: mpsc::Receiver<u32>,
}

impl ReportingGnome {
    fn new(gate: Option<Arc<Notify>>) -> (Self, GnomeReports) {
        let (inhibits, inhibited) = mpsc::channel(8);
        let (uninhibits, uninhibited) = mpsc::channel(8);
        let reports = GnomeReports { inhibits: inhibited, uninhibits: uninhibited };
        (Self { gate, inhibits, uninhibits }, reports)
    }
}

#[zbus::interface(name = "org.gnome.SessionManager")]
impl ReportingGnome {
    async fn inhibit(&self, app_id: &str, toplevel_xid: u32, reason: &str, flags: u32) -> u32 {
        let _ignored =
            self.inhibits.send((app_id.to_owned(), toplevel_xid, reason.to_owned(), flags)).await;
        if let Some(gate) = &self.gate {
            gate.notified().await;
        }
        COOKIE
    }

    async fn uninhibit(&self, inhibit_cookie: u32) {
        let _ignored = self.uninhibits.send(inhibit_cookie).await;
    }
}

/// A notification server that never answers.
struct Silent;

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Silent {
    async fn get_capabilities(&self) -> Vec<String> {
        std::future::pending().await
    }
}

/// The settings the tests give: `deadline`, and the autostart folder `autostart_dir`.
fn settings(deadline: Deadline, autostart_dir: Option<PathBuf>) -> LinuxShellSettings {
    LinuxShellSettings {
        notifier: NotifierSettings {
            desktop_entry: DESKTOP_ENTRY.to_owned(),
            app_icon: ICON.to_owned(),
        },
        autostart_dir,
        autostart_id: "eileen".to_owned(),
        program: PathBuf::from(PROGRAM),
        deadline,
    }
}

/// A shell on a bridge to `bus`, with `deadline` and no autostart folder.
fn shell_on(bus: &PrivateBus, deadline: Deadline) -> LinuxShell {
    LinuxShell::new(Bridge::on_private_bus(bus).unwrap(), settings(deadline, None))
}

/// A toast about no job.
fn toast() -> Toast {
    Toast { title: "Build finished".to_owned(), body: "All 12 tests passed".to_owned(), job: None }
}

/// Waits until every copy of the caller's end of a logind lock has closed: logind's end, `watch`,
/// then reads end of file.
fn closed(mut watch: UnixStream) -> bool {
    watch.set_nonblocking(false).unwrap();
    let mut byte = [0_u8; 1];
    matches!(watch.read(&mut byte), Ok(0))
}

#[test]
fn a_toast_is_one_notification_named_as_the_settings_say() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let (server, log) = FakeServer::new(&["body"]);
    fakes.serve(&bus, NAME, PATH, server);
    shell_on(&bus, DEFAULT_DEADLINE).toast(&toast()).unwrap();
    let notifies = log.notifies();
    let [call] = notifies.as_slice() else { panic!("one Notify: {notifies:?}") };
    assert_eq!(
        (call.app_name.as_str(), call.summary.as_str(), call.body.as_str()),
        ("Eileen", "Build finished", "All 12 tests passed"),
        "Eileen's toast"
    );
    assert_eq!(call.app_icon, ICON, "the settings' icon");
    assert_eq!(
        call.hints.get("desktop-entry"),
        Some(&Hint::Text(DESKTOP_ENTRY.to_owned())),
        "the settings' desktop entry"
    );
}

#[test]
fn a_toast_the_server_never_answers_passes_the_deadline() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    fakes.serve(&bus, NAME, PATH, Silent);
    assert_eq!(
        shell_on(&bus, SHORT).toast(&toast()),
        Err(PlatformError::DeadlineExceeded { call: "toast" }),
        "the shell's deadline, not the connection's method timeout, which names the method"
    );
}

#[test]
fn a_toast_with_no_server_is_unavailable() {
    let bus = PrivateBus::start();
    assert_eq!(
        shell_on(&bus, DEFAULT_DEADLINE).toast(&toast()),
        Err(PlatformError::Unavailable { what: "org.freedesktop.Notifications.GetCapabilities" }),
        "the notifier's error, as its platform twin"
    );
}

#[test]
fn the_lock_state_and_the_lock_are_the_logind_sessions() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let session = FakeSession::default();
    fakes.logind(&bus, FakeManager::default(), session.clone());
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(shell.session_locked(), Ok(false), "unlocked");
    session.locked.store(true, Ordering::SeqCst);
    assert_eq!(shell.session_locked(), Ok(true), "locked, read afresh");
    shell.lock_workstation().unwrap();
    assert_eq!(session.locks.load(Ordering::SeqCst), 1, "Lock() was called once");
}

#[test]
fn without_logind_the_session_calls_are_unavailable() {
    let bus = PrivateBus::start();
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let unavailable = PlatformError::Unavailable { what: "logind session" };
    assert_eq!(shell.session_locked(), Err(unavailable.clone()), "no hint to read");
    assert_eq!(shell.lock_workstation(), Err(unavailable), "no session to lock");
}

#[test]
fn keep_awake_holds_sleep_and_idle_until_the_request_drops() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let manager = FakeManager::default();
    fakes.logind(&bus, manager.clone(), FakeSession::default());
    let (gnome, mut reports) = ReportingGnome::new(None);
    fakes.serve(&bus, GNOME, GNOME_PATH, gnome);
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let request = shell.keep_awake().unwrap();
    let lock =
        ("sleep:idle".to_owned(), "Eileen".to_owned(), REASON.to_owned(), "block".to_owned());
    assert_eq!(*manager.calls.lock().unwrap(), [lock], "logind's lock, with Eileen's reason");
    assert_eq!(
        reports.inhibits.try_recv(),
        Ok(("Eileen".to_owned(), 0, REASON.to_owned(), 12)),
        "GNOME's inhibitor of suspend and idle, with no window"
    );
    let mut watch = watch_end(&manager);
    assert!(still_held(&mut watch), "logind's lock holds while the request lives");
    drop(request);
    assert_eq!(reports.uninhibits.blocking_recv(), Some(COOKIE), "GNOME's inhibitor, by cookie");
    assert!(closed(watch), "logind's lock is closed too");
}

#[test]
fn the_request_keeps_the_bridge_after_the_shell_drops() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let manager = FakeManager::default();
    fakes.logind(&bus, manager.clone(), FakeSession::default());
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let request = shell.keep_awake().unwrap();
    let mut watch = watch_end(&manager);
    drop(shell);
    assert!(still_held(&mut watch), "the bridge lives on with the request, and the lock with it");
    drop(request);
    assert!(closed(watch), "dropping the request, the bridge's last handle, ends the lock");
}

#[test]
fn a_keep_awake_past_its_deadline_releases_what_it_took_once_it_holds() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let manager = FakeManager::default();
    fakes.logind(&bus, manager.clone(), FakeSession::default());
    let gate = Arc::new(Notify::new());
    let (gnome, mut reports) = ReportingGnome::new(Some(Arc::clone(&gate)));
    fakes.serve(&bus, GNOME, GNOME_PATH, gnome);
    let shell = shell_on(&bus, SHORT);
    assert_eq!(
        shell.keep_awake().unwrap_err(),
        PlatformError::DeadlineExceeded { call: "keep-awake request" },
        "GNOME has not answered"
    );
    gate.notify_one();
    assert_eq!(
        reports.uninhibits.blocking_recv(),
        Some(COOKIE),
        "the inhibitor GNOME granted after the call gave up is ended at once"
    );
    assert!(closed(watch_end(&manager)), "and logind's lock, taken before it");
}

#[test]
fn a_hold_that_uninhibit_cannot_end_ends_as_the_holders_connection_leaves_the_bus() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let gnome = FakeGnome { forget: true, ..FakeGnome::default() };
    fakes.serve(&bus, GNOME, GNOME_PATH, gnome.clone());
    // The shell keeps the bridge, and the bridge's shared connections, past the drop.
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    let request = shell.keep_awake().unwrap();
    let holder = gnome.calls.lock().unwrap()[0].sender.clone();
    let left_the_bus = fakes.runtime.block_on(async {
        let observer = bus.connect().await;
        let mut leaving = owner_changes(&observer, Names::Name(&holder)).await.unwrap();
        let owner = name_owner(&observer, &holder).await.unwrap();
        assert!(owner.is_some(), "the holder's connection is on the bus while the request lives");
        // This blocks the test's thread, not the fakes' workers, until the holder has released.
        drop(request);
        left(&mut leaving).await
    });
    assert_eq!(*gnome.uninhibits.lock().unwrap(), [COOKIE], "Uninhibit was asked, and failed");
    assert!(
        left_the_bus,
        "the connection GNOME's inhibitor was taken on left the bus all the same, which ends it"
    );
    drop(shell);
}

#[test]
fn run_at_sign_in_writes_and_removes_the_entry_in_the_settings_folder() {
    let bus = PrivateBus::start();
    let scratch = Scratch::new("shell_host");
    let folder = scratch.autostart();
    let bridge = Bridge::on_private_bus(&bus).unwrap();
    let shell = LinuxShell::new(bridge, settings(DEFAULT_DEADLINE, Some(folder.clone())));
    shell.set_run_at_sign_in(true).unwrap();
    let entry = fs::read_to_string(folder.join("eileen.desktop")).unwrap();
    assert!(entry.contains(&format!("\nExec=\"{PROGRAM}\"\n")), "the settings' program: {entry}");
    shell.set_run_at_sign_in(false).unwrap();
    assert!(!folder.join("eileen.desktop").exists(), "removed");
}

#[test]
fn run_at_sign_in_maps_its_errors_and_needs_a_folder() {
    let bus = PrivateBus::start();
    let scratch = Scratch::new("shell_host_errors");
    let mut relative = settings(DEFAULT_DEADLINE, Some(scratch.autostart()));
    relative.program = PathBuf::from("bin/eileen");
    let shell = LinuxShell::new(Bridge::on_private_bus(&bus).unwrap(), relative);
    assert_eq!(
        shell.set_run_at_sign_in(true),
        Err(PlatformError::Invalid { what: "program path", detail: "not absolute".to_owned() }),
        "the autostart error's platform twin"
    );
    assert_eq!(
        shell_on(&bus, DEFAULT_DEADLINE).set_run_at_sign_in(true),
        Err(PlatformError::Unavailable { what: "autostart folder" }),
        "no folder to write in"
    );
}

#[test]
fn hotkeys_the_tray_and_input_wait_for_phase_l2() {
    let bus = PrivateBus::start();
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(
        shell.register_hotkey("Ctrl+Alt+Space", 1),
        Err(PlatformError::Unavailable { what: "global hotkey" }),
        "the GlobalShortcuts portal"
    );
    assert_eq!(
        shell.set_tray(TrayState::default()),
        Err(PlatformError::Unavailable { what: "tray" }),
        "StatusNotifierItem"
    );
    let watch = shell.watch_input(VirtualKey(0x14), Box::new(|_event| {}));
    assert_eq!(
        watch.unwrap_err(),
        PlatformError::Unavailable { what: "input watch" },
        "the input portals"
    );
}

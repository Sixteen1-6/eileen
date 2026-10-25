//! `ShellHost` on Linux: `LinuxShell` through a bridge on the private bus, against the fake
//! logind, logind session, GNOME session manager, Inhibit portal, notification server and tray
//! watcher of the clients' own tests, and a GNOME session manager that reports what it is asked;
//! a silent bus stands in for a bus that has hung. The test's thread calls the shell as the core does,
//! blocking; the fakes run on a runtime of their own, whose workers serve them meanwhile.

use std::fs;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use eileen_contracts::core_io::Toast;
use eileen_dbus_linux::testing::{PrivateBus, SilentBus, serve};
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE, Names, name_owner, owner_changes};
use eileen_platform::{Deadline, PlatformError, ShellHost, TrayState, VirtualKey};
use eileen_shell_linux::{
    LinuxShell, LinuxShellEvent, LinuxShellSettings, NotifierSettings, TraySettings,
};
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;

use crate::autostart::Scratch;
use crate::keep_awake::{
    Answer, COOKIE, FakeGnome, FakeManager, FakePortal, GNOME, GNOME_PATH, LOGIND, MANAGER_PATH,
    PORTAL, PORTAL_PATH, left, respond, still_held, watch_end,
};
use crate::notifications::{FakeServer, Hint, NAME, PATH};
use crate::session_lock::{FakeSession, SESSION_PATH};
use crate::tray::{ITEM, ITEM_PATH, Rig, state, tray_item};

/// The deadline of a call that cannot answer in time, its fake being silent: it passes whatever
/// its length, and a short one keeps the test short. Not for `keep_awake`, whose connects have
/// half of it ([`GNOME_SILENCE`]).
const SHORT: Deadline = Deadline::after(Duration::from_millis(100));
/// The deadline of a keep-awake call that cannot answer in time, its GNOME session manager not
/// answering: it passes whatever its length, and half of it bounds the holder's connects, which
/// take about a millisecond on a private bus, so it leaves them room on a loaded machine.
const GNOME_SILENCE: Deadline = Deadline::after(Duration::from_millis(400));
/// The deadline of a keep-awake call whose silent portal spends its half, in real time: the
/// other half leaves room for the bus calls around the portal's wait.
const PORTAL_SILENCE: Deadline = Deadline::after(Duration::from_secs(1));
/// The deadline of a keep-awake call one of whose buses never answers its connect, which spends
/// the half, in real time: the other half leaves room for the calls on the bus that answers, a
/// portal's answer among them.
const BUS_SILENCE: Deadline = Deadline::after(Duration::from_millis(400));
/// The reason Eileen gives logind and the desktop for keeping the session awake.
const REASON: &str = "An armed run is in progress";
/// The settings' desktop entry and icon, which differ from the notifier's defaults.
pub(super) const DESKTOP_ENTRY: &str = "org.eileen.Eileen";
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
        let _ignored = self.serving(bus, name, path, iface);
    }

    /// Serves `iface` at `path` under `name`, as [`Self::serve`] does, and answers the service's
    /// connection, for a test that sends from it.
    fn serving<I: zbus::object_server::Interface>(
        &mut self,
        bus: &PrivateBus,
        name: &str,
        path: &str,
        iface: I,
    ) -> zbus::Connection {
        let service = self.runtime.block_on(serve(bus, name, path, iface));
        self.services.push(service.clone());
        service
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

/// The settings the tests give: `deadline`, the autostart folder `autostart_dir`, and the tray's
/// default icons.
pub(super) fn settings(deadline: Deadline, autostart_dir: Option<PathBuf>) -> LinuxShellSettings {
    LinuxShellSettings {
        notifier: NotifierSettings {
            desktop_entry: DESKTOP_ENTRY.to_owned(),
            app_icon: ICON.to_owned(),
        },
        autostart_dir,
        autostart_id: "eileen".to_owned(),
        program: PathBuf::from(PROGRAM),
        deadline,
        tray: TraySettings::default(),
    }
}

/// A shell on a bridge to `bus`, with `deadline` and no autostart folder.
fn shell_on(bus: &PrivateBus, deadline: Deadline) -> LinuxShell {
    LinuxShell::new(Bridge::on_private_bus(bus).unwrap(), settings(deadline, None))
}

/// A shell on a bridge whose session bus is at `session` and whose system bus is at `system`,
/// with [`BUS_SILENCE`] and no autostart folder.
fn shell_at(session: &str, system: &str) -> LinuxShell {
    LinuxShell::new(Bridge::on_addresses(session, system).unwrap(), settings(BUS_SILENCE, None))
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

/// Whether every copy of the caller's end of a logind lock has closed already: logind's end,
/// `watch`, which reads without waiting, reads end of file at once.
fn closed_now(watch: &mut UnixStream) -> bool {
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
    // The drop returns once the holder has the outcome of `release`, which closes logind's lock
    // and then waits for Uninhibit's reply, sent after the fake's report.
    assert_eq!(reports.uninhibits.try_recv(), Ok(COOKIE), "GNOME's inhibitor, by cookie, by then");
    assert!(closed_now(&mut watch), "and logind's lock is closed by then");
}

#[test]
fn a_silent_portal_has_half_the_deadline_so_loginds_lock_answers_in_time() {
    let bus = PrivateBus::start();
    let mut fakes = Fakes::new();
    let manager = FakeManager::default();
    fakes.logind(&bus, manager.clone(), FakeSession::default());
    // No GNOME session manager, and a portal that replies to Inhibit and never answers it.
    let (portal, mut reports) = FakePortal::reporting(Answer::Later);
    fakes.serve(&bus, PORTAL, PORTAL_PATH, portal);
    let shell = shell_on(&bus, PORTAL_SILENCE);
    let request = shell.keep_awake();
    assert!(request.is_ok(), "the call answers within its deadline: {request:?}");
    let mut watch = watch_end(&manager);
    assert!(still_held(&mut watch), "with logind's lock, which holds");
    assert!(
        reports.closes.blocking_recv().is_some(),
        "the portal's unconfirmed request was closed"
    );
    drop(request);
    assert!(closed_now(&mut watch), "the lock is closed once the request has dropped");
}

#[test]
fn a_session_bus_that_never_answers_leaves_loginds_lock_to_hold() {
    let bus = PrivateBus::start();
    let silent = SilentBus::start();
    let mut fakes = Fakes::new();
    let manager = FakeManager::default();
    fakes.logind(&bus, manager.clone(), FakeSession::default());
    let shell = shell_at(silent.address(), bus.address());
    let request = shell.keep_awake();
    assert!(request.is_ok(), "the call answers within its deadline: {request:?}");
    let mut watch = watch_end(&manager);
    assert!(still_held(&mut watch), "with logind's lock, which holds");
    drop(request);
    // Read to its end, not at once: with no desktop hold, the release makes no call that would
    // order this read after dbus-daemon and the fake have closed the copies they passed on.
    assert!(closed(watch), "the lock is closed once the request has dropped");
}

#[test]
fn a_system_bus_that_never_answers_leaves_the_desktops_hold() {
    let bus = PrivateBus::start();
    let silent = SilentBus::start();
    let mut fakes = Fakes::new();
    let (gnome, mut reports) = ReportingGnome::new(None);
    fakes.serve(&bus, GNOME, GNOME_PATH, gnome);
    let shell = shell_at(bus.address(), silent.address());
    let request = shell.keep_awake();
    assert!(request.is_ok(), "the call answers within its deadline: {request:?}");
    assert_eq!(
        reports.inhibits.try_recv(),
        Ok(("Eileen".to_owned(), 0, REASON.to_owned(), 12)),
        "with GNOME's inhibitor of suspend and idle"
    );
    drop(request);
    assert_eq!(
        reports.uninhibits.try_recv(),
        Ok(COOKIE),
        "GNOME's inhibitor is ended, by cookie, by the time the drop returns"
    );
}

#[test]
fn a_system_bus_that_never_answers_leaves_the_portal_time_to_answer() {
    let bus = PrivateBus::start();
    let silent = SilentBus::start();
    let mut fakes = Fakes::new();
    // No GNOME session manager, and a portal that replies to Inhibit and answers it later.
    let (portal, mut reports) = FakePortal::reporting(Answer::Later);
    let service = fakes.serving(&bus, PORTAL, PORTAL_PATH, portal);
    let shell = shell_at(bus.address(), silent.address());
    let (request, call) = std::thread::scope(|scope| {
        let request = scope.spawn(|| shell.keep_awake());
        // Once the fake has the call, as the real portal answers after its reply.
        let call = reports.calls.blocking_recv().unwrap();
        fakes.runtime.block_on(respond(&service, &call.request, 0));
        (request.join().unwrap(), call)
    });
    assert!(request.is_ok(), "the portal answered within its share of what was left: {request:?}");
    drop(request);
    assert_eq!(
        reports.closes.try_recv(),
        Ok(call.request),
        "the portal's request is closed by the time the drop returns"
    );
}

#[test]
fn a_system_bus_that_never_answers_leaves_the_portal_half_of_what_is_left() {
    let bus = PrivateBus::start();
    let silent = SilentBus::start();
    let mut fakes = Fakes::new();
    // No GNOME session manager, and a portal that replies to Inhibit and never answers it.
    let (portal, mut reports) = FakePortal::reporting(Answer::Later);
    fakes.serve(&bus, PORTAL, PORTAL_PATH, portal);
    let shell = shell_at(bus.address(), silent.address());
    let start = Instant::now();
    let error = shell.keep_awake().unwrap_err();
    let waited = start.elapsed();
    assert_eq!(
        error,
        PlatformError::DeadlineExceeded { call: "system bus" },
        "nothing holds, and logind's miss, late rather than absent, comes first, before the \
         call's own deadline"
    );
    assert!(
        waited >= BUS_SILENCE.budget() * 3 / 4,
        "the connects spent half the deadline and the portal waited half of what was left, so the \
         call answered no sooner than three quarters of it: {waited:?}"
    );
    assert!(
        reports.closes.blocking_recv().is_some(),
        "the portal's unconfirmed request was closed"
    );
}

#[test]
fn keep_awake_on_two_buses_that_never_answer_passes_its_deadline() {
    let silent = SilentBus::start();
    let shell = shell_at(silent.address(), silent.address());
    assert_eq!(
        shell.keep_awake().unwrap_err(),
        PlatformError::DeadlineExceeded { call: "system bus" },
        "both buses are there and never answered, so neither holder is absent: logind's miss, \
         once the half has passed, before the call's own deadline"
    );
}

#[test]
fn keep_awake_on_an_empty_bus_is_unavailable() {
    let bus = PrivateBus::start();
    assert_eq!(
        shell_on(&bus, DEFAULT_DEADLINE).keep_awake().unwrap_err(),
        PlatformError::Unavailable { what: "keep-awake holder" },
        "no logind, no GNOME session manager and no portal: nothing holds, and nothing refused"
    );
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
    let shell = shell_on(&bus, GNOME_SILENCE);
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
fn hotkeys_and_input_wait_for_phase_l2() {
    let bus = PrivateBus::start();
    let shell = shell_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(
        shell.register_hotkey("Ctrl+Alt+Space", 1),
        Err(PlatformError::Unavailable { what: "global hotkey" }),
        "the GlobalShortcuts portal"
    );
    let watch = shell.watch_input(VirtualKey(0x14), Box::new(|_event| {}));
    assert_eq!(
        watch.unwrap_err(),
        PlatformError::Unavailable { what: "input watch" },
        "the GlobalShortcuts portal, and a source of physical activity not chosen yet"
    );
}

#[test]
fn set_tray_shows_the_mute_state_and_menu_and_reports_choices() {
    let mut rig = Rig::new();
    let mut registrations = rig.watcher();
    let tray = TraySettings { icon: "eileen-on".to_owned(), muted_icon: "eileen-muted".to_owned() };
    let (shell, mut events) =
        rig.shell_with(LinuxShellSettings { tray, ..settings(DEFAULT_DEADLINE, None) });
    let muted = TrayState { muted: true, tooltip: "Muted".to_owned(), ..state() };
    assert_eq!(shell.set_tray(muted), Ok(()), "a watcher has the item");
    let (item, _sender) = rig.registered(&mut registrations);
    let icon = || String::try_from(rig.property(&item, ITEM_PATH, ITEM, "IconName"));
    assert_eq!(icon().unwrap(), "eileen-muted", "the settings' muted icon");
    let tooltip = rig.property(&item, ITEM_PATH, ITEM, "ToolTip");
    let (_icon, _pixmaps, _title, text) =
        <(String, Vec<(i32, i32, Vec<u8>)>, String, String)>::try_from(tooltip).unwrap();
    assert_eq!(text, "Muted", "the state's tooltip");
    shell.set_tray(state()).unwrap();
    assert_eq!(icon().unwrap(), "eileen-on", "the settings' icon");
    let (_revision, root) = rig.layout(&item, 0, -1);
    assert_eq!(root.labels(), ["Mute", "Quit"], "the state's menu");
    rig.menu_event(&item, root.children[0].id, "clicked").unwrap();
    assert_eq!(rig.event(&mut events), LinuxShellEvent::TrayChoice("mute".to_owned()), "chosen");
    assert!(registrations.try_recv().is_err(), "registered once");
}

#[test]
fn set_tray_without_a_watcher_is_unavailable_and_registers_once_one_appears() {
    let mut rig = Rig::new();
    let (shell, _events) = rig.shell();
    let menu = vec![tray_item("quit", "Quit", None)];
    let tray = TrayState { menu, ..state() };
    assert_eq!(
        shell.set_tray(tray.clone()),
        Err(PlatformError::Unavailable { what: "tray" }),
        "no watcher, as on GNOME without its AppIndicator extension"
    );
    let mut registrations = rig.watcher();
    let (item, _sender) = rig.registered(&mut registrations);
    assert_eq!(shell.set_tray(tray), Ok(()), "registered once the watcher came");
    let (_revision, root) = rig.layout(&item, 0, -1);
    assert_eq!(root.labels(), ["Quit"], "what the first call showed, kept");
}

//! [`LinuxShell`]: `eileen-platform`'s `ShellHost` on Linux (BUILD.md section 3.4; docs/LINUX.md
//! section 4). It is thin: each method converts types and errors and calls this crate's client
//! for it on `eileen-dbus-linux`'s [`Bridge`], whose thread makes, uses and drops every
//! connection. No `ShellHost` method takes a `Deadline`, so each D-Bus call waits at most the
//! settings' deadline, `eileen-dbus-linux`'s [`DEFAULT_DEADLINE`] unless a caller sets another;
//! past it the call answers `PlatformError::DeadlineExceeded`, naming the call.
//!
//! - `toast`: [`Notifier::notify`] on the session bus. A notifier lives for one toast, made on
//!   the bridge's thread from the session connection of the moment, so it asks the server's
//!   capabilities for every toast, one call more than a kept notifier would make.
//! - `keep_awake`: [`LinuxPowerRequest::keep_awake`], logind's lock on the system bus and the
//!   desktop's hold on the session bus, as the next section says.
//! - `lock_workstation` and `session_locked`: [`SessionLock`] on the system bus.
//! - `set_run_at_sign_in`: [`set_run_at_sign_in`] in the settings' autostart folder, with std
//!   file I/O on the caller's thread; with no folder it is `Unavailable`.
//! - `set_tray`: the `tray` module's `StatusNotifierItem`, kept by a holder task, as the
//!   section after next says.
//! - `register_hotkey` and `watch_input`: `Unavailable` until phase L2.
//!
//! # The power request
//!
//! A holder task on the bridge's thread takes the [`LinuxPowerRequest`] and keeps it. No deadline
//! cancels the task, since a call cut short could leave a hold that nothing ends, such as a GNOME
//! inhibitor whose cookie never came back: `keep_awake`'s one call starts it and waits for its
//! answer within the settings' deadline, and past it the bridge cancels only that wait. Once it
//! holds, the task answers with a channel and waits. When the `PowerRequest` drops, or at once
//! when the wait has already given up, it ends the holds with [`LinuxPowerRequest::release`].
//! The desktop's hold is taken on a session connection of the task's own
//! ([`Buses::unshared_session`]), as [`LinuxPowerRequest::keep_awake`] asks, which the task drops
//! on the bridge's thread once `release` has run: should `Uninhibit` or the portal's `Close`
//! fail, the desktop still ends its hold when that connection closes.
//!
//! The task connects the system bus and that session connection at once, within half the
//! deadline counted from the call's start, and gives the portal's answers half of what is then
//! left of it, which leaves the other half to the calls around the portal's wait, logind's
//! included. A bus not connected by the end of that first half is a holder that missed. Such a
//! bus is there and has hung, so it misses by `DeadlineExceeded` naming it, not as the absent
//! holder a bus that fails to connect is, and when nothing holds the call fails as
//! [`LinuxPowerRequest::keep_awake`] ranks the misses, never as `Unavailable`. So a bus that
//! accepts and never answers costs half the deadline, and logind's lock, or the desktop's hold,
//! the portal's included, still answers within it.
//!
//! Dropping the `PowerRequest` asks the task to release and waits at most [`RELEASE_WAIT`] for
//! it; past that, the task finishes on its own. Dropped on the bridge's thread, in a job or a
//! sink, it neither asks nor waits, since the bridge refuses a call made there: the task then
//! releases on its own once the release channel closes. The `PowerRequest` keeps a handle to the
//! bridge, so the hold outlives the [`LinuxShell`] that took it. When that handle is the bridge's
//! last, the bridge ends after the wait and drops the task with it: logind's descriptor and the
//! task's own session connection close, which ends both holds.
//!
//! # The tray
//!
//! The first `set_tray` starts a holder task on the bridge's thread, which keeps the item and its
//! menu on a session connection of its own ([`Buses::unshared_session`]). The holder shows that
//! call's state before it registers the item with the watcher, so that a host's first read has
//! it, and registers whenever the watcher's name has an owner: at once, and again each time the
//! name gets a new one, as when the watcher restarts. Each later `set_tray` asks the holder to
//! show its state, and every call waits for the holder's answer within the settings' deadline.
//! The mute state picks [`TraySettings`]' icon, the tooltip fills the item's, and the menu
//! becomes the dbusmenu items. The answer is `Ok` while a watcher has the item, and `Unavailable`
//! while none does, as on GNOME without its `AppIndicator` extension; the item stays on the bus
//! and registers once a watcher comes. A menu item the user
//! chooses reaches the sink of [`LinuxShell::with_events`] as [`LinuxShellEvent::TrayChoice`],
//! with its `TrayItem::id`. When the shell drops, the holder ends and drops its connection, so
//! the item leaves the bus and the watcher forgets it. A holder whose connection closed has ended
//! too, and the next `set_tray` starts another, under a new name.
//!
//! # Traps
//!
//! - zbus bounds no connect, neither its handshake nor its `Hello` call, and the task's own
//!   session connection costs one more connect per `keep_awake`. The task bounds both of its
//!   connects from the call's start, not from the job's, and the portal's budget from what is
//!   left once they are done, not from their half: a system bus that hangs spends the whole
//!   half, which would leave the portal no time for its `Response` on a desktop without GNOME's
//!   session manager.
//! - `keep_awake` fails only when nothing holds. A holder that missed
//!   ([`LinuxPowerRequest::missed`]), such as the desktop's on a desktop with neither GNOME's
//!   session manager nor the portal, goes unreported: the trait's `PowerRequest` has no place
//!   for it, and nothing logs it yet.
//! - Dropped off the bridge's thread as the bridge's last handle, a `PowerRequest` joins that
//!   thread, as a [`Subscription`] does ([`Bridge::watch`]). The join first ends a sink under
//!   way, so a sink must not lock what the dropping thread holds.
//! - The events sink runs on the bridge's thread: one that blocks stalls every call, and one
//!   that calls the shell is refused.
//! - A tray choice may not come from a person, as [`LinuxShellEvent::TrayChoice`] says.
//! - The tray holder registers in turn with its other work, so a watcher that never answers
//!   holds it for its connection's method timeout, and a `set_tray` made meanwhile can pass its
//!   deadline. A watcher that answers with an error leaves the item unregistered, as none would.
//!
//! [`DEFAULT_DEADLINE`]: eileen_dbus_linux::DEFAULT_DEADLINE

mod tray;

use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eileen_contracts::core_io::Toast;
use eileen_dbus_linux::{Bridge, Buses};
use eileen_platform::{
    Deadline, PlatformError, PowerRequest, ShellHost, ShellInputEvent, Subscription, TrayState,
    VirtualKey,
};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::{
    DbusError, LinuxPowerRequest, Notifier, NotifierSettings, SessionLock, TraySettings,
    set_run_at_sign_in,
};

/// How long dropping a `PowerRequest` waits for its holds to end, at most. A desktop that answers
/// does so in milliseconds, and a drop should not stall its caller for a call's whole deadline;
/// past it, the holder task ends the holds on its own.
pub const RELEASE_WAIT: Duration = Duration::from_secs(1);

/// The reason the desktop and `systemd-inhibit --list` show beside "Eileen" while it keeps the
/// session awake, which `ShellHost::keep_awake` does for an armed run (BUILD.md section 4.8).
const KEEP_AWAKE_REASON: &str = "An armed run is in progress";

/// What the calls' errors name.
const TOAST: &str = "toast";
const KEEP_AWAKE: &str = "keep-awake request";
const RELEASE: &str = "keep-awake release";
const LOCK: &str = "workstation lock";
const LOCKED: &str = "session lock state";
const AUTOSTART: &str = "autostart folder";
const HOTKEY: &str = "global hotkey";
const TRAY: &str = "tray";
const INPUT: &str = "input watch";
/// The buses, as a holder's miss names one the holder task could not reach.
const SYSTEM_BUS: &str = "system bus";
const SESSION_BUS: &str = "session bus";

/// The detail of a keep-awake call whose holder task ended without answering: it panicked, or
/// the bridge stopped.
const HOLDER_GONE: &str = "the holder ended without answering";

/// What the holder task answers once the holds have ended: whether the desktop agreed.
type Released = oneshot::Sender<Result<(), DbusError>>;
/// The channel through which a `PowerRequest` asks its holder task to release.
type Release = oneshot::Sender<Released>;

/// What a [`LinuxShell`] needs besides its bridge, each given by the composition root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxShellSettings {
    /// How toasts name Eileen.
    pub notifier: NotifierSettings,
    /// The folder `set_run_at_sign_in` writes the entry in: [`crate::default_autostart_dir`] on
    /// a real session. `None`, as where there is no home folder, makes it `Unavailable`.
    pub autostart_dir: Option<PathBuf>,
    /// The autostart entry's desktop file ID, without `.desktop`, such as `eileen`.
    pub autostart_id: String,
    /// The program the entry starts: Eileen's own executable, an absolute path, checked when the
    /// entry is written.
    pub program: PathBuf,
    /// How long each D-Bus call may take: `eileen_dbus_linux::DEFAULT_DEADLINE`, as for any trait
    /// method that takes none, unless a caller, such as a test, sets a shorter one.
    pub deadline: Deadline,
    /// The tray's icons. The tray names Eileen by `notifier`'s desktop entry.
    pub tray: TraySettings,
}

/// What the shell reports beyond `ShellHost`'s answers, to the sink of
/// [`LinuxShell::with_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxShellEvent {
    /// The user chose the tray menu's item with this `TrayItem::id` (dbusmenu's `clicked`).
    ///
    /// Nothing says that a person chose it. Any program on the session bus may send the menu the
    /// same event, and the item cannot tell a device's click from it, so a choice cannot meet the
    /// rule that the shell emits `Approve`, `Deny` and an unmuting `SetMute` or `ToggleMute` from
    /// the tray only when the click came from a device (BUILD.md section 3.3, beside
    /// `UserInput`): a root must not map one to them until the owner decides how Linux meets it.
    TrayChoice(String),
}

/// The composition root's sink for [`LinuxShellEvent`]s, shown by its type alone.
#[derive(Clone)]
struct EventSink(Arc<dyn Fn(LinuxShellEvent) + Send + Sync>);

impl fmt::Debug for EventSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EventSink")
    }
}

/// `ShellHost` on Linux, over a [`Bridge`]; the module's doc says what each method uses.
#[derive(Debug)]
pub struct LinuxShell {
    bridge: Bridge,
    settings: LinuxShellSettings,
    /// The tray holder's asks, once `set_tray` has started one.
    tray: Mutex<Option<mpsc::Sender<tray::Ask>>>,
    /// Where the shell's own events go; with none, they are dropped.
    events: Option<EventSink>,
}

impl LinuxShell {
    /// A shell on `bridge`, a clone of the process's one bridge, as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub const fn new(bridge: Bridge, settings: LinuxShellSettings) -> Self {
        Self { bridge, settings, tray: Mutex::new(None), events: None }
    }

    /// The shell, with `sink` for what no `ShellHost` method returns: the tray menu's choices
    /// ([`LinuxShellEvent`]). The sink runs on the bridge's thread, so it must not block, nor
    /// call the shell, which the bridge refuses there.
    #[must_use]
    pub fn with_events(mut self, sink: Arc<dyn Fn(LinuxShellEvent) + Send + Sync>) -> Self {
        self.events = Some(EventSink(sink));
        self
    }

    /// The tray holder's asks, and, when there is no holder yet or the last has ended, a new one
    /// to start on them.
    fn tray(&self) -> (mpsc::Sender<tray::Ask>, Option<tray::Holder>) {
        let mut held = self.tray.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(asks) = held.as_ref().filter(|asks| !asks.is_closed()) {
            return (asks.clone(), None);
        }
        let (asks, taken) = mpsc::channel(tray::ASKS);
        *held = Some(asks.clone());
        drop(held);
        let LinuxShellSettings { notifier, tray: settings, .. } = &self.settings;
        let (id, settings) = (notifier.desktop_entry.clone(), settings.clone());
        (asks, Some(tray::Holder { asks: taken, id, settings, sink: self.events.clone() }))
    }
}

impl ShellHost for LinuxShell {
    /// `Unavailable` until phase L2 brings global shortcuts through the `GlobalShortcuts` portal.
    fn register_hotkey(&self, _chord: &str, _id: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable { what: HOTKEY })
    }

    /// Shows `state` as Eileen's `StatusNotifierItem`, through the tray holder; the module's doc
    /// says how. `Unavailable` while no watcher has the item.
    fn set_tray(&self, state: TrayState) -> Result<(), PlatformError> {
        let (asks, holder) = self.tray();
        self.bridge
            .call(TRAY, self.settings.deadline, move |buses| tray::show(buses, asks, holder, state))
    }

    /// [`Notifier::notify`] on the session bus. The notification's id is dropped: no trait
    /// method withdraws a toast.
    fn toast(&self, toast: &Toast) -> Result<(), PlatformError> {
        let toast = toast.clone();
        let settings = self.settings.notifier.clone();
        self.bridge.call(TOAST, self.settings.deadline, move |buses| async move {
            let session = buses.session().await?;
            Notifier::new(&session, &settings).notify(&toast).await?;
            Ok(())
        })
    }

    /// `Unavailable` until phase L2, for which docs/LINUX.md section 4 plans the dictation key
    /// through the `GlobalShortcuts` portal; the source of physical activity is not chosen yet.
    fn watch_input(
        &self,
        _dictation_key: VirtualKey,
        _sink: Box<dyn Fn(ShellInputEvent) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(PlatformError::Unavailable { what: INPUT })
    }

    /// [`set_run_at_sign_in`] with the settings' folder, ID and program.
    fn set_run_at_sign_in(&self, on: bool) -> Result<(), PlatformError> {
        let LinuxShellSettings { autostart_dir, autostart_id, program, .. } = &self.settings;
        let dir = autostart_dir.as_deref().ok_or(PlatformError::Unavailable { what: AUTOSTART })?;
        Ok(set_run_at_sign_in(dir, autostart_id, program, on)?)
    }

    /// [`LinuxPowerRequest::keep_awake`], held by a task on the bridge's thread until the
    /// returned request drops; the module's doc says how.
    fn keep_awake(&self) -> Result<PowerRequest, PlatformError> {
        // The call starts here, on the caller's thread: the holder's bounds count from now, not
        // from when the bridge's thread takes the job.
        let start = Instant::now();
        let deadline = self.settings.deadline;
        let job = move |buses| async move {
            let (answer, answered) = oneshot::channel();
            drop(tokio::task::spawn_local(hold(buses, start, deadline, answer)));
            // Cancelled, the job drops `answered`, which makes the holder release what it took.
            answered.await.unwrap_or_else(|_gone| {
                Err(PlatformError::Io { what: KEEP_AWAKE, detail: HOLDER_GONE.to_owned() })
            })
        };
        let release = self.bridge.call(KEEP_AWAKE, deadline, job)?;
        Ok(released_on_drop(self.bridge.clone(), release))
    }

    /// [`SessionLock::lock_workstation`] on the system bus.
    fn lock_workstation(&self) -> Result<(), PlatformError> {
        self.bridge.call(LOCK, self.settings.deadline, |buses| async move {
            Ok(SessionLock::new(&buses.system().await?).lock_workstation().await?)
        })
    }

    /// [`SessionLock::session_locked`] on the system bus.
    fn session_locked(&self) -> Result<bool, PlatformError> {
        self.bridge.call(LOCKED, self.settings.deadline, |buses| async move {
            Ok(SessionLock::new(&buses.system().await?).session_locked().await?)
        })
    }
}

/// How long the holder's connects may take, which zbus bounds by no timeout: half the call's
/// deadline, counted from its start, so that a bus that never finishes connecting is a missed
/// holder and the call still answers in time with the other holder.
fn connect_budget(deadline: Deadline) -> Duration {
    deadline.budget() / 2
}

/// How long the portal's answers may take, which no method timeout bounds, in a call that ends
/// at `end` (`None` for never): half of what is left of the call once the connects are done, so
/// that a portal that never answers is a missed holder, and the other half leaves the calls
/// around the portal's wait, logind's included, time to answer within the deadline.
fn portal_budget(end: Option<Instant>) -> Duration {
    end.map_or(Duration::MAX, |end| end.saturating_duration_since(Instant::now()) / 2)
}

/// The holder task: takes the request on `buses` within `deadline`, counted from `start`, the
/// call's start, and answers with a channel to release it through. It releases the request once
/// asked through that channel, or once the channel or the wait for the answer is gone without
/// asking.
async fn hold(
    buses: Buses,
    start: Instant,
    deadline: Deadline,
    answer: oneshot::Sender<Result<Release, PlatformError>>,
) {
    // A bus not reached by `until` is a holder that `keep_awake` names as missed: absent when its
    // connect failed, late when the connect had not answered by then. The two connect at once,
    // so a bus that hangs costs no more than the bound. `None`, past what an instant holds, is no
    // bound. The session connection is the holder's own: the request keeps it, and it closes
    // here once `release` has run, or with the task when the bridge ends first, which ends the
    // desktop's hold even when `release` failed.
    let until = start.checked_add(connect_budget(deadline));
    let (system, session) = tokio::join!(
        within(until, SYSTEM_BUS, buses.system()),
        within(until, SESSION_BUS, buses.unshared_session()),
    );
    drop(buses);
    let portal = portal_budget(start.checked_add(deadline.budget()));
    let system = system.as_ref().map_err(Clone::clone);
    let taken = LinuxPowerRequest::keep_awake(system, session, KEEP_AWAKE_REASON, portal).await;
    let request = match taken {
        Ok(request) => request,
        Err(error) => {
            let _ignored = answer.send(Err(error.into()));
            return;
        }
    };
    let (release, asked) = oneshot::channel();
    // `None` when the call or the `PowerRequest` went without asking: release at once.
    let released = if answer.send(Ok(release)).is_ok() { asked.await.ok() } else { None };
    let outcome = request.release().await;
    if let Some(released) = released {
        let _ignored = released.send(outcome);
    }
}

/// What `connect` answers by `until` (`None` for no bound), its errors naming `bus` as
/// [`LinuxPowerRequest::keep_awake`] takes them: [`DbusError::Unavailable`] when it fails, as for
/// a bus that is not there, and [`DbusError::DeadlineExceeded`] when it has not answered by
/// then, as for a bus that is there and has hung. A connect cut short drops here, on the
/// bridge's thread and inside its runtime, which closes its socket.
async fn within<T>(
    until: Option<Instant>,
    bus: &'static str,
    connect: impl Future<Output = Result<T, DbusError>>,
) -> Result<T, DbusError> {
    let connected = match until {
        Some(until) => tokio::time::timeout_at(until, connect)
            .await
            .map_err(|_late| DbusError::DeadlineExceeded { call: bus })?,
        None => connect.await,
    };
    connected.map_err(|_failed| DbusError::Unavailable { what: bus })
}

/// The `PowerRequest` of a hold. Dropped, it asks the holder task to release through `release`
/// and waits at most [`RELEASE_WAIT`] for the holds to end; it keeps `bridge` until then.
fn released_on_drop(bridge: Bridge, release: Release) -> PowerRequest {
    PowerRequest::new(move || {
        // Nothing to tell the dropping thread: past the wait, the holds end without it.
        let _ignored = bridge.call(RELEASE, Deadline::after(RELEASE_WAIT), |_buses| async move {
            let (released, outcome) = oneshot::channel();
            // A holder that is gone has released already.
            let _ignored = release.send(released);
            let _ignored = outcome.await;
            Ok(())
        });
    })
}

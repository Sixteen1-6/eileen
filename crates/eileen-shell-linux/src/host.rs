//! [`LinuxShell`]: `eileen-platform`'s `ShellHost` on Linux (BUILD.md section 3.4; docs/LINUX.md
//! section 4). It is thin: each method converts types and errors and calls this crate's client
//! for it on `eileen-dbus-linux`'s [`Bridge`], whose thread makes, uses and drops every
//! connection. No `ShellHost` method that calls D-Bus takes a `Deadline`, so each D-Bus call
//! waits at most the settings' deadline, `eileen-dbus-linux`'s [`DEFAULT_DEADLINE`] unless a
//! caller sets another; past it the call answers `PlatformError::DeadlineExceeded`, naming the
//! call. `set_run_at_sign_in` takes the caller's, for its file write, and does not enforce it
//! yet.
//!
//! - `toast`: [`Notifier::notify`] on the session bus. A notifier lives for one toast, made on
//!   the bridge's thread from the session connection of the moment, so it asks the server's
//!   capabilities for every toast, one call more than a kept notifier would make.
//! - `keep_awake`: [`LinuxPowerRequest::keep_awake`], logind's lock on the system bus and the
//!   desktop's hold on the session bus, as the next section says.
//! - `lock_workstation` and `session_locked`: [`SessionLock`] on the system bus.
//! - `set_run_at_sign_in`: [`set_run_at_sign_in`] in the settings' autostart folder, with std
//!   file I/O on the caller's thread, so a hung home folder holds that thread past the caller's
//!   deadline until WP10 moves the write onto a helper thread (BUILD.md section 3.4); with no
//!   folder it is `Unavailable`.
//! - `register_hotkey`, `set_tray` and `watch_input`: `Unavailable` until phase L2.
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
//! Dropping the `PowerRequest` asks the task to release and waits at most [`RELEASE_WAIT`] for
//! it; past that, the task finishes on its own. Dropped on the bridge's thread, in a job or a
//! sink, it neither asks nor waits, since the bridge refuses a call made there: the task then
//! releases on its own once the release channel closes. The `PowerRequest` keeps a handle to the
//! bridge, so the hold outlives the [`LinuxShell`] that took it. When that handle is the bridge's
//! last, the bridge ends after the wait and drops the task with it: logind's descriptor and the
//! task's own session connection close, which ends both holds.
//!
//! # Traps
//!
//! - The task's own session connection costs one more connect per `keep_awake`: a handshake
//!   and a `Hello` call, which zbus bounds by no timeout. A session bus that accepts and never
//!   answers delays only the task, never the call, which still answers within its deadline.
//! - `keep_awake` fails only when nothing holds. A holder that missed
//!   ([`LinuxPowerRequest::missed`]), such as the desktop's on a desktop with neither GNOME's
//!   session manager nor the portal, goes unreported: the trait's `PowerRequest` has no place
//!   for it, and nothing logs it yet.
//! - Dropped off the bridge's thread as the bridge's last handle, a `PowerRequest` joins that
//!   thread, as a [`Subscription`] does ([`Bridge::watch`]). The join first ends a sink under
//!   way, so a sink must not lock what the dropping thread holds.
//!
//! [`DEFAULT_DEADLINE`]: eileen_dbus_linux::DEFAULT_DEADLINE

use std::path::PathBuf;
use std::time::Duration;

use eileen_contracts::core_io::Toast;
use eileen_dbus_linux::{Bridge, Buses};
use eileen_platform::{
    Deadline, PlatformError, PowerRequest, ShellHost, ShellInputEvent, Subscription, TrayState,
    VirtualKey,
};
use tokio::sync::oneshot;

use crate::{
    DbusError, LinuxPowerRequest, Notifier, NotifierSettings, SessionLock, set_run_at_sign_in,
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
}

/// `ShellHost` on Linux, over a [`Bridge`]; the module's doc says what each method uses.
#[derive(Debug)]
pub struct LinuxShell {
    bridge: Bridge,
    settings: LinuxShellSettings,
}

impl LinuxShell {
    /// A shell on `bridge`, a clone of the process's one bridge, as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub const fn new(bridge: Bridge, settings: LinuxShellSettings) -> Self {
        Self { bridge, settings }
    }
}

impl ShellHost for LinuxShell {
    /// `Unavailable` until phase L2 brings global shortcuts through the `GlobalShortcuts` portal.
    fn register_hotkey(&self, _chord: &str, _id: u32) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable { what: HOTKEY })
    }

    /// `Unavailable` until phase L2 brings the tray as a `StatusNotifierItem`.
    fn set_tray(&self, _state: TrayState) -> Result<(), PlatformError> {
        Err(PlatformError::Unavailable { what: TRAY })
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

    /// [`set_run_at_sign_in`] with the settings' folder, ID and program, on the caller's thread.
    /// It does not enforce the caller's deadline yet: section 3.4 of BUILD.md asks for a helper
    /// thread, abandoned past it, which WP10 adds.
    fn set_run_at_sign_in(&self, on: bool, _deadline: Deadline) -> Result<(), PlatformError> {
        let LinuxShellSettings { autostart_dir, autostart_id, program, .. } = &self.settings;
        let dir = autostart_dir.as_deref().ok_or(PlatformError::Unavailable { what: AUTOSTART })?;
        Ok(set_run_at_sign_in(dir, autostart_id, program, on)?)
    }

    /// [`LinuxPowerRequest::keep_awake`], held by a task on the bridge's thread until the
    /// returned request drops; the module's doc says how.
    fn keep_awake(&self) -> Result<PowerRequest, PlatformError> {
        let portal = portal_budget(self.settings.deadline);
        let job = move |buses| async move {
            let (answer, answered) = oneshot::channel();
            drop(tokio::task::spawn_local(hold(buses, portal, answer)));
            // Cancelled, the job drops `answered`, which makes the holder release what it took.
            answered.await.unwrap_or_else(|_gone| {
                Err(PlatformError::Io { what: KEEP_AWAKE, detail: HOLDER_GONE.to_owned() })
            })
        };
        let release = self.bridge.call(KEEP_AWAKE, self.settings.deadline, job)?;
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

/// How long the portal's answers may take, which no method timeout bounds: half the call's
/// deadline, so that a portal that never answers is a missed holder and the call still answers
/// in time with logind's lock.
fn portal_budget(deadline: Deadline) -> Duration {
    deadline.budget() / 2
}

/// The holder task: takes the request on `buses`, with `portal` for the portal's answers, and
/// answers with a channel to release it through. It releases the request once asked through that
/// channel, or once the channel or the wait for the answer is gone without asking.
async fn hold(
    buses: Buses,
    portal: Duration,
    answer: oneshot::Sender<Result<Release, PlatformError>>,
) {
    // A bus that cannot be reached is `None`, a holder that `keep_awake` names as missed. The
    // session connection is the holder's own: the request keeps it, and it closes here once
    // `release` has run, or with the task when the bridge ends first, which ends the desktop's
    // hold even when `release` failed.
    let system = buses.system().await.ok();
    let session = buses.unshared_session().await.ok();
    drop(buses);
    let taken =
        LinuxPowerRequest::keep_awake(system.as_ref(), session, KEEP_AWAKE_REASON, portal).await;
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

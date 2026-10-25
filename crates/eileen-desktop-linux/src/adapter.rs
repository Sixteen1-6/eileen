//! [`LinuxDesktop`]: the desktop process's platform traits on Linux over `eileen-dbus-linux`'s
//! [`Bridge`], so far `Grounding` (`src/grounding.rs`).
//!
//! Each call is one job on the bridge's thread. The job takes the session bus there, asks
//! `org.a11y.Bus` for the accessibility bus, connects to it for itself alone with the settings'
//! call timeout, and answers plain data, so that connection is made, used and dropped on the
//! bridge's thread. That costs a connect per call, until a kept connection to the accessibility
//! bus joins the bridge's `Buses`. A method that takes no `Deadline` waits at most the settings'
//! deadline; past a deadline the call answers `PlatformError::DeadlineExceeded`, naming the call.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use eileen_contracts::desktop::WindowInfo;
use eileen_dbus_linux::Bridge;
use eileen_platform::{Deadline, PlatformError, TreeLimits};

use crate::atspi::{Atspi, accessibility_bus};
use crate::toplevel::{KnownWindow, toplevels, window_infos};

/// How much of the foreground window's tree a focus search reads in the product: 24 levels and
/// 2,000 objects, twice as deep as a tree read's default and four times as wide, since the focus
/// can sit deep in a web page.
pub const FOCUS_LIMITS: TreeLimits = TreeLimits { max_depth: 24, max_nodes: 2_000 };

/// What a [`LinuxDesktop`] needs besides its bridge, each given by the composition root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxDesktopSettings {
    /// How long a call that takes no `Deadline` may take: `eileen_dbus_linux::DEFAULT_DEADLINE`,
    /// as for any trait method that takes none, unless a caller, such as a test, sets another.
    pub deadline: Deadline,
    /// How long each call on the accessibility bus may take: [`crate::CALL_TIMEOUT`] in the
    /// product.
    pub call_timeout: Duration,
    /// The names of Eileen's own processes, as their executables are named (`eileen-hud`): their
    /// windows are `is_eileen`.
    pub own_processes: Vec<String>,
    /// How much of the foreground window's tree a focus search reads: [`FOCUS_LIMITS`] in the
    /// product.
    pub focus_limits: TreeLimits,
}

/// The desktop process's traits on Linux, over a [`Bridge`]; the module's doc says how each call
/// runs, and `Grounding`'s methods what each answers.
#[derive(Debug)]
pub struct LinuxDesktop {
    bridge: Bridge,
    settings: LinuxDesktopSettings,
    /// The windows the last listing found, by id.
    windows: Mutex<BTreeMap<i64, KnownWindow>>,
}

impl LinuxDesktop {
    /// A desktop on `bridge`, a clone of the process's one bridge, as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub fn new(bridge: Bridge, settings: LinuxDesktopSettings) -> Self {
        Self { bridge, settings, windows: Mutex::default() }
    }

    /// The window the last listing, by `Grounding::windows` or `foreground`, found under `hwnd`,
    /// minimized or not; `None` for an id it did not list.
    #[must_use]
    pub fn known_window(&self, hwnd: i64) -> Option<KnownWindow> {
        self.known().get(&hwnd).cloned()
    }

    /// The settings it was made with.
    pub(crate) const fn settings(&self) -> &LinuxDesktopSettings {
        &self.settings
    }

    /// Runs `job` on the bridge within `deadline`, given a client of the accessibility bus that
    /// it alone uses, and answers what it answers; errors name `call`.
    pub(crate) fn on_bus<T, F, Fut>(
        &self,
        call: &'static str,
        deadline: Deadline,
        job: F,
    ) -> Result<T, PlatformError>
    where
        F: FnOnce(Atspi) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, PlatformError>> + 'static,
        T: Send + 'static,
    {
        let call_timeout = self.settings.call_timeout;
        self.bridge.call(call, deadline, move |buses| async move {
            let session = buses.session().await?;
            job(accessibility_bus(&session, call_timeout).await?).await
        })
    }

    /// Every window the registry's apps list, front to back, minimized ones included, within the
    /// settings' deadline; errors name `call`. The listing replaces the windows
    /// [`Self::known_window`] knows. The processes are named here, on the caller's thread, so
    /// the bridge's thread reads no file.
    pub(crate) fn list(&self, call: &'static str) -> Result<Vec<WindowInfo>, PlatformError> {
        let found = self.on_bus(call, self.settings.deadline, |atspi| async move {
            Ok(toplevels(&atspi).await?)
        })?;
        let listed = window_infos(found, &self.settings.own_processes);
        *self.known() = listed.iter().map(|(info, known)| (info.hwnd, known.clone())).collect();
        Ok(listed.into_iter().map(|(info, _known)| info).collect())
    }

    /// The windows the last listing found. A panic while the lock was held cannot leave them half
    /// written, since a listing replaces them in one assignment, so a poisoned lock is used as it
    /// is.
    fn known(&self) -> MutexGuard<'_, BTreeMap<i64, KnownWindow>> {
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

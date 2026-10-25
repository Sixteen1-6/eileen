//! The tray, hotkeys, toasts, Raw Input and the session.

use eileen_contracts::core_io::{KeyEdge, Toast};

use crate::{PlatformError, PowerRequest, Subscription};

/// A Windows virtual-key code (`VK_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VirtualKey(pub u16);

/// One item of the tray menu; the menu is data, the shell draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItem {
    /// What a click reports back.
    pub id: String,
    /// What the menu shows.
    pub label: String,
    /// A check mark, for toggles.
    pub checked: Option<bool>,
}

/// What the tray shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    /// The microphone is muted.
    pub muted: bool,
    /// The tooltip.
    pub tooltip: String,
    /// The menu, top to bottom.
    pub menu: Vec<TrayItem>,
}

/// What the Raw Input sink reports: the configured dictation key and physical activity,
/// nothing else (check 25 holds the Raw Input modules to it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellInputEvent {
    /// The dictation key went down or up.
    DictationKey(KeyEdge),
    /// The user moved a physical device; carries no key and no position.
    PhysicalActivity,
}

/// The core's shell thread (`eileen-shell-win`).
pub trait ShellHost: Send + Sync {
    /// Registers a global hotkey; fails if another app owns the chord.
    fn register_hotkey(&self, chord: &str, id: u32) -> Result<(), PlatformError>;
    /// Shows the tray state.
    fn set_tray(&self, state: TrayState) -> Result<(), PlatformError>;
    /// Shows a toast.
    fn toast(&self, toast: &Toast) -> Result<(), PlatformError>;
    /// Raw Input sink: edges of the configured dictation key and a timestamp of the last
    /// physical input, nothing else.
    fn watch_input(
        &self,
        dictation_key: VirtualKey,
        sink: Box<dyn Fn(ShellInputEvent) + Send + Sync>,
    ) -> Result<Subscription, PlatformError>;
    /// Adds or removes the `HKCU` Run entry.
    fn set_run_at_sign_in(&self, on: bool) -> Result<(), PlatformError>;
    /// A display-required and system-required power request for an armed run.
    fn keep_awake(&self) -> Result<PowerRequest, PlatformError>;
    /// Locks the workstation, at the end of an armed run.
    fn lock_workstation(&self) -> Result<(), PlatformError>;
    /// Whether the session is locked.
    fn session_locked(&self) -> Result<bool, PlatformError>;
}

#[cfg(any(test, feature = "fake"))]
pub use fake::FakeShellHost;

#[cfg(any(test, feature = "fake"))]
mod fake {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use eileen_contracts::core_io::Toast;

    use super::{ShellHost, ShellInputEvent, TrayState, VirtualKey};
    use crate::fake_util::{Sinks, lock};
    use crate::{PlatformError, PowerRequest, Subscription};

    /// A shell that records what it was asked to show.
    #[derive(Debug)]
    pub struct FakeShellHost {
        hotkeys: Mutex<BTreeMap<String, u32>>,
        tray: Mutex<Option<TrayState>>,
        toasts: Mutex<Vec<Toast>>,
        input: Sinks<ShellInputEvent>,
        run_at_sign_in: AtomicBool,
        locked: AtomicBool,
        power_requests: Arc<AtomicU32>,
    }

    impl Default for FakeShellHost {
        fn default() -> Self {
            Self {
                hotkeys: Mutex::default(),
                tray: Mutex::default(),
                toasts: Mutex::default(),
                input: Sinks::new(),
                run_at_sign_in: AtomicBool::new(false),
                locked: AtomicBool::new(false),
                power_requests: Arc::new(AtomicU32::new(0)),
            }
        }
    }

    impl FakeShellHost {
        /// Delivers a Raw Input event to every watcher.
        pub fn send_input(&self, event: ShellInputEvent) {
            self.input.emit(&event);
        }

        /// How many input watchers are subscribed.
        #[must_use]
        pub fn watchers(&self) -> usize {
            self.input.len()
        }

        /// The last tray state shown.
        #[must_use]
        pub fn tray(&self) -> Option<TrayState> {
            lock(&self.tray).clone()
        }

        /// The toasts shown, in order.
        #[must_use]
        pub fn toasts(&self) -> Vec<Toast> {
            lock(&self.toasts).clone()
        }

        /// How many power requests are held now.
        #[must_use]
        pub fn power_requests(&self) -> u32 {
            self.power_requests.load(Ordering::SeqCst)
        }

        /// Whether the Run entry is present.
        #[must_use]
        pub fn runs_at_sign_in(&self) -> bool {
            self.run_at_sign_in.load(Ordering::SeqCst)
        }
    }

    impl ShellHost for FakeShellHost {
        fn register_hotkey(&self, chord: &str, id: u32) -> Result<(), PlatformError> {
            let mut hotkeys = lock(&self.hotkeys);
            if hotkeys.contains_key(chord) {
                return Err(PlatformError::Refused {
                    what: "hotkey",
                    reason: "already registered",
                });
            }
            hotkeys.insert(chord.to_owned(), id);
            drop(hotkeys);
            Ok(())
        }

        fn set_tray(&self, state: TrayState) -> Result<(), PlatformError> {
            *lock(&self.tray) = Some(state);
            Ok(())
        }

        fn toast(&self, toast: &Toast) -> Result<(), PlatformError> {
            lock(&self.toasts).push(toast.clone());
            Ok(())
        }

        fn watch_input(
            &self,
            _dictation_key: VirtualKey,
            sink: Box<dyn Fn(ShellInputEvent) + Send + Sync>,
        ) -> Result<Subscription, PlatformError> {
            Ok(self.input.add(sink))
        }

        fn set_run_at_sign_in(&self, on: bool) -> Result<(), PlatformError> {
            self.run_at_sign_in.store(on, Ordering::SeqCst);
            Ok(())
        }

        fn keep_awake(&self) -> Result<PowerRequest, PlatformError> {
            self.power_requests.fetch_add(1, Ordering::SeqCst);
            let held = Arc::clone(&self.power_requests);
            Ok(PowerRequest::new(move || {
                held.fetch_sub(1, Ordering::SeqCst);
            }))
        }

        fn lock_workstation(&self) -> Result<(), PlatformError> {
            self.locked.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn session_locked(&self) -> Result<bool, PlatformError> {
            Ok(self.locked.load(Ordering::SeqCst))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::{FakeShellHost, ShellHost, ShellInputEvent, VirtualKey};

    #[test]
    fn input_reaches_watchers_until_they_unsubscribe() {
        let shell = FakeShellHost::default();
        let seen = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&seen);
        let sub = shell
            .watch_input(
                VirtualKey(0x14),
                Box::new(move |_| {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();
        shell.send_input(ShellInputEvent::PhysicalActivity);
        drop(sub);
        shell.send_input(ShellInputEvent::PhysicalActivity);
        assert_eq!(seen.load(Ordering::SeqCst), 1, "one event before unsubscribing");
        assert_eq!(shell.watchers(), 0, "unsubscribed");
    }

    #[test]
    fn a_chord_registers_once_and_power_requests_release() {
        let shell = FakeShellHost::default();
        shell.register_hotkey("Ctrl+Alt+Space", 1).unwrap();
        assert!(shell.register_hotkey("Ctrl+Alt+Space", 2).is_err(), "taken");
        let request = shell.keep_awake().unwrap();
        assert_eq!(shell.power_requests(), 1, "held");
        drop(request);
        assert_eq!(shell.power_requests(), 0, "released");
        shell.lock_workstation().unwrap();
        assert!(shell.session_locked().unwrap(), "locked");
    }
}

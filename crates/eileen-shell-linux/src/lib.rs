//! The core process's platform implementations on Linux, as `eileen-shell-win`'s on Windows
//! (BUILD.md section 3.4; docs/LINUX.md section 4): the machine profile's facts (section 2.16), the
//! memory sample (2.5), `ShellHost`'s toasts, session calls and shortcuts, and the power facts.
//!
//! # Owns
//!
//! - The machine's facts and load on Linux: the processor and its core classes, memory totals, GPUs
//!   and NPUs, the data folder's volume ([`default_data_dir`]), the OS, power and monitors, read
//!   under a [`SysRoot`] in the shapes of [`facts`] until `eileen-contracts::machine` exists; and
//!   the power facts over D-Bus from `UPower` and power-profiles-daemon ([`LinuxPower`]) with a
//!   stream of their changes ([`PowerChanges`]), which [`LinuxMachineProbe`] does not use yet.
//! - `eileen-platform`'s `SystemMemory` for [`LinuxSystemMemory`], sampling memory into
//!   `eileen_contracts::jobs::MemoryPressure`: available memory, commit headroom under Linux's
//!   overcommit policy, the major-fault paging rate and the largest process outside Eileen's tree.
//! - `ShellHost` for [`LinuxShell`], whose D-Bus calls run on `eileen-dbus-linux`'s `Bridge`, and
//!   the trait's clients: the XDG autostart entry ([`set_run_at_sign_in`]), the lock state over
//!   logind ([`SessionLock`]), keeping the session awake ([`LinuxPowerRequest`]), toasts
//!   ([`Notifier`], with [`NotificationEvents`]), and global shortcuts from the `GlobalShortcuts`
//!   portal ([`ShortcutsSession`], with [`ShortcutEvents`] and [`trigger_of_chord`]'s triggers),
//!   which [`LinuxShell`] does not use yet.
//!
//! # Does not own
//!
//! - The traits (`eileen-platform`, WP0) and the facts' final home, `eileen-contracts::machine`,
//!   for which `MachineProbe`'s implementation waits and which [`facts`] will then re-export.
//! - The plan that reads the facts (`eileen_core::fit`, WP1); the runtime's sampler (WP2); the
//!   build guard's own memory readings (WP0); the audio endpoints and execution providers.
//! - D-Bus plumbing: [`DbusError`] (re-exported), the buses and the bridge, name owners, the
//!   portal `Request` and its answered `Close`, and the stream helpers are `eileen-dbus-linux`'s.
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - It changes nothing it is not asked to: it writes only Eileen's autostart entry, asks the
//!   session to lock only in [`SessionLock::lock_workstation`], keeps it awake no longer than a
//!   [`LinuxPowerRequest`] and the connections it was given, shows a toast only in
//!   [`Notifier::notify`] and binds shortcuts only in [`ShortcutsSession::bind`]. It starts no
//!   process itself (a call may let the bus start a service) and never needs elevation.
//! - An unreadable fact is named in `degraded`, left at its unknown value, never a real zero. No
//!   entry or error holds a user's or a fixture's path, only the fixed system file it names.
//! - It never reads a serial-number file, the machine's name, user names or accounts, and never
//!   keeps a unit's id: of NVIDIA's information file only the `Model` line is kept, and PCI ids
//!   are used to look names up, not kept. Over D-Bus each property is read alone, never with
//!   `GetAll`, so no device's serial number is fetched.
//!
//! # Boundaries
//!
//! - May depend on `eileen-platform` (the traits), `eileen-contracts` (text wrappers, `Toast`),
//!   `eileen-dbus-linux` (the D-Bus helpers, the bridge), `eileen-stdx` (the clock), `zbus`,
//!   `tokio` (the portal's deadline, the two holders), `rustix` (`statvfs`) and `thiserror`.
//!   Only the composition root on Linux may depend on it. Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-shell-linux`.
//! - Layers: unit tests for each parser over the kernel's text formats, and property tests for the
//!   CPU lists, the busy share, the MB conversion, the `os-release` quotes and the hotkey chords;
//!   the fuzz targets of `fuzz_entry`, whose seed corpora replay as tests; six fixture machines in
//!   `tests/fixtures/`, one live test of this machine, and the autostart entry in folders a guard
//!   removes. D-Bus clients run against fakes on `eileen-dbus-linux`'s private bus (feature
//!   `private-bus`, tests only), their deadlines in tokio's paused time, and [`LinuxShell`] on them
//!   over `Bridge::on_private_bus` and on the Linux desktop lane's real buses (`tests/session/`).
//! - A bug here starts as a machine panel line that does not match the PC, a plan class (`Tight`,
//!   `Standard`, `Roomy`) the PC's memory does not explain, or a call the desktop does not honour.
//!
//! # Traps
//!
//! - Each source's traps are in its own doc: `pci`, `disk`, `adapters`, `cpu`, `autostart`, `host`,
//!   `session_lock`, `keep_awake`, `notifications`, `shortcuts`, `upower`; D-Bus's in its crate.
//!
//! # Logs and tracing
//!
//! - None yet: the file reads are short, and neither the clients nor the trait implementations
//!   log. A keep-awake holder that missed is the first thing a span should carry (`host`).
//!
//! # Do not change without a decision record
//!
//! - Which source each fact comes from (docs/LINUX.md section 4) and how the core classes are
//!   numbered (higher means faster, as Windows' efficiency classes are).

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod adapters;
mod autostart;
mod cpu;
mod disk;
mod error;
pub mod facts;
#[doc(hidden)]
pub mod fuzz_entry;
mod host;
mod keep_awake;
mod load;
mod memory;
mod monitors;
mod notifications;
mod os;
mod pci;
mod power;
mod pressure;
mod probe;
mod root;
mod session_lock;
mod shortcuts;
mod upower;

pub use autostart::{default_autostart_dir, set_run_at_sign_in};
pub use disk::{Volume, read_volume};
pub use eileen_dbus_linux::DbusError;
pub use error::{AutostartError, ReadError};
pub use host::{
    BoundHotkey, CONSENT_WAIT, LinuxShell, LinuxShellEvent, LinuxShellSettings, RELEASE_WAIT,
    SETTLE, ShortcutId, ShortcutNames,
};
pub use keep_awake::{DesktopHolder, LinuxPowerRequest};
pub use notifications::{
    NotificationCapabilities, NotificationCloseReason, NotificationEvent, NotificationEvents,
    NotificationId, NotificationServer, NotificationServerInfo, Notifier, NotifierSettings,
};
pub use pressure::{LinuxSystemMemory, MIN_RATE_INTERVAL_MS};
pub use probe::{LinuxMachineProbe, default_data_dir};
pub use root::SysRoot;
pub use session_lock::SessionLock;
pub use shortcuts::{
    BoundShortcut, Shortcut, ShortcutEvent, ShortcutEvents, ShortcutsSession, trigger_of_chord,
    trigger_of_key,
};
pub use upower::{LinuxPower, PowerChanges, PowerDaemon, PowerReading};

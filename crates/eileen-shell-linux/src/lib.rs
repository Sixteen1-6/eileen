//! The core process's platform implementations on Linux: what `eileen-shell-win` does on
//! Windows (BUILD.md section 3.4; docs/LINUX.md section 4). Phase L0 starts with the machine
//! profile's facts (BUILD.md section 2.16), which [`LinuxMachineProbe`] reads from `/proc`,
//! `/sys` and `/etc`, and the memory sample of section 2.5, which [`LinuxSystemMemory`] takes.
//! Phase L1 adds `ShellHost`'s session calls, one module each.
//!
//! # Owns
//!
//! - The machine's facts and load on Linux: the processor and its core classes, memory totals,
//!   GPUs and NPUs, the data folder's volume ([`default_data_dir`]), the OS, power and monitors,
//!   read under a [`SysRoot`] in the shapes of [`facts`] until `eileen-contracts::machine` exists.
//! - Sampling memory into `eileen_contracts::jobs::MemoryPressure`: available memory, commit
//!   headroom under Linux's overcommit policy, the major-fault paging rate and the largest
//!   process outside Eileen's tree.
//! - `ShellHost`'s session calls on Linux: Eileen's XDG autostart entry ([`set_run_at_sign_in`]),
//!   the lock state over logind ([`SessionLock`]), and keeping the session awake with logind's
//!   lock and the desktop's idle hold ([`LinuxPowerRequest`]).
//!
//! # Does not own
//!
//! - The `MachineProbe`, `SystemMemory` and `ShellHost` traits (`eileen-platform`, WP0) and the
//!   fact types' final home (`eileen-contracts::machine`). When those land, this crate's readers
//!   and session calls implement the traits and [`facts`] becomes re-exports.
//! - The plan that reads the facts (`eileen_core::fit`, WP1); the runtime's sampler (WP2); the
//!   build guard's own memory readings (WP0); the audio endpoints and execution providers.
//! - D-Bus plumbing: [`DbusError`] (re-exported), the buses, name owners, signal streams and the
//!   portal `Request` are `eileen-dbus-linux`'s, not yet an answered `Close`.
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - It changes nothing it is not asked to: it writes only Eileen's autostart entry, asks the
//!   session to lock only in [`SessionLock::lock_workstation`], and keeps it awake no longer than a
//!   [`LinuxPowerRequest`] and the connections it was given. It starts no process itself, though a
//!   call may let the bus start the service it names, and it never needs elevation.
//! - An unreadable fact is named in `degraded`, left at its unknown value, never a real zero. No
//!   entry or error holds a user's or a fixture's path, only the fixed system file it names.
//! - It never reads a serial-number file, the machine's name, user names or accounts, and never
//!   keeps a unit's id: of NVIDIA's information file only the `Model` line is kept, and PCI ids
//!   are used to look names up, not kept.
//!
//! # Boundaries
//!
//! - May depend on `eileen-contracts` (the text wrappers, `Platform`), `eileen-dbus-linux` (the
//!   D-Bus helpers), `zbus` (its clients' proxies), `tokio` (the portal's one deadline),
//!   `rustix` (`statvfs`) and `thiserror`. Only the composition root on Linux may depend on it.
//! - Wire types: none of its own; the facts become contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-shell-linux`.
//! - Layers: unit tests for each parser over the kernel's text formats, and property tests for
//!   the CPU lists, the busy share, the MB conversion and the `os-release` quotes; the fuzz
//!   targets `linux_mountinfo` and `linux_proc_status` (module `fuzz_entry`), whose seed
//!   corpora replay as tests; integration tests over six fixture machines in
//!   `tests/fixtures/` (one recorded under WSL2, five synthetic) built into folders, one live
//!   test of the machine the tests run on, and the autostart entry in folders a guard removes.
//!   D-Bus clients run against fakes on `eileen-dbus-linux`'s private bus (its `private-bus`
//!   feature, tests only), the portal's deadline in tokio's paused time.
//! - A bug here starts as a machine panel line that does not match the PC, a plan class
//!   (`Tight`, `Standard`, `Roomy`) that the PC's memory does not explain, or a session call
//!   that the desktop does not honour.
//!
//! # Traps
//!
//! - Each source's traps are in its own doc: `pci`, `disk`, `adapters`, `cpu`, `autostart`,
//!   `session_lock`, `keep_awake` and, for D-Bus, `eileen-dbus-linux`.
//!
//! # Logs and tracing
//!
//! - None yet: the file reads are short and synchronous, and the D-Bus clients log nothing of
//!   their own. Spans from `eileen_trace::names` join with the trait implementations.
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
mod keep_awake;
mod load;
mod memory;
mod monitors;
mod os;
mod pci;
mod power;
mod pressure;
mod probe;
mod root;
mod session_lock;

pub use autostart::{default_autostart_dir, set_run_at_sign_in};
pub use disk::{Volume, read_volume};
pub use eileen_dbus_linux::DbusError;
pub use error::{AutostartError, ReadError};
pub use keep_awake::{DesktopHolder, LinuxPowerRequest};
pub use pressure::{LinuxSystemMemory, MIN_RATE_INTERVAL_MS};
pub use probe::{LinuxMachineProbe, default_data_dir};
pub use root::SysRoot;
pub use session_lock::SessionLock;

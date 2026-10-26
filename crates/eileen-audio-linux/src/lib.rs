//! The core and speech processes' audio implementations on Linux, as `eileen-audio-win`'s on
//! Windows (BUILD.md section 3.4; docs/LINUX.md section 4), starting with real-time audio threads
//! through `RealtimeKit`.
//!
//! # Owns
//!
//! - [`RealtimeKit`], the client of `rtkit-daemon` on the system bus: its limits ([`RtLimits`]),
//!   promoting a thread to real-time or high priority ([`RealtimePolicy`], [`Promoted`],
//!   [`Promotion`]) and the revert, with [`ThreadId`] and the bound on the process's
//!   `RLIMIT_RTTIME`.
//! - [`LinuxAudioEndpoints`]: `eileen-platform`'s `AudioEndpoints` on `eileen-dbus-linux`'s
//!   bridge. It serves `register_audio_thread`; the default endpoints wait for phase L2.
//!
//! # Does not own
//!
//! - Streaming (cpal, in `eileen-audio`, WP4), and the audio-server client the endpoints need.
//! - `SystemVolume`, which the desktop process serves (`eileen-desktop-linux`, not built yet).
//! - The bridge, the buses and [`DbusError`] (re-exported) are `eileen-dbus-linux`'s; the trait,
//!   `ThreadTicket` and `PlatformError` are `eileen-platform`'s (WP0).
//!
//! # Invariants
//!
//! - Linux only: the crate is `cfg(target_os = "linux")` and compiles to an empty library
//!   elsewhere, as the `-win` crates do off Windows.
//! - It never raises a limit (the `RLIMIT_RTTIME` bound only lowers), and never asks for more than
//!   `RealtimeKit` grants: a priority capped at `MaxRealtimePriority`, a nice level floored at
//!   `MinNiceLevel`, the revert's included.
//! - Real-time comes only through `RealtimeKit`. It starts no process (a call may let the bus
//!   start `rtkit-daemon`), needs no elevation, and changes only the threads that register.
//! - A request that rtkit may still grant after its call gave up, past the bridge's deadline or
//!   the method timeout, is reverted behind it, and a ticket reverts its thread when it drops.
//!   The promoter keeps the bridge until it has reverted, so no handle's drop cuts that short.
//!
//! # Boundaries
//!
//! - May depend on `eileen-platform` (the trait), `eileen-dbus-linux` (the bridge, `DbusError`),
//!   `rustix` (`gettid`, `getpriority`, `getrlimit`, `setrlimit`), `tokio` (the promoter's local
//!   task and its channels) and `zbus`. `eileen-audio` and `eileen-speech` may take it under their
//!   Linux target tables (BUILD.md section 3.1). Wire types: none.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p eileen-audio-linux`.
//! - Layers: unit and property tests for the limits, the bound and the promoter's hand-over;
//!   integration tests in `tests/it/` against a fake `RealtimeKit` on `eileen-dbus-linux`'s
//!   private bus, which serves its calls in order, checks the caller's `RLIMIT_RTTIME` and keeps
//!   rtkit 0.14's record of each thread, and [`LinuxAudioEndpoints`] through
//!   `Bridge::on_private_bus`. No live test: WSL2 has no `RealtimeKit`; a real Ubuntu desktop
//!   checks a registered thread with `chrt -p`.
//!
//! # Traps
//!
//! - Registration blocks its caller for a few system-bus round trips, and on first use for
//!   `RealtimeKit`'s D-Bus activation: register once per thread, before its first period.
//! - The `RLIMIT_RTTIME` bound is process-wide and permanent, the processes started after it
//!   inherit it, and a real-time thread that runs `RTTimeUSecMax` without blocking ends the
//!   process with `SIGKILL`.
//! - rtkit refuses calls past its per-user limits (25 threads; 25 calls in 20 s), a revert
//!   included, and so does rtkit 0.14 while a suspend is under way, after which it makes the
//!   thread real-time again. So does a suspend that starts between a revert's two calls, and any
//!   suspend once a thread whose nice level was negative is reverted to it. A thread that ends
//!   with its ticket escapes all three: rtkit checks a thread's start time before it acts.
//! - A request whose system-bus connection fails before rtkit answers may still be granted, and
//!   its revert, which goes over that connection, fails with it.
//! - rtkit's canary demotes every real-time thread when the system starves, which no ticket sees.
//! - Under Flatpak the Realtime portal replaces `RealtimeKit` (phase L3).
//!
//! # Logs and tracing
//!
//! - None yet: a fallback to high priority ([`Promotion::HighPriority`]) is the first thing a span
//!   should carry.
//!
//! # Do not change without a decision record
//!
//! - The `RLIMIT_RTTIME` bound, and that real-time comes only through `RealtimeKit`.

#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

mod endpoints;
mod rtkit;
mod thread;

pub use eileen_dbus_linux::DbusError;
pub use endpoints::{LinuxAudioEndpoints, LinuxAudioSettings, REVERT_WAIT};
pub use rtkit::{Promoted, Promotion, RealtimeKit, RealtimePolicy, RtLimits};
pub use thread::ThreadId;

//! The guard's sampling clock and sleep.
//!
//! The build guard depends on nothing but std and `windows-sys`, so it cannot use
//! `eileen_stdx::clock`; its one real clock and sleep are here, with the listed `#[expect]`s
//! (BUILD.md section 4b.2). Tests drive the guard with [`ManualClock`], whose sleeps only move
//! its own time.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where the guard's time comes from.
pub trait Clock {
    /// Wall-clock time, milliseconds since the Unix epoch (the `at_ms` of a sample).
    fn now_ms(&self) -> u64;
    /// Monotonic milliseconds, for deadlines and waits.
    fn mono_ms(&self) -> u64;
    /// Waits for `duration`.
    fn sleep(&self, duration: Duration);
}

/// The real clock and sleep.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealClock;

/// When the process first asked the real clock for monotonic time.
fn mono_origin() -> Instant {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(instant_now)
}

#[expect(clippy::disallowed_methods, reason = "the build guard's sampling clock (section 4b.2)")]
fn instant_now() -> Instant {
    Instant::now()
}

#[expect(clippy::disallowed_methods, reason = "the build guard's sampling clock (section 4b.2)")]
fn system_now() -> SystemTime {
    SystemTime::now()
}

/// Saturating milliseconds of a duration.
pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl Clock for RealClock {
    fn now_ms(&self) -> u64 {
        system_now().duration_since(UNIX_EPOCH).map_or(0, millis)
    }

    fn mono_ms(&self) -> u64 {
        millis(instant_now().saturating_duration_since(mono_origin()))
    }

    #[expect(clippy::disallowed_methods, reason = "the build guard's sleep (section 4b.2)")]
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// A clock for tests: time moves only when something sleeps on it or a test advances it.
#[derive(Debug)]
pub struct ManualClock {
    now_ms: AtomicU64,
    mono_ms: AtomicU64,
}

impl ManualClock {
    /// A clock whose wall time starts at `now_ms`.
    #[must_use]
    pub const fn new(now_ms: u64) -> Self {
        Self { now_ms: AtomicU64::new(now_ms), mono_ms: AtomicU64::new(0) }
    }

    /// Moves both wall and monotonic time forward.
    pub fn advance(&self, by: Duration) {
        let ms = millis(by);
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
        self.mono_ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }

    fn mono_ms(&self) -> u64 {
        self.mono_ms.load(Ordering::SeqCst)
    }

    fn sleep(&self, duration: Duration) {
        self.advance(duration);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Clock, ManualClock, RealClock};

    #[test]
    fn a_manual_clock_moves_when_slept_on() {
        let clock = ManualClock::new(1_000);
        clock.sleep(Duration::from_secs(5));
        assert_eq!(clock.now_ms(), 6_000, "wall time moved");
        assert_eq!(clock.mono_ms(), 5_000, "monotonic time moved");
    }

    #[test]
    fn the_real_clock_is_current() {
        // 2026-01-01T00:00:00Z.
        assert!(RealClock.now_ms() > 1_767_225_600_000, "wall time");
        let first = RealClock.mono_ms();
        assert!(RealClock.mono_ms() >= first, "monotonic");
    }
}

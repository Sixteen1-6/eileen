//! The clock port and its one real implementation.
//!
//! The core never reads a clock: its driver passes `MonoTime` into every step. Everything else
//! that needs the time asks a [`Clock`], so tests and replay can substitute [`ManualClock`].

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eileen_contracts::ids::{MonoTime, WallTime};

use crate::num;

/// Where time comes from.
pub trait Clock: Send + Sync {
    /// Monotonic time since this clock started, in nanoseconds.
    fn mono(&self) -> MonoTime;
    /// Wall-clock time, UTC microseconds since the Unix epoch.
    fn wall(&self) -> WallTime;
}

/// The real clock: [`Instant`] for monotonic time and [`SystemTime`] for wall time.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock whose monotonic time starts now.
    #[must_use]
    pub fn new() -> Self {
        Self { start: Self::now() }
    }

    #[expect(clippy::disallowed_methods, reason = "the one real clock (BUILD.md section 4b.2)")]
    fn now() -> Instant {
        Instant::now()
    }

    #[expect(clippy::disallowed_methods, reason = "the one real clock (BUILD.md section 4b.2)")]
    fn system_now() -> SystemTime {
        SystemTime::now()
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn mono(&self) -> MonoTime {
        let elapsed = Self::now().saturating_duration_since(self.start);
        MonoTime(num::saturating_u64_from_u128(elapsed.as_nanos()))
    }

    fn wall(&self) -> WallTime {
        wall_from_system(Self::system_now())
    }
}

/// The process-wide real clock. Its monotonic time starts the first time it is asked for.
pub fn system() -> &'static SystemClock {
    static CLOCK: OnceLock<SystemClock> = OnceLock::new();
    CLOCK.get_or_init(SystemClock::new)
}

/// The wall time of a [`SystemTime`], saturating far outside the representable range.
#[must_use]
pub fn wall_from_system(time: SystemTime) -> WallTime {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => WallTime(num::saturating_i64_from_u128(after.as_micros())),
        Err(before) => WallTime(-num::saturating_i64_from_u128(before.duration().as_micros())),
    }
}

/// A clock that only moves when a test moves it.
#[derive(Debug, Default)]
pub struct ManualClock {
    mono_ns: AtomicU64,
    wall_us: AtomicI64,
}

impl ManualClock {
    /// A clock at monotonic zero and the given wall time.
    #[must_use]
    pub const fn new(start: WallTime) -> Self {
        Self { mono_ns: AtomicU64::new(0), wall_us: AtomicI64::new(start.0) }
    }

    /// Moves both monotonic and wall time forward.
    pub fn advance(&self, by: Duration) {
        let nanos = num::saturating_u64_from_u128(by.as_nanos());
        let micros = num::saturating_i64_from_u128(by.as_micros());
        // Relaxed is enough: tests read the clock from the thread that moved it, or after a join.
        let _previous: u64 = self.mono_ns.fetch_add(nanos, Ordering::Relaxed);
        let _previous: i64 = self.wall_us.fetch_add(micros, Ordering::Relaxed);
    }

    /// Sets the wall time without moving monotonic time, as a clock change on the PC would.
    pub fn set_wall(&self, wall: WallTime) {
        self.wall_us.store(wall.0, Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn mono(&self) -> MonoTime {
        MonoTime(self.mono_ns.load(Ordering::Relaxed))
    }

    fn wall(&self) -> WallTime {
        WallTime(self.wall_us.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use eileen_contracts::ids::{MonoTime, WallTime};

    use super::{Clock, ManualClock, system, wall_from_system};

    #[test]
    fn manual_clock_moves_only_when_told() {
        let clock = ManualClock::new(WallTime(1_000));
        assert_eq!(clock.mono(), MonoTime(0), "starts at zero");
        clock.advance(Duration::from_millis(3));
        assert_eq!(clock.mono(), MonoTime(3_000_000), "monotonic nanoseconds");
        assert_eq!(clock.wall(), WallTime(4_000), "wall microseconds");
        clock.set_wall(WallTime(0));
        assert_eq!(clock.mono(), MonoTime(3_000_000), "a wall change leaves monotonic time");
    }

    #[test]
    fn system_clock_is_monotonic_and_after_2026() {
        let first = system().mono();
        let second = system().mono();
        assert!(second >= first, "monotonic");
        // 2026-01-01T00:00:00Z in microseconds.
        assert!(system().wall().0 > 1_767_225_600_000_000, "wall time is current");
    }

    #[test]
    fn wall_time_before_the_epoch_is_negative() {
        let before = UNIX_EPOCH.checked_sub(Duration::from_secs(1)).unwrap();
        assert_eq!(wall_from_system(before), WallTime(-1_000_000), "one second before");
    }
}

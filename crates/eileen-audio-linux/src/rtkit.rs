//! [`RealtimeKit`]: the client of `rtkit-daemon`, the system-bus service through which a user's
//! process gets real-time or high priority for its threads without privileges (docs/LINUX.md
//! sections 4 and 5). It serves `AudioEndpoints::register_audio_thread`, which is MMCSS on
//! Windows. What it relies on was read in rtkit 0.13's and 0.14's `rtkit-daemon.c`, the versions
//! Ubuntu 24.04 and 26.04 ship.
//!
//! # Promoting a thread
//!
//! [`RealtimeKit::promote`] reads the three limits, each property alone, then asks for real-time
//! (`SCHED_RR`) at the policy's priority capped at `MaxRealtimePriority`, after bounding the
//! process's `RLIMIT_RTTIME` to `RTTimeUSecMax`, which rtkit checks before and after it changes
//! the thread. A refusal falls back to high priority (`SCHED_OTHER` at a negative nice level,
//! floored at `MinNiceLevel`), and so does a bound that failed. rtkit refuses with
//! `org.freedesktop.DBus.Error.AccessDenied` for every reason it has: a priority past its
//! maximum, an unbounded `RLIMIT_RTTIME`, polkit (whose actions refuse a session that is not
//! local, as over SSH), its rate limit (25 calls in 20 s per user, across all of that user's
//! processes), 25 threads per user, its canary's lockout after a starved system and, on 0.14, a
//! suspend under way. `eileen-dbus-linux` maps each to [`DbusError::Refused`], "not authorized".
//!
//! # Reverting
//!
//! [`RealtimeKit::revert`] asks for `SCHED_OTHER` at the nice level the thread had: neither std
//! nor rustix has a safe `sched_setscheduler`. rtkit 0.14 records the real-time priority it last
//! granted a thread, which `MakeThreadHighPriority` leaves, and after a suspend makes every thread
//! it still records real-time again. It forgets a thread only in the garbage collection it runs
//! at the start of each call to its object, once the thread is neither real-time nor at a
//! negative nice level, so the revert reads `RTTimeUSecMax` once more to start one.

use zbus::proxy::CacheProperties;

use crate::DbusError;
use crate::thread::{ThreadId, bound_rttime};

/// What the errors name: the service, and each method and property by its D-Bus name.
const RTKIT: &str = "org.freedesktop.RealtimeKit1";
const MAKE_REALTIME: &str = "org.freedesktop.RealtimeKit1.MakeThreadRealtime";
const MAKE_HIGH_PRIORITY: &str = "org.freedesktop.RealtimeKit1.MakeThreadHighPriority";
const RTTIME_USEC_MAX: &str = "org.freedesktop.RealtimeKit1.RTTimeUSecMax";
const MAX_REALTIME_PRIORITY: &str = "org.freedesktop.RealtimeKit1.MaxRealtimePriority";
const MIN_NICE_LEVEL: &str = "org.freedesktop.RealtimeKit1.MinNiceLevel";

/// Linux's nice levels, from -20, the highest priority, to 19.
const NICE_LEVELS: std::ops::RangeInclusive<i32> = -20..=19;

/// `RealtimeKit`'s object, `/org/freedesktop/RealtimeKit1`, as rtkit 0.13 and 0.14 serve it.
#[zbus::proxy(
    interface = "org.freedesktop.RealtimeKit1",
    default_service = "org.freedesktop.RealtimeKit1",
    default_path = "/org/freedesktop/RealtimeKit1"
)]
trait Rtkit {
    /// Makes `thread`, of the caller's process, `SCHED_RR` at `priority`.
    #[zbus(name = "MakeThreadRealtime")]
    fn make_thread_realtime(&self, thread: u64, priority: u32) -> zbus::Result<()>;

    /// Makes `thread`, of the caller's process, `SCHED_OTHER` at nice level `nice_level`.
    #[zbus(name = "MakeThreadHighPriority")]
    fn make_thread_high_priority(&self, thread: u64, nice_level: i32) -> zbus::Result<()>;

    /// The hard `RLIMIT_RTTIME`, in microseconds, the caller must have at most.
    #[zbus(property, name = "RTTimeUSecMax")]
    fn rttime_usec_max(&self) -> zbus::Result<i64>;

    /// The highest real-time priority granted.
    #[zbus(property, name = "MaxRealtimePriority")]
    fn max_realtime_priority(&self) -> zbus::Result<i32>;

    /// The lowest nice level granted.
    #[zbus(property, name = "MinNiceLevel")]
    fn min_nice_level(&self) -> zbus::Result<i32>;
}

/// What [`RealtimeKit::promote`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RealtimePolicy {
    /// The `SCHED_RR` priority, capped at `MaxRealtimePriority`.
    pub priority: u32,
    /// The nice level when real-time is refused, floored at `MinNiceLevel`.
    pub fallback_nice: i32,
}

impl Default for RealtimePolicy {
    /// Priority 10, below the audio server's threads: `PipeWire` 1.0.5's `pipewire.conf` asks its
    /// `module-rt` for priority 88, which `RealtimeKit` caps at its maximum, 20 by default
    /// [inferred: rtkit refuses any priority past it]. The fallback, nice -11, is the level that
    /// file asks for.
    fn default() -> Self {
        Self { priority: 10, fallback_nice: -11 }
    }
}

/// What a thread got from [`RealtimeKit::promote`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Promotion {
    /// `SCHED_RR`.
    Realtime {
        /// Its real-time priority.
        priority: u32,
    },
    /// `SCHED_OTHER` at a negative nice level, since real-time was refused.
    HighPriority {
        /// Its nice level.
        nice: i32,
    },
}

impl Promotion {
    /// What the thread's `ThreadTicket` names as its task.
    #[must_use]
    pub const fn task(self) -> &'static str {
        match self {
            Self::Realtime { .. } => "RealtimeKit real-time",
            Self::HighPriority { .. } => "RealtimeKit high priority",
        }
    }
}

/// `RealtimeKit`'s limits, from its three properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RtLimits {
    rttime_usec_max: u64,
    max_realtime_priority: u32,
    min_nice_level: i32,
}

impl RtLimits {
    /// The limits, unless one is out of range: a `RTTimeUSecMax` that is not positive, a
    /// negative `MaxRealtimePriority`, or a `MinNiceLevel` that is no nice level.
    fn new(
        rttime_usec_max: i64,
        max_realtime_priority: i32,
        min_nice_level: i32,
    ) -> Result<Self, DbusError> {
        let invalid = |what, detail: String| DbusError::Invalid { what, detail };
        let rttime = u64::try_from(rttime_usec_max).ok().filter(|&max| max > 0);
        let rttime_usec_max = rttime
            .ok_or_else(|| invalid(RTTIME_USEC_MAX, format!("{rttime_usec_max}, not positive")))?;
        let max_realtime_priority = u32::try_from(max_realtime_priority).map_err(|_negative| {
            invalid(MAX_REALTIME_PRIORITY, format!("{max_realtime_priority}, negative"))
        })?;
        if !NICE_LEVELS.contains(&min_nice_level) {
            return Err(invalid(MIN_NICE_LEVEL, format!("{min_nice_level}, not a nice level")));
        }
        Ok(Self { rttime_usec_max, max_realtime_priority, min_nice_level })
    }

    /// `RTTimeUSecMax`: the hard `RLIMIT_RTTIME`, in microseconds, a process must have at most.
    #[must_use]
    pub const fn rttime_usec_max(&self) -> u64 {
        self.rttime_usec_max
    }

    /// `MaxRealtimePriority`.
    #[must_use]
    pub const fn max_realtime_priority(&self) -> u32 {
        self.max_realtime_priority
    }

    /// `MinNiceLevel`.
    #[must_use]
    pub const fn min_nice_level(&self) -> i32 {
        self.min_nice_level
    }

    /// The real-time priority to ask for `wanted`: capped at `MaxRealtimePriority`, and `None`
    /// when that leaves none (below 1).
    #[must_use]
    pub fn realtime_priority(&self, wanted: u32) -> Option<u32> {
        Some(wanted.min(self.max_realtime_priority)).filter(|&priority| priority >= 1)
    }

    /// The nice level to ask for `wanted`: floored at `MinNiceLevel`.
    #[must_use]
    pub fn nice_level(&self, wanted: i32) -> i32 {
        wanted.max(self.min_nice_level)
    }
}

/// A client of `RealtimeKit` on the system bus. Every read asks rtkit afresh: the proxy keeps no
/// property cache and adds no match rule.
#[derive(Debug, Clone)]
pub struct RealtimeKit {
    system: zbus::Connection,
}

impl RealtimeKit {
    /// A client of `RealtimeKit` on `system`, the system bus.
    #[must_use]
    pub fn new(system: &zbus::Connection) -> Self {
        Self { system: system.clone() }
    }

    /// The limits, each property read alone; one out of range is [`DbusError::Invalid`].
    pub async fn limits(&self) -> Result<RtLimits, DbusError> {
        let rtkit = self.proxy().await?;
        let rttime =
            rtkit.rttime_usec_max().await.map_err(|error| failed(RTTIME_USEC_MAX, &error))?;
        let priority = rtkit
            .max_realtime_priority()
            .await
            .map_err(|error| failed(MAX_REALTIME_PRIORITY, &error))?;
        let nice = rtkit.min_nice_level().await.map_err(|error| failed(MIN_NICE_LEVEL, &error))?;
        RtLimits::new(rttime, priority, nice)
    }

    /// `MakeThreadRealtime`: `thread` becomes `SCHED_RR` at `priority`. rtkit refuses unless the
    /// process's hard `RLIMIT_RTTIME` is at most `RTTimeUSecMax`, which [`Self::promote`] sees to.
    pub async fn make_realtime(&self, thread: ThreadId, priority: u32) -> Result<(), DbusError> {
        let rtkit = self.proxy().await?;
        rtkit
            .make_thread_realtime(thread.get(), priority)
            .await
            .map_err(|error| failed(MAKE_REALTIME, &error))
    }

    /// `MakeThreadHighPriority`: `thread` becomes `SCHED_OTHER` at nice level `nice`.
    pub async fn make_high_priority(&self, thread: ThreadId, nice: i32) -> Result<(), DbusError> {
        let rtkit = self.proxy().await?;
        rtkit
            .make_thread_high_priority(thread.get(), nice)
            .await
            .map_err(|error| failed(MAKE_HIGH_PRIORITY, &error))
    }

    /// Makes `thread` real-time as `policy` asks, or high priority when rtkit refuses or the
    /// bound fails; the module's doc says how. An error leaves the thread as it was, except one
    /// past a method timeout, after which rtkit may still act.
    pub async fn promote(
        &self,
        thread: ThreadId,
        policy: RealtimePolicy,
    ) -> Result<Promotion, DbusError> {
        let limits = self.limits().await?;
        if let Some(priority) = limits.realtime_priority(policy.priority)
            && bound_rttime(limits.rttime_usec_max()).is_ok()
        {
            match self.make_realtime(thread, priority).await {
                Ok(()) => return Ok(Promotion::Realtime { priority }),
                Err(DbusError::Refused { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        let nice = limits.nice_level(policy.fallback_nice);
        self.make_high_priority(thread, nice).await?;
        Ok(Promotion::HighPriority { nice })
    }

    /// Puts `thread` back on `SCHED_OTHER` at nice level `nice`, the level it had, then reads
    /// `RTTimeUSecMax`, so that rtkit 0.14 forgets the thread (the module's doc says why).
    pub async fn revert(&self, thread: ThreadId, nice: i32) -> Result<(), DbusError> {
        self.make_high_priority(thread, nice).await?;
        self.proxy()
            .await?
            .rttime_usec_max()
            .await
            .map_err(|error| failed(RTTIME_USEC_MAX, &error))?;
        Ok(())
    }

    /// rtkit's object on the system bus, with no property cache.
    async fn proxy(&self) -> Result<RtkitProxy<'static>, DbusError> {
        RtkitProxy::builder(&self.system)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|error| failed(RTKIT, &error))
    }
}

/// What a failed call to `what` means, by [`DbusError::from_zbus`].
fn failed(what: &'static str, error: &zbus::Error) -> DbusError {
    DbusError::from_zbus(what, error)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{NICE_LEVELS, RtLimits};

    /// Limits rtkit could report: a positive `RTTimeUSecMax`, a priority from 0 to 99, the
    /// range of `SCHED_RR`, and any nice level.
    fn limits() -> impl Strategy<Value = RtLimits> {
        (1..=i64::MAX, 0..=99_i32, NICE_LEVELS)
            .prop_map(|(rttime, priority, nice)| RtLimits::new(rttime, priority, nice).unwrap())
    }

    proptest! {
        #[test]
        fn the_priority_and_nice_stay_within_realtimekits_limits(
            limits in limits(),
            wanted_priority in any::<u32>(),
            wanted_nice in any::<i32>(),
        ) {
            match limits.realtime_priority(wanted_priority) {
                Some(priority) => {
                    prop_assert!(priority >= 1, "a real-time priority");
                    prop_assert!(priority <= limits.max_realtime_priority(), "within the maximum");
                    prop_assert!(priority <= wanted_priority, "never more than asked");
                }
                None => prop_assert!(
                    wanted_priority.min(limits.max_realtime_priority()) == 0,
                    "none only when the cap leaves 0"
                ),
            }
            let nice = limits.nice_level(wanted_nice);
            prop_assert!(nice >= limits.min_nice_level(), "never past the minimum");
            prop_assert!(nice >= wanted_nice, "never a higher priority than asked");
        }
    }
}

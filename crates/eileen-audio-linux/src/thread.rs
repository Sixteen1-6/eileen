//! The calling thread's kernel id and nice level, and the bound on the process's `RLIMIT_RTTIME`
//! that `RealtimeKit` asks for before it makes a thread real-time, all with rustix's safe calls.
//!
//! `RLIMIT_RTTIME` counts the CPU time a thread under a real-time policy uses without a blocking
//! system call. Past the soft limit the process gets `SIGXCPU`, whose default action ends it,
//! and `SIGKILL` at the hard limit, which comes first when the two are equal. Linux's default is
//! unlimited, and `rtkit-daemon` refuses `MakeThreadRealtime` unless the hard limit in
//! `/proc/<pid>/limits` is a number no larger than its `RTTimeUSecMax` (`verify_process_rttime`
//! in rtkit 0.13 and 0.14).

use eileen_platform::PlatformError;
use rustix::io::Errno;
use rustix::process::{Resource, Rlimit, getpriority_process, getrlimit, setrlimit};
use rustix::thread::Pid;

/// A thread's kernel id, `gettid`, as `RealtimeKit` names threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(Pid);

impl ThreadId {
    /// The calling thread's.
    #[must_use]
    pub fn current() -> Self {
        Self(rustix::thread::gettid())
    }

    /// The id as `RealtimeKit`'s methods take it.
    #[must_use]
    pub fn get(self) -> u64 {
        u64::from(self.0.as_raw_pid().unsigned_abs())
    }

    /// The thread's nice level: `getpriority(PRIO_PROCESS, tid)`, since Linux keeps one per
    /// thread.
    pub fn nice(self) -> Result<i32, PlatformError> {
        getpriority_process(Some(self.0)).map_err(|errno| os_error("getpriority", errno))
    }
}

/// Bounds the process's `RLIMIT_RTTIME` to at most `max_us` microseconds, as [`bounded`] says,
/// with `setrlimit` when that changes it. The bound is process-wide, the processes started after
/// it inherit it, and it is permanent: raising a hard limit again needs `CAP_SYS_RESOURCE`.
pub(crate) fn bound_rttime(max_us: u64) -> Result<(), PlatformError> {
    match bounded(getrlimit(Resource::Rttime), max_us) {
        Some(limit) => {
            setrlimit(Resource::Rttime, limit).map_err(|errno| os_error("setrlimit", errno))
        }
        None => Ok(()),
    }
}

/// The limit `current` becomes, bounded to `max`: the hard limit at most `max` (an unlimited one
/// becomes `max`), and the soft limit at most the hard one (an unlimited one becomes the hard
/// one). `None` when that changes nothing. Neither limit ever rises.
pub(crate) fn bounded(current: Rlimit, max: u64) -> Option<Rlimit> {
    let hard = current.maximum.map_or(max, |hard| hard.min(max));
    let soft = current.current.map_or(hard, |soft| soft.min(hard));
    let limit = Rlimit { current: Some(soft), maximum: Some(hard) };
    (limit != current).then_some(limit)
}

/// The error of the OS call `call`, with its `errno`.
fn os_error(call: &'static str, errno: Errno) -> PlatformError {
    PlatformError::Os { call, code: i64::from(errno.raw_os_error()) }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rustix::process::Rlimit;

    use super::bounded;

    /// rtkit's default `RTTimeUSecMax`, 200 ms.
    const MAX: u64 = 200_000;

    /// Whether `limit` is at most `bound`, `None` being unlimited.
    fn at_most(limit: Option<u64>, bound: Option<u64>) -> bool {
        match (limit, bound) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(limit), Some(bound)) => limit <= bound,
        }
    }

    #[test]
    fn an_unlimited_hard_limit_is_lowered_to_the_maximum() {
        let unlimited = Rlimit { current: None, maximum: None };
        assert_eq!(
            bounded(unlimited, MAX),
            Some(Rlimit { current: Some(MAX), maximum: Some(MAX) }),
            "Linux's default: both limits become the maximum"
        );
        let soft = Rlimit { current: Some(50_000), maximum: None };
        assert_eq!(
            bounded(soft, MAX),
            Some(Rlimit { current: Some(50_000), maximum: Some(MAX) }),
            "a lower soft limit stays"
        );
        let within = Rlimit { current: Some(50_000), maximum: Some(100_000) };
        assert_eq!(bounded(within, MAX), None, "already within: nothing to set");
    }

    proptest! {
        #[test]
        fn the_bounded_limit_never_rises_and_keeps_soft_within_hard(
            soft in proptest::option::of(any::<u64>()),
            hard in proptest::option::of(any::<u64>()),
            max in 1..=u64::MAX,
        ) {
            let current = Rlimit { current: soft, maximum: hard };
            let limit = bounded(current, max).unwrap_or(current);
            prop_assert!(limit.maximum.is_some_and(|hard| hard <= max), "within RTTimeUSecMax");
            prop_assert!(at_most(limit.current, limit.maximum), "soft within hard: {:?}", limit);
            prop_assert!(at_most(limit.maximum, current.maximum), "the hard limit never rises");
            prop_assert!(at_most(limit.current, current.current), "the soft limit never rises");
        }
    }
}

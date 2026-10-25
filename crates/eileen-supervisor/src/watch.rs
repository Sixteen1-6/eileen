//! Deadlines, heartbeats and bounded restart backoff (section 2.1 rule 2, simulator
//! invariant 8): every call to another process has a deadline, every sidecar a heartbeat, and
//! a child that keeps failing restarts at most a bounded number of times per window.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use eileen_trace::names;
use tokio::time::Instant;

/// A call that passed its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{call} passed its {}-ms deadline", deadline.as_millis())]
pub struct DeadlineExceeded {
    /// The call.
    pub call: &'static str,
    /// The deadline it had.
    pub deadline: Duration,
}

/// Runs `future` for at most `deadline`; past it, logs `deadline.passed` and gives up on the
/// future (dropping it), answering [`DeadlineExceeded`].
pub async fn with_deadline<F: Future>(
    call: &'static str,
    deadline: Duration,
    future: F,
) -> Result<F::Output, DeadlineExceeded> {
    match tokio::time::timeout(deadline, future).await {
        Ok(output) => Ok(output),
        Err(_elapsed) => {
            let deadline_ms = u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX);
            tracing::warn!(name: names::DEADLINE_PASSED, call, deadline_ms);
            Err(DeadlineExceeded { call, deadline })
        }
    }
}

/// A child's heartbeat: it must beat at least once every `timeout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heartbeat {
    timeout: Duration,
    last: Instant,
}

impl Heartbeat {
    /// A heartbeat that last beat at `now`.
    #[must_use]
    pub const fn new(timeout: Duration, now: Instant) -> Self {
        Self { timeout, last: now }
    }

    /// Records a beat.
    pub const fn beat(&mut self, now: Instant) {
        self.last = now;
    }

    /// How long it has been silent, once that passes the timeout; `None` while it is alive.
    #[must_use]
    pub fn missed(&self, now: Instant) -> Option<Duration> {
        let silent = now.saturating_duration_since(self.last);
        (silent > self.timeout).then_some(silent)
    }

    /// When it will count as missed if no beat comes first; `None` when that lies past the
    /// clock's range (a timeout such as `Duration::MAX` used as "never").
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.last.checked_add(self.timeout)
    }
}

/// How a crashed or hung child restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    /// The first delay.
    pub initial: Duration,
    /// The longest delay.
    pub max: Duration,
    /// The most restarts inside `window`; one more gives up.
    pub max_restarts: u32,
    /// The window the restarts are counted in.
    pub window: Duration,
    /// A run at least this long resets the delay to `initial`.
    pub stable_after: Duration,
}

impl RestartPolicy {
    /// The first delay, never longer than the longest one.
    fn first_delay(&self) -> Duration {
        self.initial.min(self.max)
    }
}

impl Default for RestartPolicy {
    /// 250 ms doubling to 30 s, at most 5 restarts in 10 minutes, reset after 2 stable
    /// minutes.
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(250),
            max: Duration::from_secs(30),
            max_restarts: 5,
            window: Duration::from_mins(10),
            stable_after: Duration::from_mins(2),
        }
    }
}

/// What to do after a child exited unexpectedly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Restart it after this delay.
    After(Duration),
    /// Stop restarting: it failed too often inside the window.
    GiveUp {
        /// Restarts in the window so far.
        restarts: u32,
    },
}

/// The restart history of one child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restarts {
    policy: RestartPolicy,
    history: VecDeque<Instant>,
    next: Duration,
}

impl Restarts {
    /// No restarts yet.
    #[must_use]
    pub fn new(policy: RestartPolicy) -> Self {
        Self { policy, history: VecDeque::new(), next: policy.first_delay() }
    }

    /// The child, started at `started`, exited at `now`: restart it, after how long, or give up.
    /// Delays double up to the policy's maximum and saturate rather than overflow, whatever
    /// the policy says.
    pub fn on_exit(&mut self, started: Instant, now: Instant) -> RestartDecision {
        if now.saturating_duration_since(started) >= self.policy.stable_after {
            self.next = self.policy.first_delay();
        }
        while self
            .history
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > self.policy.window)
        {
            self.history.pop_front();
        }
        let restarts = u32::try_from(self.history.len()).unwrap_or(u32::MAX);
        if restarts >= self.policy.max_restarts {
            return RestartDecision::GiveUp { restarts };
        }
        self.history.push_back(now);
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.policy.max);
        RestartDecision::After(delay)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;

    use super::{Heartbeat, RestartDecision, RestartPolicy, Restarts, with_deadline};

    #[test]
    fn restarts_back_off_and_give_up_inside_the_window() {
        let t0 = Instant::now();
        let mut restarts = Restarts::new(RestartPolicy::default());
        let at = |s: u64| t0 + Duration::from_secs(s);
        let delays: Vec<RestartDecision> =
            (0..6).map(|i| restarts.on_exit(at(i), at(i + 1))).collect();
        assert_eq!(
            delays[..5],
            [250, 500, 1_000, 2_000, 4_000]
                .map(|ms| RestartDecision::After(Duration::from_millis(ms))),
            "doubling"
        );
        assert_eq!(delays[5], RestartDecision::GiveUp { restarts: 5 }, "at most 5 in the window");
        // Eleven minutes later the window has emptied, and a long run resets the delay.
        assert_eq!(
            restarts.on_exit(at(7), at(7 + 660)),
            RestartDecision::After(Duration::from_millis(250)),
            "reset"
        );
    }

    #[test]
    fn delays_stop_at_the_maximum() {
        let t0 = Instant::now();
        let policy = RestartPolicy { max_restarts: 100, ..RestartPolicy::default() };
        let mut restarts = Restarts::new(policy);
        let last = (0..20).map(|i| restarts.on_exit(t0, t0 + Duration::from_secs(i))).last();
        assert_eq!(last, Some(RestartDecision::After(Duration::from_secs(30))), "capped at 30 s");
    }

    #[test]
    fn a_silent_child_misses_its_heartbeat() {
        let t0 = Instant::now();
        let mut heartbeat = Heartbeat::new(Duration::from_secs(10), t0);
        assert_eq!(heartbeat.missed(t0 + Duration::from_secs(9)), None, "alive");
        assert_eq!(
            heartbeat.missed(t0 + Duration::from_secs(11)),
            Some(Duration::from_secs(11)),
            "missed"
        );
        heartbeat.beat(t0 + Duration::from_secs(11));
        assert_eq!(heartbeat.missed(t0 + Duration::from_secs(12)), None, "beat again");
        assert_eq!(heartbeat.deadline(), Some(t0 + Duration::from_secs(21)), "next deadline");
    }

    #[test]
    fn a_never_timeout_and_huge_or_inverted_delays_do_not_overflow() {
        let t0 = Instant::now();
        let never = Heartbeat::new(Duration::MAX, t0);
        assert_eq!(never.deadline(), None, "a deadline past the clock's range is none");
        assert_eq!(never.missed(t0 + Duration::from_secs(3_600)), None, "and is never missed");
        let huge = RestartPolicy {
            initial: Duration::MAX,
            max: Duration::MAX,
            max_restarts: 100,
            ..RestartPolicy::default()
        };
        let mut restarts = Restarts::new(huge);
        for i in 0..3 {
            assert_eq!(
                restarts.on_exit(t0, t0 + Duration::from_secs(i)),
                RestartDecision::After(Duration::MAX),
                "doubling saturates at the maximum"
            );
        }
        let inverted = RestartPolicy {
            initial: Duration::from_secs(60),
            max: Duration::from_secs(30),
            ..RestartPolicy::default()
        };
        let mut restarts = Restarts::new(inverted);
        assert_eq!(
            restarts.on_exit(t0, t0 + Duration::from_secs(1)),
            RestartDecision::After(Duration::from_secs(30)),
            "the first delay never exceeds the maximum"
        );
        assert_eq!(
            restarts.on_exit(t0, t0 + Duration::from_secs(600)),
            RestartDecision::After(Duration::from_secs(30)),
            "nor does the first delay after a stable run"
        );
    }

    #[tokio::test]
    async fn a_call_past_its_deadline_is_abandoned() {
        // A zero deadline: tokio's timer rounds up to its next millisecond, so this waits at
        // most a millisecond for a future that never completes.
        let pending =
            with_deadline("test.call", Duration::ZERO, std::future::pending::<()>()).await;
        assert_eq!(pending.unwrap_err().call, "test.call", "abandoned");
        assert_eq!(
            with_deadline("test.call", Duration::from_secs(5), async { 7 }).await,
            Ok(7),
            "in time"
        );
    }
}

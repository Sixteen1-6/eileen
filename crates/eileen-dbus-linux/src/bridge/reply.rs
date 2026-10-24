//! A job's answer, carried from the bridge's thread to the caller that waits for it.
//!
//! The caller waits on a condition variable with the call's budget, so its wait is bounded
//! whatever the bridge's thread is doing, a stuck thread included. A [`Reply`] dropped
//! unanswered, by a job that panicked or a bridge that stopped, ends the wait at once.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// How a caller's wait ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome<T> {
    /// The job answered in time.
    Answered(T),
    /// The budget passed first.
    Late,
    /// The job ended without answering.
    Dropped,
}

/// Where the answer is, or is not yet.
#[derive(Debug)]
enum State<T> {
    Waiting,
    Answered(T),
    Dropped,
    /// The caller has taken the answer or stopped waiting: what comes later is dropped.
    Closed,
}

/// The slot the two halves share.
#[derive(Debug)]
struct Slot<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}

impl<T> Slot<T> {
    /// The state, locked. A panic while it was held cannot leave it half written (each
    /// change is one assignment), so a poisoned lock is used as it is.
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Settles the slot as `state` and wakes the caller, unless it is settled already.
    fn settle(&self, state: State<T>) {
        let mut current = self.lock();
        if matches!(*current, State::Waiting) {
            *current = state;
            drop(current);
            self.changed.notify_all();
        }
    }
}

/// The half a job answers through, on the bridge's thread.
#[derive(Debug)]
pub(super) struct Reply<T>(Arc<Slot<T>>);

/// The half the caller waits on.
#[derive(Debug)]
pub(super) struct Answer<T>(Arc<Slot<T>>);

/// A new slot's two halves.
pub(super) fn pair<T>() -> (Reply<T>, Answer<T>) {
    let slot = Arc::new(Slot { state: Mutex::new(State::Waiting), changed: Condvar::new() });
    (Reply(Arc::clone(&slot)), Answer(slot))
}

impl<T> Reply<T> {
    /// Answers with `value`; dropped if the caller has stopped waiting.
    pub(super) fn send(self, value: T) {
        self.0.settle(State::Answered(value));
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        self.0.settle(State::Dropped);
    }
}

impl<T> Answer<T> {
    /// Waits at most `budget` for the answer.
    pub(super) fn wait(self, budget: Duration) -> Outcome<T> {
        let (mut state, _timeout) = self
            .0
            .changed
            .wait_timeout_while(self.0.lock(), budget, |state| matches!(state, State::Waiting))
            .unwrap_or_else(PoisonError::into_inner);
        let taken = std::mem::replace(&mut *state, State::Closed);
        drop(state);
        match taken {
            State::Answered(value) => Outcome::Answered(value),
            State::Dropped => Outcome::Dropped,
            State::Waiting | State::Closed => Outcome::Late,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Outcome, pair};

    #[test]
    fn an_answer_sent_before_the_wait_is_taken() {
        let (reply, answer) = pair();
        reply.send(7);
        assert_eq!(answer.wait(Duration::ZERO), Outcome::Answered(7), "already there");
    }

    #[test]
    fn no_answer_within_the_budget_is_late_and_a_later_one_is_dropped() {
        let (reply, answer) = pair::<u32>();
        assert_eq!(answer.wait(Duration::ZERO), Outcome::Late, "nothing came");
        reply.send(7);
    }

    #[test]
    fn a_reply_dropped_unanswered_ends_the_wait() {
        let (reply, answer) = pair::<u32>();
        drop(reply);
        assert_eq!(answer.wait(Duration::from_secs(3600)), Outcome::Dropped, "at once");
    }
}

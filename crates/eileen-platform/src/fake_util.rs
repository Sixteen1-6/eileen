//! Shared pieces of the fakes.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use eileen_stdx::clock::ManualClock;

use crate::{Deadline, PlatformError, Subscription};

/// Locks a fake's state, carrying on past a poisoned lock (a test that panicked).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How long a fake's file read or write takes, on a [`ManualClock`] the test holds: what lets
/// the fake of a method that reads or writes the user's files (BUILD.md section 3.4) answer
/// within its caller's [`Deadline`], or pass it, without sleeping. Until a test sets a time,
/// every operation takes none.
#[derive(Debug, Default)]
pub(crate) struct FileTime(Mutex<Option<(Arc<ManualClock>, Duration)>>);

impl FileTime {
    /// From now on, each operation takes `takes` on `clock`.
    pub(crate) fn set(&self, clock: Arc<ManualClock>, takes: Duration) {
        *lock(&self.0) = Some((clock, takes));
    }

    /// Spends one operation's time within `deadline`, as an implementation's helper thread
    /// would. An operation that fits the deadline's budget moves the clock by its own time and
    /// answers `Ok`. One that does not moves the clock by the budget alone, the whole of the
    /// caller's wait, and answers [`PlatformError::DeadlineExceeded`] naming `call`; the fake
    /// then leaves its state as it was, since the caller cannot know whether the abandoned
    /// operation lands.
    pub(crate) fn spend(
        &self,
        call: &'static str,
        deadline: Deadline,
    ) -> Result<(), PlatformError> {
        let time = lock(&self.0).clone();
        let Some((clock, takes)) = time else {
            return Ok(());
        };
        let budget = deadline.budget();
        if takes <= budget {
            clock.advance(takes);
            Ok(())
        } else {
            clock.advance(budget);
            Err(PlatformError::DeadlineExceeded { call })
        }
    }
}

/// A subscriber, shared so that [`Sinks::emit`] can call a copy of the list after it lets go of
/// the lock.
type Sink<T> = Arc<dyn Fn(T) + Send + Sync>;

/// The subscribers of a fake's stream.
pub(crate) struct Sinks<T> {
    sinks: Arc<Mutex<BTreeMap<u64, Sink<T>>>>,
    next: AtomicU64,
}

impl<T: Clone + 'static> Sinks<T> {
    pub(crate) fn new() -> Self {
        Self { sinks: Arc::new(Mutex::new(BTreeMap::new())), next: AtomicU64::new(0) }
    }

    /// Adds a subscriber; dropping the returned subscription removes it.
    pub(crate) fn add(&self, sink: Box<dyn Fn(T) + Send + Sync>) -> Subscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.sinks).insert(id, Arc::from(sink));
        let sinks = Arc::clone(&self.sinks);
        Subscription::new(move || {
            lock(&sinks).remove(&id);
        })
    }

    /// Sends `value` to every subscriber. The list is copied under the lock and called after
    /// the lock is released, so a subscriber may unsubscribe, or subscribe again, from inside
    /// its own callback (the `-win` and `-linux` implementations copy this pattern).
    pub(crate) fn emit(&self, value: &T) {
        let sinks: Vec<Sink<T>> = lock(&self.sinks).values().map(Arc::clone).collect();
        for sink in sinks {
            sink(value.clone());
        }
    }

    /// How many subscribers there are.
    pub(crate) fn len(&self) -> usize {
        lock(&self.sinks).len()
    }
}

impl<T> std::fmt::Debug for Sinks<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sinks").field("count", &lock(&self.sinks).len()).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{Sinks, lock};
    use crate::Subscription;

    #[test]
    fn a_subscriber_may_unsubscribe_from_inside_its_own_callback() {
        let sinks = Sinks::<u32>::new();
        let calls = Arc::new(AtomicU32::new(0));
        let own: Arc<Mutex<Option<Subscription>>> = Arc::new(Mutex::new(None));
        let (counter, slot) = (Arc::clone(&calls), Arc::clone(&own));
        let subscription = sinks.add(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            // Dropping its own subscription takes the list's lock, which emit no longer holds.
            drop(lock(&slot).take());
        }));
        *lock(&own) = Some(subscription);
        sinks.emit(&1);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "called once, without a deadlock");
        assert_eq!(sinks.len(), 0, "it unsubscribed from inside its callback");
        sinks.emit(&2);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "and is not called again");
    }
}

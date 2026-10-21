//! Shared pieces of the fakes.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::Subscription;

/// Locks a fake's state, carrying on past a poisoned lock (a test that panicked).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

type Sink<T> = Box<dyn Fn(T) + Send + Sync>;

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
    pub(crate) fn add(&self, sink: Sink<T>) -> Subscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.sinks).insert(id, sink);
        let sinks = Arc::clone(&self.sinks);
        Subscription::new(move || {
            lock(&sinks).remove(&id);
        })
    }

    /// Sends `value` to every subscriber.
    pub(crate) fn emit(&self, value: &T) {
        for sink in lock(&self.sinks).values() {
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

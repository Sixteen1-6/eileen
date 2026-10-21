//! Bounded channels with a name: every queue in Eileen has a maximum, and a full queue says
//! so in the logs (`channel.full`) instead of growing without limit. `unbounded_channel` is
//! banned by clippy.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use eileen_trace::names;
use tokio::sync::{broadcast, mpsc};

pub use tokio::sync::mpsc::Receiver;
pub use tokio::sync::mpsc::error::{SendError, TrySendError};

/// The sending half of a [`bounded`] channel.
pub struct Sender<T> {
    inner: mpsc::Sender<T>,
    name: &'static str,
    capacity: usize,
    full: Arc<AtomicU64>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            name: self.name,
            capacity: self.capacity,
            full: Arc::clone(&self.full),
        }
    }
}

impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sender")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

/// A bounded channel named `name`, holding at most `capacity` messages.
#[must_use]
pub fn bounded<T>(name: &'static str, capacity: NonZeroUsize) -> (Sender<T>, Receiver<T>) {
    let (inner, receiver) = mpsc::channel(capacity.get());
    (Sender { inner, name, capacity: capacity.get(), full: Arc::new(AtomicU64::new(0)) }, receiver)
}

impl<T> Sender<T> {
    /// The channel's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Its maximum.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How often a send found it full.
    #[must_use]
    pub fn full_events(&self) -> u64 {
        self.full.load(Ordering::Relaxed)
    }

    /// Logs the first time the channel was found full and every thousandth time after.
    fn note_full(&self) {
        let n = self.full.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(1_000) {
            tracing::warn!(name: names::CHANNEL_FULL, channel = self.name, capacity = self.capacity, times = n + 1);
        }
    }

    /// Sends, waiting for room when the channel is full (and saying so).
    pub async fn send(&self, value: T) -> Result<(), SendError<T>> {
        match self.inner.try_send(value) {
            Ok(()) => Ok(()),
            Err(TrySendError::Closed(value)) => Err(SendError(value)),
            Err(TrySendError::Full(value)) => {
                self.note_full();
                self.inner.send(value).await
            }
        }
    }

    /// Sends at once, or gives the value back when the channel is full or closed.
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let result = self.inner.try_send(value);
        if matches!(result, Err(TrySendError::Full(_))) {
            self.note_full();
        }
        result
    }

    /// True once every receiver is gone.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}

/// A broadcast channel holding at most `capacity` messages: a slow subscriber gets an
/// explicit `Lagged` error, never silent loss (simulator invariant 6).
#[must_use]
pub fn broadcast<T: Clone>(
    capacity: NonZeroUsize,
) -> (broadcast::Sender<T>, broadcast::Receiver<T>) {
    broadcast::channel(capacity.get())
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use tokio::sync::broadcast::error::TryRecvError;

    use super::{TrySendError, bounded, broadcast};

    const TWO: NonZeroUsize = NonZeroUsize::new(2).unwrap();

    #[test]
    fn a_full_channel_refuses_and_counts() {
        let (tx, mut rx) = bounded::<u32>("test.queue", TWO);
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        assert!(matches!(tx.try_send(3), Err(TrySendError::Full(3))), "full");
        assert_eq!(tx.full_events(), 1, "counted");
        assert_eq!(rx.try_recv().unwrap(), 1, "in order");
        assert_eq!((tx.name(), tx.capacity()), ("test.queue", 2), "named");
    }

    #[test]
    fn a_slow_broadcast_subscriber_is_told_it_lagged() {
        let (tx, mut rx) = broadcast::<u32>(TWO);
        for n in 0..5 {
            tx.send(n).unwrap();
        }
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Lagged(3))), "lagged by three");
    }
}

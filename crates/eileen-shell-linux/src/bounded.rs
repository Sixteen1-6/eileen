//! A read that another process can hold, bounded by its caller's deadline (BUILD.md section 2.1
//! rule 2, and section 3.4 for a method that takes a `Deadline`): it runs on a named thread of
//! its own, and the caller waits at most the deadline for its answer. Past it the caller is
//! answered `DeadlineExceeded` and the read is left to its thread, which ends when the read does,
//! if ever: a read that a network file server or a FUSE daemon holds cannot be interrupted.
//!
//! [`OneRead`] runs reads of one kind one at a time, which `eileen-apps-linux`'s blocking pool
//! does for its reads: while a read is held, a new call waits for that read within its own
//! deadline rather than start another, so a hang keeps one thread however often it is asked.
//!
//! # Traps
//!
//! - Two calls at the same moment while a read is held can each leave one; the slot keeps the
//!   first, and the other's thread ends when its read does.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use eileen_platform::{Deadline, PlatformError};
use eileen_stdx::clock::Clock;

/// What a read that ended without answering says: it panicked.
const ENDED: &str = "the read ended without answering";

/// Reads of one kind, one at a time, each within its caller's deadline.
#[derive(Debug)]
pub(crate) struct OneRead<T> {
    /// The name of the threads the reads run on, within Linux's 15 bytes.
    thread: &'static str,
    /// The answer of a read left past its caller's deadline, while one is.
    held: Mutex<Option<Receiver<T>>>,
}

impl<T: Send + 'static> OneRead<T> {
    /// Reads on threads named `thread`.
    pub(crate) const fn new(thread: &'static str) -> Self {
        Self { thread, held: Mutex::new(None) }
    }

    /// Runs `reading` on a thread of its own and answers what it answers, waiting at most
    /// `deadline`: past it, [`PlatformError::DeadlineExceeded`] naming `call`. While a read an
    /// earlier call left is still held, this call waits for it instead, and reads anew once it
    /// ends within the deadline: the earlier read's answer may be older than the call. The
    /// answer comes through a channel of one place, so the thread never waits to send it.
    pub(crate) fn within(
        &self,
        call: &'static str,
        deadline: Deadline,
        reading: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, PlatformError> {
        let clock = eileen_stdx::clock::system();
        let start = clock.mono();
        let held = self.slot().take();
        if let Some(held) = held
            && matches!(held.recv_timeout(deadline.budget()), Err(RecvTimeoutError::Timeout))
        {
            self.keep(held);
            return Err(PlatformError::DeadlineExceeded { call });
        }
        let spent = Duration::from_nanos(clock.mono().0.saturating_sub(start.0));
        let (answer, answered) = mpsc::sync_channel(1);
        // Detached: the read answers through `answered`, or no one waits for it.
        drop(
            eileen_stdx::thread::spawn_named(self.thread, move || {
                let _ignored = answer.send(reading());
            })
            .map_err(|error| PlatformError::io(call, &error))?,
        );
        match answered.recv_timeout(deadline.budget().saturating_sub(spent)) {
            Ok(answer) => Ok(answer),
            Err(RecvTimeoutError::Timeout) => {
                self.keep(answered);
                Err(PlatformError::DeadlineExceeded { call })
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(PlatformError::Io { what: call, detail: ENDED.to_owned() })
            }
        }
    }

    /// Keeps `read`, held past its caller's deadline, for the next call to wait on, unless
    /// another call has kept one meanwhile.
    fn keep(&self, read: Receiver<T>) {
        let mut held = self.slot();
        if held.is_none() {
            *held = Some(read);
        }
    }

    /// The slot of the held read. A panic cannot leave it half written.
    fn slot(&self) -> MutexGuard<'_, Option<Receiver<T>>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use eileen_platform::{Deadline, PlatformError};

    use super::{ENDED, OneRead};

    /// A deadline no read in these tests comes near, unless it is held.
    const LONG: Deadline = Deadline::after(Duration::from_secs(30));
    /// The deadline of a held read: it passes whatever its length.
    const SHORT: Deadline = Deadline::after(Duration::from_millis(20));

    #[test]
    fn a_read_answers_through_a_named_thread_of_its_own() {
        let reads = OneRead::new("eileen-facts");
        let thread =
            reads.within("a read", LONG, || std::thread::current().name().map(str::to_owned));
        assert_eq!(thread, Ok(Some("eileen-facts".to_owned())), "on the named thread");
    }

    #[test]
    fn a_read_held_past_its_deadline_is_left_to_its_thread() {
        let (release, released) = mpsc::sync_channel::<()>(0);
        let (ended, end) = mpsc::sync_channel(1);
        let held = OneRead::new("eileen-facts").within("a held read", SHORT, move || {
            let _released = released.recv();
            let _ignored = ended.send(());
        });
        assert_eq!(held, Err(PlatformError::DeadlineExceeded { call: "a held read" }), "held");
        drop(release);
        assert_eq!(end.recv(), Ok(()), "the read went on, and ended once released");
    }

    #[test]
    fn while_a_read_is_held_later_calls_wait_for_it_and_start_none() {
        let reads = OneRead::new("eileen-facts");
        let started = Arc::new(AtomicU32::new(0));
        let (release, released) = mpsc::channel::<()>();
        let released = Arc::new(Mutex::new(released));
        let read = || {
            let (started, released) = (Arc::clone(&started), Arc::clone(&released));
            move || {
                let count = started.fetch_add(1, Ordering::SeqCst) + 1;
                let _released = released.lock().map(|released| released.recv());
                count
            }
        };
        let held = Err(PlatformError::DeadlineExceeded { call: "a held read" });
        assert_eq!(reads.within("a held read", SHORT, read()), held, "the first call");
        assert_eq!(reads.within("a held read", SHORT, read()), held, "the second call");
        assert_eq!(started.load(Ordering::SeqCst), 1, "one read, which the second call waited on");

        release.send(()).unwrap();
        release.send(()).unwrap();
        assert_eq!(reads.within("a read", LONG, read()), Ok(2), "once it ends, a read anew");
    }

    #[test]
    fn a_read_that_panics_answers_io() {
        let reads = OneRead::new("eileen-facts");
        let read =
            reads.within("a read", LONG, || -> u32 { panic!("a read that fails an assertion") });
        assert_eq!(read, Err(PlatformError::Io { what: "a read", detail: ENDED.to_owned() }));
    }
}

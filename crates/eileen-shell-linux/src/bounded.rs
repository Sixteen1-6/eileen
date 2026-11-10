//! A read that another process can hold, bounded by its caller's deadline (BUILD.md section 2.1
//! rule 2, and section 3.4 for a method that takes a `Deadline`): it runs on a named thread of
//! its own, and the caller waits at most the deadline for its answer. Past it the caller is
//! answered `DeadlineExceeded` and the read is left to its thread, which ends when the read does,
//! if ever: a read that a network file server or a FUSE daemon holds cannot be interrupted.
//!
//! [`OneRead`] runs reads of one kind one at a time, which `eileen-apps-linux`'s blocking pool
//! does for its reads: from the moment a read starts until it ends, any other call waits for it
//! within its own deadline rather than start another, so a hang keeps one thread however often
//! it is asked, and however the calls overlap.
//!
//! # Traps
//!
//! - While a read is held, every call answers `DeadlineExceeded` at its deadline; the read's
//!   answer goes only to the call that started it, and the first call after it ends reads anew.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use eileen_platform::{Deadline, PlatformError};
use eileen_stdx::clock::Clock;

/// What a read that ended without answering says: it panicked.
const ENDED: &str = "the read ended without answering";

/// Reads of one kind, one at a time, each within its caller's deadline.
#[derive(Debug)]
pub(crate) struct OneRead {
    /// The name of the threads the reads run on, within Linux's 15 bytes.
    thread: &'static str,
    /// Whether a read is running, which its thread clears as it ends.
    running: Arc<Running>,
}

/// Whether a read is running, and the signal of its end.
#[derive(Debug, Default)]
struct Running {
    /// True from a read's start to its end, however long its caller waits.
    read: Mutex<bool>,
    /// Signalled as a read ends.
    ended: Condvar,
}

impl Running {
    /// The flag. Nothing panics while holding it, and a bool cannot be left half written.
    fn flag(&self) -> MutexGuard<'_, bool> {
        self.read.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Clears the flag and signals the end when dropped: as a read ends, panics, or never starts
/// because its thread could not.
struct Ends(Arc<Running>);

impl Drop for Ends {
    fn drop(&mut self) {
        *self.0.flag() = false;
        self.0.ended.notify_all();
    }
}

impl OneRead {
    /// Reads on threads named `thread`.
    pub(crate) fn new(thread: &'static str) -> Self {
        Self { thread, running: Arc::default() }
    }

    /// Runs `reading` on a thread of its own and answers what it answers, waiting at most
    /// `deadline`: past it, [`PlatformError::DeadlineExceeded`] naming `call`. While another
    /// call's read is running, this call waits for it to end instead, and reads anew once it
    /// does within the deadline. The answer comes through a channel of one place, so the
    /// thread never waits to send it.
    pub(crate) fn within<T: Send + 'static>(
        &self,
        call: &'static str,
        deadline: Deadline,
        reading: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, PlatformError> {
        let clock = eileen_stdx::clock::system();
        let start = clock.mono();
        let ends = self.start(call, deadline)?;
        let spent = Duration::from_nanos(clock.mono().0.saturating_sub(start.0));
        let (answer, answered) = mpsc::sync_channel(1);
        // Detached: the read answers through `answered`, or no one waits for it. Its end is
        // signalled before its answer, so a caller with the answer finds no read running.
        drop(
            eileen_stdx::thread::spawn_named(self.thread, move || {
                let read = reading();
                drop(ends);
                let _ignored = answer.send(read);
            })
            .map_err(|error| PlatformError::io(call, &error))?,
        );
        match answered.recv_timeout(deadline.budget().saturating_sub(spent)) {
            Ok(answer) => Ok(answer),
            Err(RecvTimeoutError::Timeout) => Err(PlatformError::DeadlineExceeded { call }),
            Err(RecvTimeoutError::Disconnected) => {
                Err(PlatformError::Io { what: call, detail: ENDED.to_owned() })
            }
        }
    }

    /// Marks a read running once none is, waiting at most `deadline` for the one that is; the
    /// guard it answers clears the mark when dropped.
    fn start(&self, call: &'static str, deadline: Deadline) -> Result<Ends, PlatformError> {
        let (mut running, _waited) = self
            .running
            .ended
            .wait_timeout_while(self.running.flag(), deadline.budget(), |running| *running)
            .unwrap_or_else(PoisonError::into_inner);
        if *running {
            return Err(PlatformError::DeadlineExceeded { call });
        }
        *running = true;
        drop(running);
        Ok(Ends(Arc::clone(&self.running)))
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
    fn a_call_while_another_waits_on_its_read_starts_none() {
        let reads = Arc::new(OneRead::new("eileen-facts"));
        let started = Arc::new(AtomicU32::new(0));
        let (began, begun) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel::<()>(0);
        let first = {
            let (reads, started) = (Arc::clone(&reads), Arc::clone(&started));
            let read = move || {
                started.fetch_add(1, Ordering::SeqCst);
                let _ignored = began.send(());
                let _released = released.recv();
            };
            eileen_stdx::thread::spawn_named("first-call", move || {
                reads.within("a held read", LONG, read)
            })
            .unwrap()
        };
        begun.recv().unwrap();
        // The first call waits for its read's answer meanwhile, as long as its deadline allows.
        let second = {
            let started = Arc::clone(&started);
            reads.within("a held read", SHORT, move || {
                started.fetch_add(1, Ordering::SeqCst);
            })
        };
        assert_eq!(second, Err(PlatformError::DeadlineExceeded { call: "a held read" }), "held");
        assert_eq!(started.load(Ordering::SeqCst), 1, "no read beside the one in flight");
        drop(release);
        assert_eq!(first.join().unwrap(), Ok(()), "the first call has its read's answer");
    }

    #[test]
    fn a_read_that_panics_answers_io() {
        let reads = OneRead::new("eileen-facts");
        let read =
            reads.within("a read", LONG, || -> u32 { panic!("a read that fails an assertion") });
        assert_eq!(read, Err(PlatformError::Io { what: "a read", detail: ENDED.to_owned() }));
    }
}

//! A read that another process can hold, bounded by its caller's deadline (BUILD.md section 2.1
//! rule 2, and section 3.4 for a method that takes a `Deadline`): it runs on a named thread of
//! its own, and the caller waits at most the deadline for its answer. Past it the caller is
//! answered `DeadlineExceeded` and the read is left to its thread, which ends when the read does,
//! if ever: a read that a network file server or a FUSE daemon holds cannot be interrupted.
//!
//! It is `eileen-apps-linux`'s `bounded::within` without the blocking pool: the machine's facts
//! are read at start and on a change, so a thread per read costs little.

use std::sync::mpsc::{self, RecvTimeoutError};

use eileen_platform::{Deadline, PlatformError};

/// The name of the threads the reads run on, within Linux's 15 bytes.
const THREAD: &str = "eileen-facts";
/// What a read that ended without answering says: it panicked.
const ENDED: &str = "the read ended without answering";

/// Runs `reading` on a thread of its own and answers what it answers, waiting at most
/// `deadline`: past it, [`PlatformError::DeadlineExceeded`] naming `call`. The answer comes
/// through a channel of one place, so the thread never waits to send it.
pub(crate) fn within<T: Send + 'static>(
    call: &'static str,
    deadline: Deadline,
    reading: impl FnOnce() -> T + Send + 'static,
) -> Result<T, PlatformError> {
    let (answer, answered) = mpsc::sync_channel(1);
    // Detached: the read answers through `answered`, or no one waits for it.
    drop(
        eileen_stdx::thread::spawn_named(THREAD, move || {
            let _ignored = answer.send(reading());
        })
        .map_err(|error| PlatformError::io(call, &error))?,
    );
    answered.recv_timeout(deadline.budget()).map_err(|error| match error {
        RecvTimeoutError::Timeout => PlatformError::DeadlineExceeded { call },
        RecvTimeoutError::Disconnected => {
            PlatformError::Io { what: call, detail: ENDED.to_owned() }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use eileen_platform::{Deadline, PlatformError};

    use super::{ENDED, within};

    /// A deadline no read in these tests comes near, unless it is held.
    const LONG: Deadline = Deadline::after(Duration::from_secs(30));

    #[test]
    fn a_read_answers_through_a_named_thread_of_its_own() {
        let thread = within("a read", LONG, || std::thread::current().name().map(str::to_owned));
        assert_eq!(thread, Ok(Some("eileen-facts".to_owned())), "on the named thread");
    }

    #[test]
    fn a_read_held_past_its_deadline_is_left_to_its_thread() {
        let (release, released) = mpsc::sync_channel::<()>(0);
        let (ended, end) = mpsc::sync_channel(1);
        let deadline = Deadline::after(Duration::from_millis(20));
        let held = within("a held read", deadline, move || {
            let _released = released.recv();
            let _ignored = ended.send(());
        });
        assert_eq!(held, Err(PlatformError::DeadlineExceeded { call: "a held read" }), "held");
        drop(release);
        assert_eq!(end.recv(), Ok(()), "the read went on, and ended once released");
    }

    #[test]
    fn a_read_that_panics_answers_io() {
        let read = within("a read", LONG, || -> u32 { panic!("a read that fails an assertion") });
        assert_eq!(read, Err(PlatformError::Io { what: "a read", detail: ENDED.to_owned() }));
    }
}

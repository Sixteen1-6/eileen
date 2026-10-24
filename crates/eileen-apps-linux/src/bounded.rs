//! Reads of local files for a trait call that takes no `Deadline`, bounded all the same: each
//! runs on a thread of its own, and the call waits for its answer at most [`DEFAULT_DEADLINE`],
//! as it would for a D-Bus call. A FUSE daemon or a network file server can hold a read for as
//! long as it likes, and BUILD.md section 2.1 (rule 2) gives every call to another process a
//! deadline. Past it the call answers `DeadlineExceeded` and leaves the read to its thread,
//! which ends when the read does, if ever. No read runs on a runtime's blocking pool, which a
//! hung read would hold for good.

use std::sync::mpsc::{self, RecvTimeoutError};

use eileen_dbus_linux::DEFAULT_DEADLINE;
use eileen_platform::{Deadline, PlatformError};

/// The name of the threads the reads run on.
const THREAD: &str = "eileen-file-rd";
/// What a read that ended without answering (it panicked) says.
const ENDED: &str = "the read ended without answering";

/// Runs `reading` on a thread of its own and answers what it answers, waiting at most
/// [`DEFAULT_DEADLINE`]: past it, [`PlatformError::DeadlineExceeded`] naming `call`.
pub(crate) fn read<T: Send + 'static>(
    call: &'static str,
    reading: impl FnOnce() -> T + Send + 'static,
) -> Result<T, PlatformError> {
    within(call, DEFAULT_DEADLINE, reading)
}

/// [`read`] with any deadline. The answer comes through a channel of one place, so the thread
/// never waits to send it: tokio's channels give a thread outside a runtime no timed wait.
fn within<T: Send + 'static>(
    call: &'static str,
    deadline: Deadline,
    reading: impl FnOnce() -> T + Send + 'static,
) -> Result<T, PlatformError> {
    let (answer, answered) = mpsc::sync_channel(1);
    let thread = eileen_stdx::thread::spawn_named(THREAD, move || {
        let _ignored = answer.send(reading());
    });
    // Detached: the read answers through `answered`, or no one waits for it.
    drop(thread.map_err(|error| PlatformError::io(call, &error))?);
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
    fn a_read_answers_through_its_thread() {
        let thread = within("a read", LONG, || std::thread::current().name().map(str::to_owned));
        assert_eq!(thread, Ok(Some("eileen-file-rd".to_owned())), "on a named thread of its own");
    }

    #[test]
    fn a_read_held_past_its_deadline_is_left_to_its_thread() {
        let (release, released) = mpsc::sync_channel::<()>(0);
        let (ended, end) = mpsc::sync_channel(1);
        let held = within("a held read", Deadline::after(Duration::from_millis(20)), move || {
            let _released = released.recv();
            let _ignored = ended.send(());
        });
        assert_eq!(held, Err(PlatformError::DeadlineExceeded { call: "a held read" }), "held");
        drop(release);
        assert_eq!(end.recv(), Ok(()), "the read went on, and ended once released");
    }

    #[test]
    fn a_read_that_panics_answers_io() {
        let read =
            within("a read", LONG, || -> u32 { panic!("a read that fails a soft assertion") });
        assert_eq!(read, Err(PlatformError::Io { what: "a read", detail: ENDED.to_owned() }));
    }
}

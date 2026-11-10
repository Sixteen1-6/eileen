//! `ThreadId` live: the scheduling of the calling thread and of a thread that has ended, read from
//! procfs on this machine.

use std::time::Duration;

use eileen_audio_linux::{Scheduling, ThreadId};
use eileen_platform::PlatformError;
use tokio::time::Instant;

/// How long a thread that has ended may still be listed in `/proc/self/task` after its join.
const UNLISTED: Duration = Duration::from_secs(5);

#[test]
fn the_calling_threads_own_policy_reads_as_sched_other() {
    assert_eq!(
        ThreadId::current().scheduling(),
        Ok(Scheduling::Other),
        "a test's thread has the default policy: only rtkit or a privilege makes one real-time, \
         and the build guard lowers its nice level alone"
    );
}

/// Every thread a test can make is under `SCHED_OTHER`, so only a thread that is gone tells a read
/// of the given thread from a read of the caller's. `join` returns once the kernel has cleared the
/// thread's id, a little before it takes the thread out of `/proc/self/task`, so the read is tried
/// again until then, within [`UNLISTED`].
#[test]
fn a_thread_that_has_ended_reads_as_an_io_error() {
    let ended =
        eileen_stdx::thread::spawn_named("ended", ThreadId::current).unwrap().join().unwrap();
    assert_ne!(ended, ThreadId::current());
    let until = Instant::now() + UNLISTED;
    let read = loop {
        let read = ended.scheduling();
        if read.is_err() || Instant::now() >= until {
            break read;
        }
        std::thread::yield_now();
    };
    assert!(
        matches!(read, Err(PlatformError::Io { what: "/proc/self/task/<tid>/stat", .. })),
        "the read follows the id it is given, and the thread is gone: {read:?}"
    );
}

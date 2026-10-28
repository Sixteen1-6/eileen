//! `ThreadId` live: the calling thread's own scheduling, read from procfs on this machine.

use eileen_audio_linux::{Scheduling, ThreadId};

#[test]
fn the_calling_threads_own_policy_reads_as_sched_other() {
    assert_eq!(
        ThreadId::current().scheduling(),
        Ok(Scheduling::Other),
        "a test's thread has the default policy: only rtkit or a privilege makes one real-time, \
         and the build guard lowers its nice level alone"
    );
}

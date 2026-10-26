//! Building a process's tokio runtime: named threads and a bounded blocking pool.

use std::io;

/// The blocking pool's default bound (tokio's own default is 512 threads).
pub const DEFAULT_MAX_BLOCKING_THREADS: usize = 16;

/// What [`build`] puts after its `name` in the name of every thread the runtime starts.
#[cfg(not(target_os = "linux"))]
const THREAD_SUFFIX: &str = "-worker";

/// What [`build`] puts after its `name` in the name of every thread the runtime starts. It is
/// short, so that a `name` of up to 12 bytes fits the [`LINUX_THREAD_NAME_MAX`] bytes Linux keeps.
#[cfg(target_os = "linux")]
const THREAD_SUFFIX: &str = "-rt";

/// The bytes of a thread name Linux keeps: `TASK_COMM_LEN` (16) less its nul. std cuts a longer
/// name to this before `pthread_setname_np` and says nothing, so `/proc/<pid>/task/<tid>/comm`,
/// gdb, `top -H`, perf and core dumps would show only its start, and two names that share their
/// first 15 bytes as one. Only Rust's own copy, which the logs show, would keep the whole name.
#[cfg(target_os = "linux")]
const LINUX_THREAD_NAME_MAX: usize = 15;

/// The longest `name` whose threads' name, `<name>-rt`, Linux keeps whole: 12 bytes.
#[cfg(target_os = "linux")]
const LINUX_NAME_MAX: usize = LINUX_THREAD_NAME_MAX - THREAD_SUFFIX.len();

/// A multi-threaded runtime with `workers` worker threads (tokio's default when `None`) and at
/// most `max_blocking` blocking threads.
///
/// Every thread the runtime starts, worker or blocking (tokio gives them one name), is named
/// `<name>-worker`, and on Linux `<name>-rt`. Linux keeps 15 bytes of a thread name, so there
/// `name` is at most 12 bytes (`eileen-apps` fits, `eileen-desktop` does not). A longer one fails
/// a soft assertion ([`eileen_stdx::never!`]: a panic under tests, a logged `soft_assert.failed`
/// in a running Eileen), and its threads are named after its first 12 bytes (`eileen-deskt-rt`),
/// so the runtime still starts and the kernel keeps the whole thread name.
pub fn build(
    name: &str,
    workers: Option<usize>,
    max_blocking: usize,
) -> io::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.thread_name(thread_name(name)).max_blocking_threads(max_blocking.max(1)).enable_all();
    if let Some(n) = workers {
        builder.worker_threads(n.max(1));
    }
    builder.build()
}

/// The name of every thread a runtime called `name` starts.
#[cfg(not(target_os = "linux"))]
fn thread_name(name: &str) -> String {
    format!("{name}{THREAD_SUFFIX}")
}

/// The name of every thread a runtime called `name` starts. A `name` that Linux would cut fails
/// a soft assertion and keeps its first [`LINUX_NAME_MAX`] bytes, cut at a character boundary.
#[cfg(target_os = "linux")]
fn thread_name(name: &str) -> String {
    // `floor_char_boundary` answers a boundary, so `get` always finds the start.
    let kept = name.get(..name.floor_char_boundary(LINUX_NAME_MAX)).unwrap_or(name);
    let thread_name = format!("{kept}{THREAD_SUFFIX}");
    eileen_stdx::never!(
        kept.len() < name.len(),
        "the runtime's name {name:?} is {} bytes and Linux keeps {LINUX_THREAD_NAME_MAX} of a \
         thread name, so the runtime's name is at most {LINUX_NAME_MAX} bytes; its threads are \
         named {thread_name:?}",
        name.len()
    );
    thread_name
}

#[cfg(test)]
mod tests {
    use super::build;

    #[test]
    fn worker_threads_carry_the_process_name() {
        let runtime = build("eileen-test", Some(1), 2).unwrap();
        let name = runtime.block_on(async {
            crate::task::spawn("test.name", async {
                std::thread::current().name().map(str::to_owned)
            })
            .await
        });
        let named = if cfg!(target_os = "linux") { "eileen-test-rt" } else { "eileen-test-worker" };
        assert_eq!(name.unwrap().as_deref(), Some(named), "named");
    }

    /// This thread's name as Rust keeps it, which the logs show, and as the kernel keeps it,
    /// which gdb, `top -H` and core dumps show.
    #[cfg(target_os = "linux")]
    fn both_names() -> (Option<String>, String) {
        let kernel = std::fs::read_to_string("/proc/thread-self/comm").unwrap();
        (std::thread::current().name().map(str::to_owned), kernel.trim_end().to_owned())
    }

    /// Both copies of the name on a worker thread and on a blocking thread of `runtime`.
    #[cfg(target_os = "linux")]
    fn names_on(
        runtime: &tokio::runtime::Runtime,
    ) -> [(&'static str, (Option<String>, String)); 2] {
        let (worker, blocking) = runtime.block_on(async {
            let worker = crate::task::spawn("test.worker.name", async { both_names() });
            let blocking = crate::task::spawn_blocking("test.blocking.name", both_names);
            (worker.await.unwrap(), blocking.await.unwrap())
        });
        [("worker", worker), ("blocking", blocking)]
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_thread_keeps_its_whole_name_on_linux() {
        // 12 bytes, the longest name that fits: its threads' names are 15 bytes.
        let runtime = build("eileen-guard", Some(1), 1).unwrap();
        for (thread, (rust, kernel)) in names_on(&runtime) {
            assert_eq!(
                Some(kernel.as_str()),
                rust.as_deref(),
                "the kernel keeps the {thread} thread's whole name"
            );
            assert_eq!(rust.as_deref(), Some("eileen-guard-rt"), "the {thread} thread's name");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_name_that_linux_would_cut_still_starts_with_its_first_12_bytes() {
        use eileen_stdx::soft_assert::{Mode, failures, with_mode};
        // `eileen-speech` (13 bytes) is the shortest name that does not fit, `eileen-desktop`
        // (14) the longest process name of BUILD.md section 2.1. In the last name, the 12th byte
        // is inside a character, so the cut comes one byte earlier.
        let cases = [
            ("eileen-speech", "eileen-speec-rt"),
            ("eileen-desktop", "eileen-deskt-rt"),
            ("eileen-spee\u{e9}", "eileen-spee-rt"),
        ];
        for (name, cut) in cases {
            let before = failures();
            // A running Eileen reports a failed soft assertion and carries on.
            let runtime = with_mode(Mode::Report, || build(name, Some(1), 1)).unwrap();
            assert!(failures() > before, "{name}: a soft assertion failed");
            for (thread, (rust, kernel)) in names_on(&runtime) {
                assert_eq!(rust.as_deref(), Some(cut), "{name}: the {thread} thread's name");
                assert_eq!(Some(kernel.as_str()), rust.as_deref(), "{name}: the kernel keeps it");
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_name_that_linux_would_cut_fails_under_tests() {
        use eileen_stdx::soft_assert::{Mode, with_mode};
        let panicked = std::panic::catch_unwind(|| {
            with_mode(Mode::Panic, || build("eileen-desktop", Some(1), 1))
        })
        .unwrap_err();
        let message = panicked.downcast_ref::<String>().unwrap();
        assert!(message.contains("\"eileen-desktop\""), "names the runtime: {message}");
        assert!(message.contains("at most 12 bytes"), "says what fits: {message}");
        assert!(message.contains("\"eileen-deskt-rt\""), "names its threads: {message}");
    }
}

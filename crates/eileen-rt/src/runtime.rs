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

/// A multi-threaded runtime with `workers` worker threads (tokio's default when `None`) and at
/// most `max_blocking` blocking threads.
///
/// Every thread the runtime starts, worker or blocking (tokio gives them one name), is named
/// `<name>-worker`, and on Linux `<name>-rt`. Linux keeps 15 bytes of a thread name, so there
/// `name` is at most 12 bytes (`eileen-apps` fits, `eileen-desktop` does not), and a longer one is
/// an [`io::ErrorKind::InvalidInput`] error, before any thread starts.
pub fn build(
    name: &str,
    workers: Option<usize>,
    max_blocking: usize,
) -> io::Result<tokio::runtime::Runtime> {
    let thread_name = format!("{name}{THREAD_SUFFIX}");
    #[cfg(target_os = "linux")]
    fits_linux(&thread_name)?;
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.thread_name(thread_name).max_blocking_threads(max_blocking.max(1)).enable_all();
    if let Some(n) = workers {
        builder.worker_threads(n.max(1));
    }
    builder.build()
}

/// Refuses a thread name that Linux would cut.
#[cfg(target_os = "linux")]
fn fits_linux(thread_name: &str) -> io::Result<()> {
    if thread_name.len() <= LINUX_THREAD_NAME_MAX {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "the runtime's thread name {thread_name:?} is {} bytes and Linux keeps \
             {LINUX_THREAD_NAME_MAX}, so the runtime's name is at most {} bytes",
            thread_name.len(),
            LINUX_THREAD_NAME_MAX - THREAD_SUFFIX.len()
        ),
    ))
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

    #[cfg(target_os = "linux")]
    #[test]
    fn every_thread_keeps_its_whole_name_on_linux() {
        // 12 bytes, the longest name that fits: its threads' names are 15 bytes.
        let runtime = build("eileen-guard", Some(1), 1).unwrap();
        let (worker, blocking) = runtime.block_on(async {
            let worker = crate::task::spawn("test.worker.name", async { both_names() });
            let blocking = crate::task::spawn_blocking("test.blocking.name", both_names);
            (worker.await.unwrap(), blocking.await.unwrap())
        });
        for (thread, (rust, kernel)) in [("worker", worker), ("blocking", blocking)] {
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
    fn a_name_that_linux_would_cut_is_refused() {
        // `eileen-speech` (13 bytes) is the shortest name that does not fit, `eileen-desktop`
        // (14) the longest process name of BUILD.md section 2.1.
        for name in ["eileen-speech", "eileen-desktop"] {
            let error = build(name, Some(1), 1).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{name}: {error}");
            let message = error.to_string();
            assert!(message.contains(&format!("\"{name}-rt\"")), "names the thread: {message}");
            assert!(message.contains("at most 12 bytes"), "says what fits: {message}");
        }
    }
}

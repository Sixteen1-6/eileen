//! Building a process's tokio runtime: named worker threads and a bounded blocking pool.

use std::io;

/// The blocking pool's default bound (tokio's own default is 512 threads).
pub const DEFAULT_MAX_BLOCKING_THREADS: usize = 16;

/// A multi-threaded runtime whose threads are named `<name>-worker`, with `workers` worker
/// threads (tokio's default when `None`) and at most `max_blocking` blocking threads.
pub fn build(
    name: &str,
    workers: Option<usize>,
    max_blocking: usize,
) -> io::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder
        .thread_name(format!("{name}-worker"))
        .max_blocking_threads(max_blocking.max(1))
        .enable_all();
    if let Some(n) = workers {
        builder.worker_threads(n.max(1));
    }
    builder.build()
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
        assert_eq!(name.unwrap().as_deref(), Some("eileen-test-worker"), "named");
    }
}

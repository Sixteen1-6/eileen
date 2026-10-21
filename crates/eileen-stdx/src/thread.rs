//! Named threads and sleep.
//!
//! Every thread has a name, so it shows up as itself in logs, minidumps and the debugger. The
//! `-win` crates run UI Automation, COM and input on their own threads through this module, and
//! sleep only here, on a named thread, never on a runtime worker.

use std::io;
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

/// Starts a thread named `name` with the default stack size.
pub fn spawn_named<F, T>(name: impl Into<String>, f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_named_with_stack(name, None, f)
}

/// Starts a thread named `name`, with `stack_bytes` of stack when given (the simulator asks
/// for a large one, since Windows' default main-thread stack is small).
#[expect(
    clippy::disallowed_methods,
    reason = "the one way Eileen starts a thread (BUILD.md section 4b.2)"
)]
pub fn spawn_named_with_stack<F, T>(
    name: impl Into<String>,
    stack_bytes: Option<usize>,
    f: F,
) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let mut builder = Builder::new().name(name.into());
    if let Some(bytes) = stack_bytes {
        builder = builder.stack_size(bytes);
    }
    builder.spawn(f)
}

/// Blocks the current thread for `duration`. Only for named threads that own their loop
/// (a COM apartment, a sampler); async code uses `tokio::time::sleep`.
#[expect(clippy::disallowed_methods, reason = "the one sleep (BUILD.md section 4b.2)")]
pub fn sleep(duration: Duration) {
    std::thread::sleep(duration);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{sleep, spawn_named, spawn_named_with_stack};

    #[test]
    fn threads_carry_their_name() {
        let handle = spawn_named("eileen-test-worker", || {
            sleep(Duration::from_millis(1));
            std::thread::current().name().map(str::to_owned)
        })
        .unwrap();
        assert_eq!(handle.join().unwrap().as_deref(), Some("eileen-test-worker"), "name");
    }

    #[test]
    fn a_large_stack_is_honoured() {
        // Recursion that needs more than the 2 MB default would overflow without it.
        fn depth(n: u32) -> u32 {
            let pad = [n; 64];
            if n == 0 { 0 } else { std::hint::black_box(pad)[0].min(1) + depth(n - 1) }
        }
        let handle = spawn_named_with_stack("eileen-test-deep", Some(64 << 20), || depth(20_000));
        assert_eq!(handle.unwrap().join().unwrap(), 20_000, "ran to the end");
    }
}

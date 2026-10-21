//! The one way Eileen starts a task: named, in the span that started it, registered with
//! `await-tree`, and with a panic logged as `task.panicked` before it fails its handle.
//!
//! Without the span, work started inside a turn writes log lines with no `turn_id`; without
//! the tree, "a job hung" has no answer on Windows (observability report sections 2.4 and
//! 3.7). Bare `tokio::spawn` and `spawn_blocking` are banned by clippy everywhere else.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::task::{Context, Poll};

use eileen_stdx::clock::{self, Clock as _};
use eileen_trace::names;
use tokio::task::JoinHandle;
use tracing::Instrument as _;
use tracing::instrument::WithSubscriber as _;

/// A task's key in the await-tree registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskKey {
    /// Its name.
    pub name: &'static str,
    /// A number unique in this process.
    pub id: u64,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The process's await-tree registry.
pub fn registry() -> &'static await_tree::Registry {
    static REGISTRY: OnceLock<await_tree::Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| await_tree::Registry::new(await_tree::Config::default()))
}

/// The blocking tasks running now: id to name and start (monotonic nanoseconds).
fn blocking() -> &'static Mutex<BTreeMap<u64, (&'static str, u64)>> {
    static BLOCKING: OnceLock<Mutex<BTreeMap<u64, (&'static str, u64)>>> = OnceLock::new();
    BLOCKING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// A panic's message, with any backtick-quoted part replaced, because a slicing panic quotes
/// the user's string ("byte index 4 is out of bounds of `...`").
#[must_use]
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    let raw = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with a non-text payload".to_owned());
    let mut out = String::with_capacity(raw.len());
    for (i, part) in raw.split('`').enumerate() {
        if i % 2 == 0 {
            out.push_str(part);
        } else {
            out.push_str("`<redacted>`");
        }
    }
    out
}

fn log_panic(task: &'static str, payload: &(dyn Any + Send)) {
    tracing::error!(name: names::TASK_PANICKED, task, message = %panic_message(payload));
}

/// Logs a panic of the future it wraps, then lets the panic continue to the join handle.
struct CatchPanic<T> {
    name: &'static str,
    inner: Pin<Box<dyn Future<Output = T> + Send>>,
}

impl<T> Future for CatchPanic<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let name = self.name;
        match catch_unwind(AssertUnwindSafe(|| self.inner.as_mut().poll(cx))) {
            Ok(poll) => poll,
            Err(payload) => {
                log_panic(name, payload.as_ref());
                resume_unwind(payload)
            }
        }
    }
}

/// Starts `future` as a task named `name`, in the current span and with the current
/// subscriber, registered with await-tree.
pub fn spawn<F>(name: &'static str, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let key = TaskKey { name, id: NEXT_ID.fetch_add(1, Ordering::Relaxed) };
    let root = registry().register(key, name);
    let traced =
        root.instrument(future.instrument(tracing::Span::current())).with_current_subscriber();
    spawn_raw(CatchPanic { name, inner: Box::pin(traced) })
}

#[expect(
    clippy::disallowed_methods,
    reason = "eileen_rt::task::spawn: the one tokio::spawn (BUILD.md section 4b.2)"
)]
fn spawn_raw<T: Send + 'static>(future: CatchPanic<T>) -> JoinHandle<T> {
    tokio::spawn(future)
}

/// Removes a blocking task from the list when it ends, panic or not.
struct Running(u64);

impl Drop for Running {
    fn drop(&mut self) {
        blocking().lock().unwrap_or_else(PoisonError::into_inner).remove(&self.0);
    }
}

/// Runs `f` on the runtime's blocking pool (bounded by the runtime's
/// `max_blocking_threads`), named, in the current span, listed by [`dump`] while it runs.
#[expect(
    clippy::disallowed_methods,
    reason = "eileen_rt::task::spawn_blocking: the one tokio::task::spawn_blocking (BUILD.md section 4b.2)"
)]
pub fn spawn_blocking<F, R>(name: &'static str, f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let span = tracing::Span::current();
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    blocking()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, (name, clock::system().mono().0));
    tokio::task::spawn_blocking(move || {
        let _running = Running(id);
        tracing::dispatcher::with_default(&dispatch, || {
            let _entered = span.enter();
            match catch_unwind(AssertUnwindSafe(f)) {
                Ok(value) => value,
                Err(payload) => {
                    log_panic(name, payload.as_ref());
                    resume_unwind(payload)
                }
            }
        })
    })
}

/// The await tree of every live task and the blocking tasks running now, as text
/// (`eileen debug tasks`, `tasks.txt` in an incident).
#[must_use]
pub fn dump() -> String {
    let mut trees = registry().collect::<TaskKey>();
    trees.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (key, tree) in trees {
        let _infallible = writeln!(out, "[{} #{}]\n{tree}", key.name, key.id);
    }
    let now = clock::system().mono().0;
    for (id, (name, start)) in blocking().lock().unwrap_or_else(PoisonError::into_inner).iter() {
        let _infallible = writeln!(
            out,
            "[{name} #{id}] blocking for {} ms",
            now.saturating_sub(*start) / 1_000_000
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::panic_message;

    #[test]
    fn panic_messages_lose_their_quoted_user_text() {
        let payload: Box<dyn std::any::Any + Send> =
            Box::new("byte index 4 is out of bounds of `my secret title`".to_owned());
        assert_eq!(
            panic_message(payload.as_ref()),
            "byte index 4 is out of bounds of `<redacted>`",
            "quoted part replaced"
        );
        let payload: Box<dyn std::any::Any + Send> = Box::new(7_u32);
        assert_eq!(
            panic_message(payload.as_ref()),
            "a panic with a non-text payload",
            "non-text payload"
        );
    }
}

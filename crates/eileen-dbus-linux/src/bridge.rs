//! [`Bridge`]: where the synchronous platform traits meet the async D-Bus clients.
//!
//! A bridge owns one named thread running a current-thread tokio runtime. A trait method posts
//! a job there and waits for its answer within the call's [`Deadline`] (BUILD.md section 2.1,
//! rule 2); past it, the call answers [`PlatformError::DeadlineExceeded`] and the job is
//! cancelled, while the thread goes on serving the others. A subscription's stream runs there
//! as a task until its [`Subscription`] drops. The jobs reach the buses through [`Buses`],
//! which connects each on first use and keeps it, so every connection and every stream is made,
//! used and dropped on the bridge's thread, inside its runtime (the trap in `src/stream.rs`).
//!
//! Each job runs as a task of the thread's `LocalSet` (`tokio::task::spawn_local`), so many
//! are in flight at once and none has to be `Send`. clippy.toml bans `tokio::spawn` and
//! `spawn_blocking` for `eileen_rt::task`, which this crate may not depend on (BUILD.md section
//! 3.1); a local task on the bridge's own `LocalSet` is neither.

mod reply;

use std::convert::Infallible;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use eileen_platform::{Deadline, PlatformError, Subscription};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot};
use tokio::task::LocalSet;

use crate::{DbusError, METHOD_TIMEOUT, Stream, drain};
use reply::Outcome;

/// The deadline of a trait method that takes none: [`METHOD_TIMEOUT`] plus half a second. A job
/// that makes one call then fails on its connection's own timeout, whose error names the D-Bus
/// call, before the bridge gives up on the job as a whole, provided connecting was quick.
/// Connecting has no timeout: zbus 5.19 bounds neither the SASL handshake nor the `Hello` reply
/// it reads within it, so a bus that accepts and never answers holds the job to this deadline.
pub const DEFAULT_DEADLINE: Deadline =
    Deadline::after(METHOD_TIMEOUT.saturating_add(Duration::from_millis(500)));

/// The thread's name, as logs and dumps show it.
const THREAD: &str = "eileen-dbus";
/// The name of the runtime's blocking threads, on which zbus connects a Unix socket.
const BLOCKING_THREAD: &str = "eileen-dbus-blk";
/// At most this many blocking threads: one per bus being connected.
const BLOCKING_THREADS: usize = 2;
/// How many posted jobs wait for the thread, at most. The thread takes each at once, so the
/// queue is full only when the thread is stuck.
const QUEUE: usize = 64;
/// How long the thread's end waits for a blocking thread still connecting, at most.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
/// What errors name the bridge itself.
const BRIDGE: &str = "the D-Bus bridge";
/// The detail of a call whose job ended without answering.
const DROPPED: &str = "the job ended without answering";
/// Why a call made on the bridge's own thread is refused.
const REENTERED: &str = "made on the D-Bus bridge's own thread, which its wait would stall";

/// A job as the thread takes it: given the buses, the future it runs as a local task.
type Job = Box<dyn FnOnce(Buses) -> LocalJob + Send>;
/// A job's future, which stays on the thread and need not be `Send`.
type LocalJob = Pin<Box<dyn Future<Output = ()>>>;

/// A handle to a bridge's thread. Cloning one is cheap (an `Arc`), and the clones share the
/// thread, so each adapter keeps its own. The thread ends once the last clone and the last
/// [`Subscription`] from [`Bridge::watch`] have dropped: that drop ends the thread's jobs and
/// connections there and waits for it, a short wait, since no job blocks the thread.
#[derive(Debug, Clone)]
pub struct Bridge {
    shared: Arc<Shared>,
}

/// What the handles share. Fields drop in order: the queue closes, which ends the thread's
/// loop, before the join waits for the thread, which is held only for that.
#[derive(Debug)]
struct Shared {
    jobs: mpsc::Sender<Job>,
    thread: Joiner,
}

/// Joins the thread when dropped, unless dropped on the thread itself (by a job that held the
/// last handle), which then ends on its own. A job's panic ends only its task, so a thread
/// that panicked is a fault of the bridge's own: a soft assertion, which a test fails on.
#[derive(Debug)]
struct Joiner(Option<JoinHandle<()>>);

impl Joiner {
    /// Whether the caller runs on the bridge's thread.
    fn is_current(&self) -> bool {
        self.0.as_ref().is_some_and(|thread| thread.thread().id() == std::thread::current().id())
    }
}

impl Drop for Joiner {
    fn drop(&mut self) {
        if self.is_current() {
            return;
        }
        if let Some(thread) = self.0.take() {
            let ended = thread.join();
            // Not while a panic unwinds: a second one would abort the process.
            if !std::thread::panicking() {
                eileen_stdx::never!(ended.is_err(), "the D-Bus bridge's thread panicked");
            }
        }
    }
}

impl Bridge {
    /// Starts a bridge to the user's session bus and the system bus, each connected on first
    /// use by [`crate::session_bus`] or [`crate::system_bus`].
    pub fn start() -> Result<Self, PlatformError> {
        Self::start_with(Source::Session, Source::System)
    }

    /// Starts a bridge whose session bus and system bus are both `bus`, for tests: no test
    /// meets the real buses.
    #[cfg(feature = "private-bus")]
    pub fn on_private_bus(bus: &crate::testing::PrivateBus) -> Result<Self, PlatformError> {
        let address = bus.address();
        Self::start_with(Source::Address(address.to_owned()), Source::Address(address.to_owned()))
    }

    /// Starts the thread, its buses at `session` and `system`.
    fn start_with(session: Source, system: Source) -> Result<Self, PlatformError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .thread_name(BLOCKING_THREAD)
            .max_blocking_threads(BLOCKING_THREADS)
            .build()
            .map_err(|error| PlatformError::io(BRIDGE, &error))?;
        let (jobs, queue) = mpsc::channel(QUEUE);
        let thread = eileen_stdx::thread::spawn_named(THREAD, move || {
            run(&runtime, queue, session, system);
            runtime.shutdown_timeout(SHUTDOWN_GRACE);
        })
        .map_err(|error| PlatformError::io(BRIDGE, &error))?;
        Ok(Self { shared: Arc::new(Shared { jobs, thread: Joiner(Some(thread)) }) })
    }

    /// Runs `job` on the bridge and answers what it answers, waiting at most `deadline`. Past
    /// it, the answer is [`PlatformError::DeadlineExceeded`] naming `call`, and the job is
    /// cancelled: the bridge's thread drops it the next time it would run it, so a job still
    /// queued then never begins. A job that ends without answering (it panicked) answers
    /// [`PlatformError::Io`] at once. Made on the bridge's own thread (in a job or a sink), a
    /// call answers [`PlatformError::Refused`] at once: its wait would stall the thread its job
    /// needs until the deadline.
    ///
    /// A job answers plain data, never a zbus connection, proxy or stream, which are `Send` but
    /// belong to the bridge's runtime: dropped by the caller, a stream or a proxy that subscribed
    /// panics (zbus finds no reactor), and a connection stops working once the bridge has ended.
    pub fn call<T, F, Fut>(
        &self,
        call: &'static str,
        deadline: Deadline,
        job: F,
    ) -> Result<T, PlatformError>
    where
        F: FnOnce(Buses) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, PlatformError>> + 'static,
        T: Send + 'static,
    {
        let (reply, answer) = reply::pair();
        let (stop, stopped) = oneshot::channel::<Infallible>();
        self.post(
            call,
            Box::new(move |buses| {
                Box::pin(until_stopped(stopped, async move {
                    reply.send(job(buses).await);
                }))
            }),
        )?;
        let outcome = answer.wait(deadline.budget());
        // Cancels the job if it is still running.
        drop(stop);
        answer_of(call, outcome)
    }

    /// Opens a stream with `open` on the bridge, waiting at most `deadline` for it, then hands
    /// each of its items to `each` there, until the stream ends or the [`Subscription`] drops.
    /// This returns once `open` has answered, so a stream that subscribes there, as
    /// [`subscribe`](crate::subscribe) does, brings every change made after. Past the deadline
    /// it answers [`PlatformError::DeadlineExceeded`], and a stream that opens later is dropped
    /// unused. Items are taken through [`drain`], so a flood yields to the other jobs; `each`
    /// runs on the bridge's thread and must not block it. Once the `Subscription`'s drop has
    /// returned, `each` runs at most once more: the call already under way. As [`Bridge::call`]
    /// does, this answers [`PlatformError::Refused`] on the bridge's own thread, and `each`
    /// hands on plain data, never a zbus connection, proxy or stream.
    ///
    /// Dropping the `Subscription` that is the bridge's last handle joins its thread, which
    /// first ends the `each` under way: a sink must not lock what the dropping thread holds.
    pub fn watch<S, F, Fut, E>(
        &self,
        call: &'static str,
        deadline: Deadline,
        open: F,
        mut each: E,
    ) -> Result<Subscription, PlatformError>
    where
        F: FnOnce(Buses) -> Fut + Send + 'static,
        Fut: Future<Output = Result<S, PlatformError>> + 'static,
        S: Stream + Unpin + 'static,
        E: FnMut(S::Item) + Send + 'static,
    {
        let (reply, answer) = reply::pair();
        let (stop, stopped) = oneshot::channel::<Infallible>();
        // Set by the subscription's drop, before the stop, and read before each item: after
        // the drop, only an item already under way reaches `each`.
        let dropped = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&dropped);
        let consume = move |buses: Buses| -> LocalJob {
            Box::pin(until_stopped(stopped, async move {
                let mut stream = match open(buses).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        reply.send(Err(error));
                        return;
                    }
                };
                // The caller has given up on a stream that opened too late: it drops here.
                if !reply.send(Ok(())) {
                    return;
                }
                let mut take = |item: S::Item| {
                    if ended.load(Ordering::Acquire) {
                        return Some(());
                    }
                    each(item);
                    None
                };
                poll_fn(|cx| drain(&mut stream, cx, &mut take)).await;
            }))
        };
        self.post(call, Box::new(consume))?;
        // On an error `stop` drops here, so a job still opening its stream stops at once.
        answer_of(call, answer.wait(deadline.budget()))?;
        let shared = Arc::clone(&self.shared);
        Ok(Subscription::new(move || {
            dropped.store(true, Ordering::Release);
            drop(stop);
            drop(shared);
        }))
    }

    /// Puts `job` on the thread's queue, unless the caller is that thread.
    fn post(&self, call: &'static str, job: Job) -> Result<(), PlatformError> {
        if self.shared.thread.is_current() {
            return Err(PlatformError::Refused { what: call, reason: REENTERED });
        }
        self.shared.jobs.try_send(job).map_err(|error| match error {
            // Only a stuck thread leaves the queue full: the call would pass its deadline.
            mpsc::error::TrySendError::Full(_) => PlatformError::DeadlineExceeded { call },
            mpsc::error::TrySendError::Closed(_) => PlatformError::Unavailable { what: BRIDGE },
        })
    }
}

/// What a caller answers for `call` once its wait has ended as `outcome`.
fn answer_of<T>(
    call: &'static str,
    outcome: Outcome<Result<T, PlatformError>>,
) -> Result<T, PlatformError> {
    match outcome {
        Outcome::Answered(result) => result,
        Outcome::Late => Err(PlatformError::DeadlineExceeded { call }),
        Outcome::Dropped => Err(PlatformError::Io { what: call, detail: DROPPED.to_owned() }),
    }
}

/// Runs `work` until it ends or the sender of `stopped` drops, whichever comes first. The stop
/// is polled first, so `work` takes no step once its caller has stopped waiting.
async fn until_stopped(stopped: oneshot::Receiver<Infallible>, work: impl Future<Output = ()>) {
    tokio::select! {
        biased;
        _stopped = stopped => {}
        () = work => {}
    }
}

/// The thread's work: runs the jobs until every handle has dropped, then drops what is left of
/// them, and the connections, inside the runtime's context.
fn run(runtime: &Runtime, mut queue: mpsc::Receiver<Job>, session: Source, system: Source) {
    let buses = Buses(Rc::new(Both { session: Lazy::new(session), system: Lazy::new(system) }));
    let jobs = LocalSet::new();
    runtime.block_on(jobs.run_until(async {
        while let Some(job) = queue.recv().await {
            // Detached: the job answers through its reply and stops when its caller does.
            drop(tokio::task::spawn_local(job(buses.clone())));
        }
    }));
    // A watch whose subscription has just dropped may not have run since, so its stream drops
    // with the set. zbus queues the removal of the stream's match rule on the runtime; outside
    // its context it panics instead, and tokio, which drops a task's future under
    // `catch_unwind`, would only print that panic: no test can see this line go missing.
    let _context = runtime.enter();
    drop(jobs);
    drop(buses);
}

/// The buses a job uses: the user's session bus and the system bus, each connected on first
/// use, kept for the jobs after, and connected again once it has closed. Clones are cheap and
/// share the connections. A job gets one on the bridge's thread, where it stays: it is not
/// `Send`.
#[derive(Debug, Clone)]
pub struct Buses(Rc<Both>);

/// The two buses.
#[derive(Debug)]
struct Both {
    session: Lazy,
    system: Lazy,
}

impl Buses {
    /// The session bus, connected now if it is not, or no longer.
    pub async fn session(&self) -> Result<zbus::Connection, DbusError> {
        self.0.session.get().await
    }

    /// The system bus, connected now if it is not, or no longer.
    pub async fn system(&self) -> Result<zbus::Connection, DbusError> {
        self.0.system.get().await
    }

    /// A new connection to the session bus for this job alone, unlike [`Buses::session`]: it
    /// is not kept, nor shared with any other job. The job owns it, and it closes once the job
    /// drops it on the bridge's thread, which ends what the bus holds for it while it is on the
    /// bus (the desktop's hold that `LinuxPowerRequest::keep_awake` takes, for one). A bridge on
    /// a test's private bus connects it there.
    pub async fn unshared_session(&self) -> Result<zbus::Connection, DbusError> {
        self.0.session.source.connect().await
    }
}

/// One bus: where it is, and its connection once made.
#[derive(Debug)]
struct Lazy {
    source: Source,
    connection: tokio::sync::Mutex<Option<zbus::Connection>>,
}

impl Lazy {
    fn new(source: Source) -> Self {
        Self { source, connection: tokio::sync::Mutex::new(None) }
    }

    /// The open connection, made now if there is none or it has closed. The lock is held while
    /// connecting, so jobs that ask at once share one connection.
    async fn get(&self) -> Result<zbus::Connection, DbusError> {
        let mut connection = self.connection.lock().await;
        if let Some(open) = connection.as_ref().filter(|open| !open.is_closed()) {
            return Ok(open.clone());
        }
        let fresh = self.source.connect().await?;
        *connection = Some(fresh.clone());
        drop(connection);
        Ok(fresh)
    }
}

/// Where a bus is.
#[derive(Debug)]
enum Source {
    Session,
    System,
    /// A private bus's address, for tests.
    #[cfg(feature = "private-bus")]
    Address(String),
}

impl Source {
    /// A new connection to the bus, with [`METHOD_TIMEOUT`].
    async fn connect(&self) -> Result<zbus::Connection, DbusError> {
        match self {
            Self::Session => crate::session_bus().await,
            Self::System => crate::system_bus().await,
            #[cfg(feature = "private-bus")]
            Self::Address(address) => {
                const WHAT: &str = "the private bus";
                let builder = zbus::connection::Builder::address(address.as_str())
                    .map_err(|error| DbusError::from_zbus(WHAT, &error))?;
                crate::bus::build_within(builder, WHAT, METHOD_TIMEOUT).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eileen_platform::PlatformError;
    use tokio::sync::mpsc;

    use super::{BRIDGE, Bridge, Job, Joiner, Shared};

    /// A job that does nothing.
    fn idle() -> Job {
        Box::new(|_buses| Box::pin(async {}))
    }

    #[test]
    fn a_full_queue_answers_deadline_exceeded_and_a_closed_one_unavailable() {
        // No thread: the test holds the queue's other end, and takes nothing from it.
        let (jobs, queue) = mpsc::channel(1);
        let bridge = Bridge { shared: Arc::new(Shared { jobs, thread: Joiner(None) }) };
        assert_eq!(bridge.post("first", idle()), Ok(()), "a free place");
        let full = PlatformError::DeadlineExceeded { call: "second" };
        assert_eq!(bridge.post("second", idle()), Err(full), "as a stuck thread leaves it");
        drop(queue);
        let gone = PlatformError::Unavailable { what: BRIDGE };
        assert_eq!(bridge.post("third", idle()), Err(gone), "as a thread that has gone leaves it");
    }
}

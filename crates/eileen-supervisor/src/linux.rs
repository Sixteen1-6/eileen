//! The Linux process model (docs/LINUX.md section 5).
//!
//! Every child is forked on one thread, `supervisor.fork`, that lives as long as the process,
//! because a child's parent-death signal fires when the thread that forked it ends. The child
//! leads a session of its own (the `ffi` setup), so its pid is also its process group's id.
//!
//! [`GroupChild`] kills that group only while the leader is unreaped: until it is reaped its pid
//! cannot be reused, so the group's id names no other group. A pidfd turns readable when the
//! leader exits and `waitid(WNOWAIT)` reads that without reaping it, so `wait` and `try_wait`
//! kill what is left of the group first and only then let tokio reap the leader. After the reap
//! nothing here signals anything: tokio's `id()` is `None` from then on, and `start_kill` does
//! nothing.

use std::future::Future;
use std::io;
use std::os::fd::{AsFd as _, OwnedFd};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::OnceLock;
use std::sync::mpsc::{SyncSender, sync_channel};

use process_wrap::tokio::{ChildWrapper, CommandWrap};
use rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, kill_process_group, pidfd_open, waitid,
};
use tokio::io::unix::AsyncFd;

/// Spawns `wrap` on the fork thread, inside the caller's tokio runtime, and wraps the child so
/// its whole group dies with it.
pub(crate) fn spawn(wrap: CommandWrap) -> io::Result<Box<dyn ChildWrapper>> {
    let inner = fork(wrap)?;
    Ok(Box::new(GroupChild::new(inner)?))
}

type Job = Box<dyn FnOnce() + Send>;

fn fork(mut wrap: CommandWrap) -> io::Result<Box<dyn ChildWrapper>> {
    let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
    let (reply, answer) = sync_channel(1);
    let job: Job = Box::new(move || {
        let _entered = runtime.enter();
        let _ignored = reply.send(wrap.spawn());
    });
    let not_running = || io::Error::other("the supervisor's fork thread is not running");
    fork_thread().ok_or_else(not_running)?.send(job).map_err(|_| not_running())?;
    answer.recv().map_err(|_| io::Error::other("spawning panicked on the fork thread"))?
}

/// The fork thread's queue. The thread never ends: its end would kill every child it forked,
/// so a job that panics is caught and only that spawn fails.
fn fork_thread() -> Option<&'static SyncSender<Job>> {
    static THREAD: OnceLock<Option<SyncSender<Job>>> = OnceLock::new();
    THREAD
        .get_or_init(|| {
            let (jobs, queue) = sync_channel::<Job>(0);
            let started = eileen_stdx::thread::spawn_named("supervisor.fork", move || {
                for job in queue {
                    let _survived = catch_unwind(AssertUnwindSafe(job));
                }
            });
            started.ok().map(|_detached| jobs)
        })
        .as_ref()
}

fn pid(id: u32) -> io::Result<Pid> {
    i32::try_from(id).ok().and_then(Pid::from_raw).ok_or_else(|| io::ErrorKind::InvalidInput.into())
}

/// Sends `SIGKILL` to the process group `leader` leads. Sound only while the leader is
/// unreaped, which every caller checks through tokio's `id()`.
fn kill_group(leader: u32) -> io::Result<()> {
    Ok(kill_process_group(pid(leader)?, Signal::KILL)?)
}

/// A supervised child on Linux: tokio's child, and a pidfd that tells when it has exited.
#[derive(Debug)]
pub(crate) struct GroupChild {
    inner: Box<dyn ChildWrapper>,
    exit: AsyncFd<OwnedFd>,
}

impl GroupChild {
    /// Opens the pidfd of `inner`, which nothing has reaped yet. On failure the group is
    /// killed, and dropping `inner` kills and reaps the leader.
    fn new(inner: Box<dyn ChildWrapper>) -> io::Result<Self> {
        let id = inner.id().ok_or_else(|| io::Error::other("the child was reaped at once"))?;
        let exit = pid(id)
            .and_then(|pid| Ok(pidfd_open(pid, PidfdFlags::empty())?))
            .and_then(crate::ffi::watch_pidfd);
        match exit {
            Ok(exit) => Ok(Self { inner, exit }),
            Err(error) => {
                let _ignored = kill_group(id);
                Err(error)
            }
        }
    }

    /// Whether the leader has exited, leaving it unreaped.
    fn has_exited(&self) -> io::Result<bool> {
        let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
        Ok(waitid(WaitId::PidFd(self.exit.get_ref().as_fd()), options)?.is_some())
    }

    /// Waits until the leader has exited, leaving it unreaped.
    async fn exited(&self) -> io::Result<()> {
        loop {
            let mut ready = self.exit.readable().await?;
            if self.has_exited()? {
                return Ok(());
            }
            ready.clear_ready();
        }
    }
}

impl ChildWrapper for GroupChild {
    fn inner(&self) -> &dyn ChildWrapper {
        self.inner.as_ref()
    }

    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        self.inner.as_mut()
    }

    /// Tokio's child alone: nothing kills the group for it any more.
    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        self.inner
    }

    /// Kills the whole group while the leader is unreaped; once it is reaped, the group was
    /// killed before the reap and there is nothing left to signal.
    fn start_kill(&mut self) -> io::Result<()> {
        self.inner.id().map_or(Ok(()), kill_group)
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(id) = self.inner.id() {
            if !self.has_exited()? {
                return Ok(None);
            }
            let _ignored = kill_group(id);
        }
        // Tokio reaps it, marks it done and stops tracking the pid.
        self.inner.try_wait()
    }

    fn wait(&mut self) -> Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(async move {
            if let Some(id) = self.inner.id() {
                self.exited().await?;
                let _ignored = kill_group(id);
            }
            self.inner.wait().await
        })
    }
}

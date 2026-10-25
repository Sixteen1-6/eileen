//! The console when its terminal stops reading: Ctrl-S on a Linux terminal (`IXON` is on by
//! default), a mouse selection in a Windows console, or a stalled SSH session blocks every write
//! to stderr. A thread that logs must not wait for it (BUILD.md section 2.1: the core links
//! nothing that can hang).

use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use eileen_trace::{Console, ProcessKind, TraceConfig};
use tracing::Dispatch;

/// How long a warning may take before the test calls it blocked. A warning returns at once when
/// the console writes on a thread of its own, so only a failing run waits this long.
const DEADLINE: Duration = Duration::from_secs(5);

/// Stderr's lock, held on a thread of its own until this is dropped. A write to a terminal that
/// stopped reading blocks while it holds that lock, so every other write to stderr waits on it:
/// holding the lock stands in for the stopped terminal, on every platform.
struct StoppedTerminal {
    resume: Option<SyncSender<()>>,
    holder: Option<JoinHandle<()>>,
}

impl StoppedTerminal {
    fn stop() -> Self {
        let (held_tx, held_rx) = sync_channel(1);
        let (resume_tx, resume_rx) = sync_channel::<()>(1);
        let holder = eileen_stdx::thread::spawn_named("stopped-stderr", move || {
            let _lock = std::io::stderr().lock();
            held_tx.send(()).unwrap();
            // Until the test resumes the terminal, or drops the sender as it unwinds.
            let _ignored = resume_rx.recv();
        })
        .unwrap();
        held_rx.recv().unwrap();
        Self { resume: Some(resume_tx), holder: Some(holder) }
    }
}

impl Drop for StoppedTerminal {
    fn drop(&mut self) {
        drop(self.resume.take());
        if let Some(holder) = self.holder.take() {
            let _ignored = holder.join();
        }
    }
}

#[test]
fn a_warning_never_waits_for_a_stopped_terminal() {
    let mut config = TraceConfig::new(None);
    config.console = Console::Always;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    let stack = Dispatch::new(subscriber);
    let terminal = StoppedTerminal::stop();
    let (returned_tx, returned_rx) = sync_channel(1);
    let warner = eileen_stdx::thread::spawn_named("warner", move || {
        tracing::dispatcher::with_default(&stack, || {
            tracing::warn!(target: "eileen_core", "a warning while the terminal is stopped");
        });
        let _ignored = returned_tx.send(());
    })
    .unwrap();
    let returned = returned_rx.recv_timeout(DEADLINE).is_ok();
    // The terminal reads again: a warning that waited goes through, and the console's own
    // thread writes the line it was given.
    drop(terminal);
    warner.join().unwrap();
    drop(guard);
    assert!(returned, "a warning waited {DEADLINE:?} for a terminal that stopped reading");
}

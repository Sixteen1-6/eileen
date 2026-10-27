//! [`LinuxAudioEndpoints`]: `eileen-platform`'s `AudioEndpoints` on Linux (BUILD.md section 3.4;
//! docs/LINUX.md section 4), on `eileen-dbus-linux`'s [`Bridge`]. It is thin: the behaviour is
//! [`RealtimeKit`]'s.
//!
//! - `register_audio_thread`: [`RealtimeKit::promote`] for the calling thread, on the system bus.
//! - `set_communications_ducking`: nothing to do to opt out, and nothing to opt back into.
//! - `default_capture` and `default_render_loopback`: `Unavailable`, since `PipeWire`'s graph is
//!   not on D-Bus. They wait for phase L2's audio-server client, the owner's choice: the
//!   `pipewire` binding, which BUILD.md section 4.1 does not pin and which builds against
//!   libpipewire, or a client of the `PulseAudio` protocol that `pipewire-pulse` serves.
//!
//! # The registration
//!
//! The thread's id and nice level are read on the caller's thread. Its one bridge call starts a
//! promoter task on the bridge's thread and waits for its answer within the settings' deadline;
//! past it, the bridge cancels only that wait, never the task, since a promotion cut short could
//! leave a thread real-time with no ticket to revert it. Once promoted, the task answers with a
//! channel and waits. When the ticket drops, or at once when no ask can come (the call had given
//! up, its answer arrived too late, or the ticket's ask was dropped unsent), the task reverts the
//! thread with [`RealtimeKit::revert`], to the nice level read before, floored at `MinNiceLevel`
//! ([`Promoted::restore`]), unless the promotion changed nothing ([`Promotion::reverts`]). A
//! request whose system-bus connection failed is reverted over the bridge's next one: the
//! promoter waits for zbus to mark the failed connection closed, which the socket's reader does
//! only once it has failed the calls pending on it and handed the error to the connection's
//! streams, so that `Buses::system` connects again. On the bridge's one thread the promoter runs
//! between the two when a stream on the connection is full: the socket's reader then waits for
//! that stream's consumer.
//!
//! Dropping the ticket asks the task to revert and waits at most [`REVERT_WAIT`] for it; past
//! that, the task finishes on its own. The ticket keeps a handle to the bridge, so a registration
//! outlives the [`LinuxAudioEndpoints`] that made it, and the task keeps one until it has
//! reverted, so no drop of the bridge's last other handle (the endpoints before a late promotion
//! answers, or a ticket whose wait ended first) ends the bridge's thread under it. The task's
//! handle, when it is the last, drops on the bridge's own thread, which then ends on its own. A
//! ticket is not `Send`, and registering on the bridge's own thread is refused, so no ticket is
//! ever dropped there.

use std::time::Duration;

use eileen_dbus_linux::{Bridge, Buses, DEFAULT_DEADLINE, METHOD_TIMEOUT};
use eileen_platform::{
    AudioEndpoints, Deadline, EndpointInfo, PlatformError, StreamCategory, ThreadTicket,
};
use tokio::sync::oneshot;

use crate::{DbusError, Promoted, Promotion, RealtimeKit, RealtimePolicy, ThreadId};

/// How long dropping a ticket waits for the revert, at most. rtkit answers in milliseconds, and
/// a drop should not stall the audio thread for a call's whole deadline; past it, the promoter
/// task reverts on its own, since it keeps the bridge until it has.
pub const REVERT_WAIT: Duration = Duration::from_secs(1);

/// What the calls' errors name.
const REGISTER: &str = "audio thread registration";
const REVERT: &str = "audio thread revert";
const CAPTURE: &str = "default capture endpoint";
const LOOPBACK: &str = "default render loopback";
const DUCKING: &str = "communications ducking";

/// The detail of a registration whose promoter task ended without answering: it panicked, or
/// the bridge stopped.
const PROMOTER_GONE: &str = "the promoter ended without answering";

/// What the promoter task answers once it has reverted: whether rtkit agreed.
type Reverted = oneshot::Sender<Result<(), DbusError>>;
/// The channel through which a ticket asks its promoter task to revert.
type Revert = oneshot::Sender<Reverted>;
/// What the promoter task answers the registration.
type Answer = Result<(Promotion, Revert), PlatformError>;

/// What a [`LinuxAudioEndpoints`] needs besides its bridge, given by the composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxAudioSettings {
    /// How long a registration may take: `DEFAULT_DEADLINE`, as for any trait method that takes
    /// none, unless a caller, such as a test, sets a shorter one.
    pub deadline: Deadline,
    /// What a registration asks `RealtimeKit` for.
    pub policy: RealtimePolicy,
}

impl Default for LinuxAudioSettings {
    fn default() -> Self {
        Self { deadline: DEFAULT_DEADLINE, policy: RealtimePolicy::default() }
    }
}

/// `AudioEndpoints` on Linux, over a [`Bridge`]; the module's doc says what each method does.
#[derive(Debug)]
pub struct LinuxAudioEndpoints {
    bridge: Bridge,
    settings: LinuxAudioSettings,
}

impl LinuxAudioEndpoints {
    /// Endpoints on `bridge`, a clone of the process's one bridge, as `settings` say. It calls
    /// nothing yet.
    #[must_use]
    pub const fn new(bridge: Bridge, settings: LinuxAudioSettings) -> Self {
        Self { bridge, settings }
    }
}

impl AudioEndpoints for LinuxAudioEndpoints {
    /// `Unavailable`: the default source is `PipeWire`'s, whose graph is not on D-Bus. Phase L2's
    /// audio-server client brings it, with the crate the owner chooses for it.
    fn default_capture(&self, _category: StreamCategory) -> Result<EndpointInfo, PlatformError> {
        Err(PlatformError::Unavailable { what: CAPTURE })
    }

    /// `Unavailable`: the far-end reference is the default sink's monitor, in `PipeWire`'s graph,
    /// which is not on D-Bus. Phase L2's audio-server client brings it.
    fn default_render_loopback(&self) -> Result<EndpointInfo, PlatformError> {
        Err(PlatformError::Unavailable { what: LOOPBACK })
    }

    /// Opting out needs nothing: `WirePlumber` ducks other streams only for a stream that
    /// declares a `media.role`, and Eileen's declare none [inferred for `WirePlumber` 0.5, on
    /// Ubuntu 26.04]. Ubuntu 24.04's `WirePlumber` 0.4.17 ships its role-based policy commented
    /// out, so it ducks nothing at all. Opting back in is `Unavailable`: there is no ducking of
    /// the OS's to opt into.
    fn set_communications_ducking(&self, opt_out: bool) -> Result<(), PlatformError> {
        if opt_out { Ok(()) } else { Err(PlatformError::Unavailable { what: DUCKING }) }
    }

    /// [`RealtimeKit::promote`] for the calling thread, reverted when the ticket drops; the
    /// module's doc says how. The ticket's task says which the thread got.
    fn register_audio_thread(&self) -> Result<ThreadTicket, PlatformError> {
        let thread = ThreadId::current();
        let nice = thread.nice()?;
        let policy = self.settings.policy;
        let bridge = self.bridge.clone();
        let job = move |buses| async move {
            let (answer, answered) = oneshot::channel();
            drop(tokio::task::spawn_local(promoter(bridge, buses, thread, nice, policy, answer)));
            // Cancelled, the job drops `answered`, which makes the promoter revert what it got.
            answered.await.unwrap_or_else(|_gone| {
                Err(PlatformError::Io { what: REGISTER, detail: PROMOTER_GONE.to_owned() })
            })
        };
        let (promotion, revert) = self.bridge.call(REGISTER, self.settings.deadline, job)?;
        Ok(reverted_on_drop(self.bridge.clone(), promotion, revert))
    }
}

/// The promoter task: promotes `thread`, whose nice level is `nice`, as `policy` asks, hands the
/// promotion over ([`hand_over`]), then reverts `thread`, unless nothing changed, and answers the
/// ask, if one came. It keeps `bridge` until it has reverted, so the bridge's thread, which runs
/// it, lives as long.
async fn promoter(
    bridge: Bridge,
    buses: Buses,
    thread: ThreadId,
    nice: i32,
    policy: RealtimePolicy,
    answer: oneshot::Sender<Answer>,
) {
    let promoted = match buses.system().await {
        Ok(system) => {
            let fresh = || reconnect(&buses, &system);
            RealtimeKit::new(&system).promote(thread, nice, policy, fresh).await
        }
        Err(error) => Err(error),
    };
    let Promoted { promotion, restore } = match promoted {
        Ok(promoted) => promoted,
        Err(error) => {
            let _ignored = answer.send(Err(error.into()));
            return;
        }
    };
    let asked = hand_over(answer, promotion).await;
    // The system bus of the moment: the bridge connects it again if it has closed since.
    let outcome = if promotion.reverts() {
        match buses.system().await {
            Ok(system) => RealtimeKit::new(&system).revert(thread, restore).await,
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    // Before the outcome goes out, so a ticket still waiting for it holds the last handle, if
    // this was the other one, and its drop ends the bridge's thread and waits for it.
    drop(bridge);
    if let Some(reverted) = asked {
        let _ignored = reverted.send(outcome);
    }
}

/// The system bus again, for the revert of a request whose connection, `failed`, failed. It waits
/// for zbus to mark `failed` closed, at most [`METHOD_TIMEOUT`] (an error from a write alone may
/// leave it open, and then usable), so that [`Buses::system`] makes a new connection rather
/// than answer the failed one, which the socket's reader may not have marked yet.
async fn reconnect(
    buses: &Buses,
    failed: &zbus::Connection,
) -> Result<zbus::Connection, DbusError> {
    let _closed_or_open = tokio::time::timeout(METHOD_TIMEOUT, failed.closed()).await;
    buses.system().await
}

/// Answers the registration with `promotion` and a channel to ask for the revert through, then
/// waits for that ask. Returns the channel the ask brought, to answer it on, or `None` when no
/// ask will come: the registration did not take its answer (its wait had ended), or the ask's
/// channel dropped unsent (the answer reached the bridge's reply as the wait ended, or the bridge
/// refused or cancelled the ticket's call). The promoter reverts next in every case.
async fn hand_over(answer: oneshot::Sender<Answer>, promotion: Promotion) -> Option<Reverted> {
    let (revert, asked) = oneshot::channel();
    if answer.send(Ok((promotion, revert))).is_err() {
        return None;
    }
    asked.await.ok()
}

/// The ticket of a promotion. Dropped, it asks the promoter task to revert through `revert` and
/// waits at most [`REVERT_WAIT`] for it; it keeps `bridge` until then.
fn reverted_on_drop(bridge: Bridge, promotion: Promotion, revert: Revert) -> ThreadTicket {
    ThreadTicket::new(promotion.task(), move || {
        // Nothing to tell the dropping thread: past the wait, the task reverts without it.
        let _ignored = bridge.call(REVERT, Deadline::after(REVERT_WAIT), |_buses| async move {
            let (reverted, outcome) = oneshot::channel();
            let _ignored = revert.send(reverted);
            let _ignored = outcome.await;
            Ok(())
        });
    })
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::{Pin, pin};
    use std::task::{Context, Poll, Waker};

    use eileen_dbus_linux::testing::PrivateBus;
    use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};
    use tokio::sync::oneshot;

    use super::{hand_over, reconnect};
    use crate::Promotion;

    const PROMOTION: Promotion = Promotion::Realtime { priority: 10 };

    /// `future` polled once with a waker that does nothing: the channels need no runtime, and
    /// each step below is ready or not when it is polled.
    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn an_answer_nobody_takes_means_revert_now() {
        let (answer, answered) = oneshot::channel();
        drop(answered);
        let mut handing = pin!(hand_over(answer, PROMOTION));
        assert!(matches!(poll_once(handing.as_mut()), Poll::Ready(None)), "no ask will come");
    }

    #[test]
    fn an_ask_dropped_unsent_means_revert_now() {
        let (answer, mut answered) = oneshot::channel();
        let mut handing = pin!(hand_over(answer, PROMOTION));
        assert!(poll_once(handing.as_mut()).is_pending(), "answered, and waiting for the ask");
        let (promotion, revert) = answered.try_recv().unwrap().unwrap();
        assert_eq!(promotion, PROMOTION, "the promotion");
        drop(revert);
        assert!(matches!(poll_once(handing.as_mut()), Poll::Ready(None)), "no ask will come");
    }

    #[test]
    fn an_ask_is_answered_on_the_channel_it_brings() {
        let (answer, mut answered) = oneshot::channel();
        let mut handing = pin!(hand_over(answer, PROMOTION));
        assert!(poll_once(handing.as_mut()).is_pending(), "answered, and waiting for the ask");
        let (_promotion, revert) = answered.try_recv().unwrap().unwrap();
        let (reverted, mut outcome) = oneshot::channel();
        revert.send(reverted).unwrap();
        let Poll::Ready(Some(reverted)) = poll_once(handing.as_mut()) else {
            panic!("the ask's channel");
        };
        reverted.send(Ok(())).unwrap();
        assert_eq!(outcome.try_recv(), Ok(Ok(())), "the ticket hears how the revert went");
    }

    #[test]
    fn a_reconnect_waits_for_zbus_to_mark_the_failed_connection_closed() {
        let bus = PrivateBus::start();
        let bridge = Bridge::on_private_bus(&bus).unwrap();
        let seen = bridge.call("reconnecting", DEFAULT_DEADLINE, move |buses| async move {
            let failed = buses.system().await.unwrap();
            // The bus goes, which fails the connection from the far side. Its socket's reader
            // runs on this thread, which the job holds until it awaits, so zbus has not marked
            // the connection closed yet, as when the promoter runs before the reader is done.
            drop(bus);
            let marked_before = failed.is_closed();
            let again = reconnect(&buses, &failed).await;
            Ok((marked_before, failed.is_closed(), again.is_err()))
        });
        let (marked_before, marked_after, connected_again) = seen.unwrap();
        assert!(!marked_before, "the failure is not marked yet");
        assert!(marked_after, "reconnect waited for the mark");
        assert!(
            connected_again,
            "then connected anew, which fails with the bus gone, not answered the failed one"
        );
    }
}

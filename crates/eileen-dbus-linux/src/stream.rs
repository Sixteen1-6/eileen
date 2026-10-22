//! The conventions every signal stream in the Linux crates follows, written once here, with the
//! two helpers that keep them: [`subscribe`] opens a stream for a match rule, and [`drain`] takes
//! what a stream has ready.
//!
//! - **Queue.** [`subscribe`] gives a stream made for a match rule [`QUEUE_SIZE`] as its
//!   `max_queued`, so a burst (a player's every property, a device list) fits while the owner of
//!   the stream is busy.
//! - **Drain cap.** A stream that takes several signals per poll takes them through [`drain`], at
//!   most [`DRAIN_CAP`], which then wakes the task and yields, so a flood cannot starve the
//!   task's other work.
//! - **Errors.** A read error is passed over, and the stream's end follows it: zbus hands a
//!   read error to every stream of the connection and stops reading. [`Signals`], [`next`] and
//!   [`OwnerChanges`](crate::OwnerChanges) do this, so what [`drain`] takes is never an error.
//! - **Names.** The method that opens a client's stream of changes is named for the trait method
//!   it serves, where there is one (`MprisSessions::subscribe`, for `MediaSessions::subscribe`,
//!   opens an `MprisWatch`). Else it is `changes()`, or `<what>_changes()` for a free function,
//!   and the stream's type ends in `Changes`: `LinuxPower::changes` gives `PowerChanges`, and
//!   [`owner_changes`](crate::owner_changes) gives `OwnerChanges`. A stream of events rather
//!   than a state's changes is named for them: `Notifier::events` gives `NotificationEvents`.
//! - **The trait.** [`Stream`] is re-exported from here, the one place that names zbus's path to
//!   it (`zbus::export::futures_core`, which zbus calls macro support rather than public API).
//!   A zbus release that moves it fails to compile in this module alone; a direct `futures-core`
//!   dependency, already in the lockfile through zbus, is then the fix.
//!
//! # Traps
//!
//! These hold for every zbus stream, the signal streams of proxies included.
//!
//! - **An unpolled stream stalls its whole connection.** zbus queues at most a stream's
//!   `max_queued` messages ([`QUEUE_SIZE`] for the streams made here, zbus's default of 64 for
//!   others), then stops reading the connection, so every call on it waits, the method timeout
//!   then failing each one. Poll every stream steadily, or drop it.
//! - **Drop a stream inside the tokio runtime's context** (a task, `block_on` or
//!   `Runtime::enter`): zbus spawns a task there to remove the stream's match rule.
//!   Dropped on a plain thread, as a `Subscription`'s release closure may be, a stream panics
//!   in zbus's destructor and leaves its rule on the bus; dropped while a panic unwinds, it
//!   aborts the process.
//! - **A signal sent before its stream exists never reaches it**: subscribe, then call.
//! - **zbus cannot compare a rule's well-known sender with a signal's unique one.** It checks a
//!   unique-name sender itself (the bus's own name counts as one), but a rule that names a
//!   service's well-known name lets through any sender's signal addressed to this connection.

use std::pin::Pin;
use std::task::{Context, Poll, ready};

use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::DbusError;

/// The `Stream` trait, which zbus's streams implement.
pub use zbus::export::futures_core::Stream;

/// How many signals a stream made for a match rule queues before its connection waits for it:
/// four times zbus's default of 64, which streams made without a size (proxies' signal streams)
/// still have.
pub const QUEUE_SIZE: usize = 256;

/// The most signals one poll of a stream takes from a subscription before it yields.
pub const DRAIN_CAP: usize = 64;

/// Subscribes to the messages `rule` matches on `conn` from now on, queueing at most
/// [`QUEUE_SIZE`] of them while the stream is not polled. `rule` comes as the match rule's
/// builder gives it, so a rule that could not be built fails as one the bus refuses would: with
/// the [`DbusError`] that `what` names. Subscribe before the call whose answer it waits for.
pub async fn subscribe(
    conn: &Connection,
    rule: zbus::Result<MatchRule<'_>>,
    what: &'static str,
) -> Result<Signals, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(what, &error);
    let messages = MessageStream::for_match_rule(rule.map_err(failed)?, conn, Some(QUEUE_SIZE))
        .await
        .map_err(failed)?;
    Ok(Signals { messages })
}

/// The messages a [`subscribe`] rule matches, in the order the connection received them; it
/// ends when the connection does. A read error is passed over, and the end follows it. The
/// crate's Traps say how to poll and drop it.
#[derive(Debug)]
#[must_use = "a stream does nothing unless polled"]
pub struct Signals {
    messages: MessageStream,
}

impl Signals {
    /// The next message; `None` once the connection has ended. Cancel-safe.
    pub async fn next(&mut self) -> Option<Message> {
        next(&mut self.messages).await
    }
}

impl Stream for Signals {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Message>> {
        poll_message(&mut self.messages, cx)
    }
}

/// Takes what `stream` has ready, handing each item to `take` until `take` gives a value: at
/// most [`DRAIN_CAP`] items, after which it wakes the task, to come back for the rest, and
/// yields.
///
/// It answers as a stream's `poll_next` does: `Ready(Some(value))` with the value `take` gave,
/// `Ready(None)` once the stream has ended, and `Pending` when nothing more is ready (the stream
/// wakes the task once something is) or the cap was reached. A `take` that never gives a value
/// takes everything ready, up to the cap, and its `drain` is ready only at the stream's end.
pub fn drain<S, T>(
    stream: &mut S,
    cx: &mut Context<'_>,
    mut take: impl FnMut(S::Item) -> Option<T>,
) -> Poll<Option<T>>
where
    S: Stream + Unpin,
{
    for _ in 0..DRAIN_CAP {
        let Some(item) = ready!(Pin::new(&mut *stream).poll_next(cx)) else {
            return Poll::Ready(None);
        };
        if let Some(value) = take(item) {
            return Poll::Ready(Some(value));
        }
    }
    // A run of items still to take: yield, and come back for the rest.
    cx.waker().wake_by_ref();
    Poll::Pending
}

/// The next message `stream` brings; `None` once its connection has ended. A read error is
/// passed over, and the end follows it. Cancel-safe.
pub async fn next(stream: &mut MessageStream) -> Option<Message> {
    std::future::poll_fn(|cx| poll_message(stream, cx)).await
}

/// Polls `stream` for its next message, passing over a read error, which its end follows.
fn poll_message(stream: &mut MessageStream, cx: &mut Context<'_>) -> Poll<Option<Message>> {
    loop {
        match ready!(Pin::new(&mut *stream).poll_next(cx)) {
            Some(Ok(message)) => return Poll::Ready(Some(message)),
            Some(Err(_)) => {}
            None => return Poll::Ready(None),
        }
    }
}

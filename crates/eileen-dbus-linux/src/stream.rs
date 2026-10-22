//! The conventions every signal stream in the Linux crates follows, written once here.
//!
//! - **Queue.** A stream made for a match rule takes [`QUEUE_SIZE`] as its `max_queued`, so a
//!   burst (a player's every property, a device list) fits while the owner of the stream is busy.
//! - **Drain cap.** A stream that takes several signals per poll takes at most [`DRAIN_CAP`],
//!   then wakes itself and yields, so a flood cannot starve the task's other work.
//! - **Errors.** A read error is passed over, and the stream's end follows it: zbus hands a
//!   read error to every stream of the connection and stops reading. [`next`] does the same.
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

/// The `Stream` trait, which zbus's streams implement.
pub use zbus::export::futures_core::Stream;

/// How many signals a stream made for a match rule queues before its connection waits for it:
/// four times zbus's default of 64, which streams made without a size (proxies' signal streams)
/// still have.
pub const QUEUE_SIZE: usize = 256;

/// The most signals one poll of a stream takes from a subscription before it yields.
pub const DRAIN_CAP: usize = 64;

/// The next message `stream` brings; `None` once its connection has ended. A read error is
/// passed over, and the end follows it. Cancel-safe.
pub async fn next(stream: &mut zbus::MessageStream) -> Option<zbus::Message> {
    loop {
        let item = std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await?;
        if let Ok(message) = item {
            return Some(message);
        }
    }
}

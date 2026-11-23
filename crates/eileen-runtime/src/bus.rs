//! The bus (BUILD.md sections 3.3.4, 4.7 and 6.4): the component that claims the `Emit` effects
//! and hands each event, in its envelope, to everyone subscribed.
//!
//! - **Fan-out.** [`bounded`] gives the [`Bus`], which the composition root starts as a
//!   component, and a [`BusHandle`], which the HUD client, the loopback endpoint, the store and
//!   the console view subscribe through. Each [`Subscriber`] gets every event published after
//!   it subscribed, in the order the core emitted them, with the effect's own `seq`, times and
//!   `TraceCtx`.
//! - **Bound.** The bus keeps the newest events, at least `capacity` of them (tokio rounds the
//!   number up to a power of two), and publishing never waits: a subscriber that reads slowly
//!   holds neither the driver nor any other subscriber.
//! - **Lag.** A subscriber that fell further behind than the bus keeps is told so, never left
//!   to find a gap: its next event is [`BusEvent::Lagged`] with the number of events it missed,
//!   followed by the oldest event still kept. The notice has no place of its own in the core's
//!   order, so it carries the `seq` and times of the event that follows it, where the
//!   subscriber's view resumes, and no turn or job.
//! - **End.** The bus publishes what is already queued for it, then stops at shutdown or when
//!   its effect queue closes. Each subscriber still gets what the bus kept for it, then `None`.

use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::task::Poll;

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{Effect, EffectFamily};
use eileen_contracts::ids::{Envelope, MonoTime, TraceCtx, WallTime};
use eileen_contracts::ports::{Component, ComponentCtx, ComponentError};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::{Receiver, Sender, WeakSender};

/// The name the bus runs under, and claims `Emit` with.
pub const NAME: &str = "bus";

/// How many events a running Eileen's bus keeps for a subscriber that has fallen behind.
pub const CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(1_024 - 1);

/// A bus keeping at least `capacity` events, and the handle to subscribe through.
#[must_use]
pub fn bounded(capacity: NonZeroUsize) -> (Bus, BusHandle) {
    let (sender, _first) = eileen_rt::channel::broadcast(capacity);
    let handle = BusHandle(sender.downgrade());
    (Bus { sender }, handle)
}

/// The bus as a component: it claims [`EffectFamily::Emit`] and publishes each event.
#[derive(Debug)]
pub struct Bus {
    sender: Sender<Envelope<BusEvent>>,
}

impl Bus {
    /// Hands one event to every subscriber, without waiting for any of them.
    pub(crate) fn publish(&self, effect: Envelope<Effect>) {
        let Envelope { seq, at, mono, ctx, body } = effect;
        let Effect::Emit(body) = body else {
            eileen_stdx::never!(true, "the bus was routed an effect outside the Emit family");
            return;
        };
        // An error says nobody is subscribed now, and then nobody waits for this event.
        let _subscribers = self.sender.send(Envelope { seq, at, mono, ctx, body });
    }
}

impl Component for Bus {
    fn name(&self) -> &'static str {
        NAME
    }

    fn accepts(&self) -> &'static [EffectFamily] {
        &[EffectFamily::Emit]
    }

    fn start(
        self: Box<Self>,
        ctx: ComponentCtx,
    ) -> Pin<Box<dyn Future<Output = Result<(), ComponentError>> + Send>> {
        Box::pin(async move {
            let ComponentCtx { mut effects, shutdown, .. } = ctx;
            let mut stop = shutdown.wait();
            loop {
                let mut next = effects.recv();
                // An effect already queued goes out before shutdown is looked at, so the
                // events of the last step reach the subscribers.
                let effect = poll_fn(|cx| {
                    if let Poll::Ready(effect) = next.as_mut().poll(cx) {
                        return Poll::Ready(effect);
                    }
                    stop.as_mut().poll(cx).map(|()| None)
                })
                .await;
                drop(next);
                let Some(effect) = effect else { return Ok(()) };
                self.publish(effect);
            }
        })
    }
}

/// Where the bus's readers subscribe. It does not keep the bus running: once the [`Bus`] has
/// stopped, a new subscriber is at its end from the start.
#[derive(Debug, Clone)]
pub struct BusHandle(WeakSender<Envelope<BusEvent>>);

impl BusHandle {
    /// A subscriber to every event published from now on.
    #[must_use]
    pub fn subscribe(&self) -> Subscriber {
        Subscriber {
            receiver: self.0.upgrade().map(|sender| sender.subscribe()),
            held: None,
            owed: 0,
            stamp: Stamp { seq: 0, at: WallTime(0), mono: MonoTime(0) },
        }
    }
}

/// The place in the core's order a lag notice is given.
#[derive(Debug, Clone, Copy)]
struct Stamp {
    seq: u64,
    at: WallTime,
    mono: MonoTime,
}

/// One reader of the bus.
#[derive(Debug)]
pub struct Subscriber {
    /// `None` once the bus has stopped and everything it kept has been taken.
    receiver: Option<Receiver<Envelope<BusEvent>>>,
    /// The event that follows a lag notice not yet given, or just given.
    held: Option<Envelope<BusEvent>>,
    /// The events missed and not yet reported.
    owed: u64,
    /// The stamp of the newest event taken off the bus.
    stamp: Stamp,
}

impl Subscriber {
    /// The next event; [`BusEvent::Lagged`] first when this subscriber missed some; `None`
    /// once the bus has stopped and nothing is left. Dropping the future loses nothing: a
    /// count of missed events is kept until it has been given.
    pub async fn recv(&mut self) -> Option<Envelope<BusEvent>> {
        loop {
            if self.owed > 0 && (self.held.is_some() || self.receiver.is_none()) {
                let Stamp { seq, at, mono } = self.stamp;
                let body = BusEvent::Lagged { missed: std::mem::take(&mut self.owed) };
                return Some(Envelope { seq, at, mono, ctx: TraceCtx::default(), body });
            }
            if let Some(event) = self.held.take() {
                return Some(event);
            }
            let receiver = self.receiver.as_mut()?;
            // Nothing awaits between here and the next turn of the loop, so each outcome is
            // either kept in `self` or was never taken off the bus.
            match receiver.recv().await {
                Ok(event) => {
                    self.stamp = Stamp { seq: event.seq, at: event.at, mono: event.mono };
                    self.held = Some(event);
                }
                Err(RecvError::Lagged(missed)) => self.owed = self.owed.saturating_add(missed),
                Err(RecvError::Closed) => self.receiver = None,
            }
        }
    }
}

#[cfg(test)]
mod tests;

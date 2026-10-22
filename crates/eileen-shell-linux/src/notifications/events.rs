//! The notification server's events: [`NotificationEvents`], from [`Notifier::events`], yields
//! the server's `NotificationClosed` and `ActionInvoked` signals, each id with the server that
//! sent it, and [`NotificationEvent::ServerChanged`] when the name changes owner (the bus's
//! `NameOwnerChanged`), in the order the connection received them. The [specification][spec]
//! defines the two signals. They serve no trait method yet: they tell when the user dismissed a
//! toast.
//!
//! [spec]: https://specifications.freedesktop.org/notification/latest/protocol.html#signals
//!
//! The rules:
//!
//! - The caller polls the stream; the client spawns nothing of its own. The stream subscribes
//!   and drains with `eileen-dbus-linux`'s `subscribe` and `drain`, so it keeps the conventions
//!   for signal streams: their queue size, at most
//!   [`DRAIN_CAP`](eileen_dbus_linux::DRAIN_CAP) messages passed over per poll, and a read error
//!   passed over before the end.
//! - Only the name's owner speaks for the server. The stream knows the owner from the moment it
//!   subscribes and follows each owner change in the order the connection received it, so a
//!   signal from any other sender is passed over, though the bus delivers one to the connection
//!   when another subscription on it asks for any sender. A signal whose arguments are not the
//!   specification's, or whose id is zero, is passed over too, and so are the interface's other
//!   signals, such as `ActivationToken`.
//! - An action key is another program's text, so it is `Untrusted`.
//!
//! Traps:
//!
//! - Every signal stream's traps hold (`eileen-dbus-linux`'s crate doc, Traps): poll the stream
//!   steadily or drop it, and drop it inside the tokio runtime's context.
//! - Many servers broadcast the signals for every app's notifications, so the stream carries
//!   ids this client never made: the caller matches the ids `notify` returned, server included.
//! - Once the name changes owner, no event names the previous server's notifications again: on
//!   [`NotificationEvent::ServerChanged`], stop waiting for the ids of the `previous` server.
//! - A signal sent before the stream exists never reaches it: make the stream before the
//!   notification whose end it waits for.

use std::pin::Pin;
use std::task::{Context, Poll};

use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{
    DbusError, Names, OwnerChange, OwnerChanges, Signals, Stream, drain, name_owner, owner_changes,
    subscribe,
};
use zbus::MatchRule;
use zbus::match_rule::Builder;
use zbus::names::OwnedUniqueName;

use super::{INTERFACE, NAME, NotificationId, NotificationServer, Notifier, PATH};

/// The name errors give the subscription to the server's signals.
const SIGNALS: &str = "org.freedesktop.Notifications signals";

/// Why a notification closed, as `NotificationClosed` numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationCloseReason {
    /// It expired (1).
    Expired,
    /// The user dismissed it (2).
    Dismissed,
    /// A `CloseNotification` call closed it (3).
    Closed,
    /// The specification's undefined or reserved reason (4), or a number it does not define.
    Undefined,
}

impl NotificationCloseReason {
    /// The reason `code` names.
    const fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Expired,
            2 => Self::Dismissed,
            3 => Self::Closed,
            _ => Self::Undefined,
        }
    }
}

/// What the server says happened to a notification, or that the server changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationEvent {
    /// It closed (`NotificationClosed`).
    Closed {
        /// The notification.
        id: NotificationId,
        /// Why it closed.
        reason: NotificationCloseReason,
    },
    /// The user invoked one of its actions (`ActionInvoked`). Eileen's notifications list no
    /// actions.
    Action {
        /// The notification.
        id: NotificationId,
        /// The action's key, which the notification's sender chose; `default` is a click on the
        /// notification itself.
        key: Untrusted<String>,
    },
    /// The server's name changed owner (`NameOwnerChanged`): a server stopped, started, or took
    /// the name over. No later event names a notification of the `previous` server, and one
    /// that stopped took its notifications with it.
    ServerChanged {
        /// The server that owned the name; `None` when none did.
        previous: Option<NotificationServer>,
        /// The server that owns it now; `None` until one starts.
        current: Option<NotificationServer>,
    },
}

impl Notifier {
    /// The server's events from now on. It succeeds with no server running: the stream
    /// follows the name to whichever program owns it, now or later, and says when that
    /// changes. Call it, and drop the stream, in the tokio runtime's context, as in a task.
    pub async fn events(&self) -> Result<NotificationEvents, DbusError> {
        // The owner changes first, then the signals, then the owner itself: no change after the
        // first subscription goes unseen, and no signal is judged by the wrong owner.
        let mut owners = owner_changes(&self.connection, Names::Name(NAME)).await?;
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(NAME)
            .and_then(|rule| rule.path(PATH))
            .and_then(|rule| rule.interface(INTERFACE))
            .map(Builder::build);
        let signals = subscribe(&self.connection, rule, SIGNALS).await?;
        let answer = name_owner(&self.connection, NAME).await?;
        let (owner, change) = start(answer, &mut owners).await;
        let incoming = Incoming { signals, owners, signal: Held::Nothing, change };
        Ok(NotificationEvents { incoming, owner: owner.map(NotificationServer) })
    }
}

/// Where the stream starts: the owner its subscriptions began with, and the owner change it
/// holds. `answer` is the name's owner as the bus gave it once both subscriptions began, and
/// `owners` the owner changes, polled once and never waited on. A change the bus made before it
/// answered came in ahead of the answer, and zbus hands each message to its streams before it
/// reads the next. So a change ready then is the first since the subscriptions began, and its
/// previous owner is the one they began with, though the answer may already name the next one;
/// with none ready, the answer is.
async fn start<S>(
    answer: Option<OwnedUniqueName>,
    owners: &mut S,
) -> (Option<OwnedUniqueName>, Held<OwnerChange>)
where
    S: Stream<Item = OwnerChange> + Unpin,
{
    let ready = std::future::poll_fn(|cx| Poll::Ready(Pin::new(&mut *owners).poll_next(cx)));
    match ready.await {
        Poll::Ready(Some(change)) => (change.old.clone(), Held::Item(change)),
        Poll::Ready(None) => (answer, Held::End),
        Poll::Pending => (answer, Held::Nothing),
    }
}

/// The server's `NotificationClosed` and `ActionInvoked` signals and the bus's word that the
/// server changed, as a stream the caller drives with [`next`](Self::next) or as a [`Stream`];
/// nothing runs on its own. The `events` module's doc gives its rules and traps: only the name's
/// owner speaks for the server, other apps' notifications come too, and the stream must be
/// polled steadily and dropped inside the tokio runtime's context.
#[derive(Debug)]
#[must_use = "a stream does nothing unless polled"]
pub struct NotificationEvents {
    /// What the two subscriptions give, in the order the connection received it.
    incoming: Incoming,
    /// The name's owner where the stream has read to: only its signals count.
    owner: Option<NotificationServer>,
}

/// The two subscriptions behind [`NotificationEvents`], as one stream of their items.
#[derive(Debug)]
struct Incoming {
    /// The server's signals as the bus routes them; their sender is checked by the events.
    signals: Signals,
    /// The bus's owner changes for the name.
    owners: OwnerChanges,
    /// What `signals` gave that the stream has not passed on.
    signal: Held<zbus::Message>,
    /// What `owners` gave that the stream has not passed on.
    change: Held<OwnerChange>,
}

/// What one of the two subscriptions gave that the stream has not passed on.
#[derive(Debug, PartialEq, Eq)]
enum Held<T> {
    /// Nothing.
    Nothing,
    /// An item, which waits until the other subscription has shown nothing came before it.
    Item(T),
    /// The subscription's end.
    End,
}

/// An item one of the subscriptions gave.
#[derive(Debug)]
enum Received {
    /// One of the server's signals.
    Signal(zbus::Message),
    /// An owner change of the name.
    Change(OwnerChange),
}

impl NotificationEvents {
    /// The next event; `None` once the connection is gone. Cancel-safe.
    pub async fn next(&mut self) -> Option<NotificationEvent> {
        std::future::poll_fn(|cx| self.poll_event(cx)).await
    }

    /// Polls for the next event. It passes over at most
    /// [`DRAIN_CAP`](eileen_dbus_linux::DRAIN_CAP) messages that make none, then yields.
    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<NotificationEvent>> {
        let owner = &mut self.owner;
        drain(&mut self.incoming, cx, |received| match received {
            Received::Signal(signal) => Self::signal_event(owner.as_ref(), &signal),
            Received::Change(change) => Some(Self::change_event(owner, change)),
        })
    }
}

impl Incoming {
    /// The next item of the two subscriptions, in the order the connection received them.
    /// zbus hands each message it reads to every subscription before it reads the next, so a
    /// subscription that has nothing once the other's item is held has nothing earlier to come.
    fn poll_received(&mut self, cx: &mut Context<'_>) -> Poll<Option<Received>> {
        self.take_signal(cx);
        self.take_change(cx);
        if matches!((&self.signal, &self.change), (Held::Nothing, Held::Item(_))) {
            // The change may have come in after the signals were polled.
            self.take_signal(cx);
        }
        let signal = std::mem::replace(&mut self.signal, Held::Nothing);
        let change = std::mem::replace(&mut self.change, Held::Nothing);
        match (signal, change) {
            (Held::Item(signal), Held::Item(change)) => {
                if signal.recv_position() < change.received {
                    self.change = Held::Item(change);
                    Poll::Ready(Some(Received::Signal(signal)))
                } else {
                    self.signal = Held::Item(signal);
                    Poll::Ready(Some(Received::Change(change)))
                }
            }
            (Held::Item(signal), change) => {
                self.change = change;
                Poll::Ready(Some(Received::Signal(signal)))
            }
            (signal, Held::Item(change)) => {
                self.signal = signal;
                Poll::Ready(Some(Received::Change(change)))
            }
            (Held::End, Held::End) => {
                (self.signal, self.change) = (Held::End, Held::End);
                Poll::Ready(None)
            }
            (signal, change) => {
                (self.signal, self.change) = (signal, change);
                Poll::Pending
            }
        }
    }

    /// Takes the server's next signal, unless one is held or the subscription has ended.
    fn take_signal(&mut self, cx: &mut Context<'_>) {
        if matches!(self.signal, Held::Nothing) {
            self.signal = match Pin::new(&mut self.signals).poll_next(cx) {
                Poll::Ready(Some(signal)) => Held::Item(signal),
                Poll::Ready(None) => Held::End,
                Poll::Pending => Held::Nothing,
            };
        }
    }

    /// Takes the name's next owner change, unless one is held or the subscription has ended.
    fn take_change(&mut self, cx: &mut Context<'_>) {
        if matches!(self.change, Held::Nothing) {
            self.change = match Pin::new(&mut self.owners).poll_next(cx) {
                Poll::Ready(Some(change)) => Held::Item(change),
                Poll::Ready(None) => Held::End,
                Poll::Pending => Held::Nothing,
            };
        }
    }
}

impl NotificationEvents {
    /// The event `signal` reports: one the specification defines, with the arguments it
    /// defines, from the name's owner, `owner`.
    fn signal_event(
        owner: Option<&NotificationServer>,
        signal: &zbus::Message,
    ) -> Option<NotificationEvent> {
        let header = signal.header();
        let sender = header.sender()?;
        if owner.is_none_or(|owner| owner.as_str() != sender.as_str()) {
            return None;
        }
        let server = NotificationServer::of(sender);
        let body = signal.body();
        match header.member()?.as_str() {
            "NotificationClosed" => {
                let (number, reason) = body.deserialize::<(u32, u32)>().ok()?;
                let reason = NotificationCloseReason::from_code(reason);
                Some(NotificationEvent::Closed { id: NotificationId::new(server, number)?, reason })
            }
            "ActionInvoked" => {
                let (number, key) = body.deserialize::<(u32, String)>().ok()?;
                let key = Untrusted::new(key);
                Some(NotificationEvent::Action { id: NotificationId::new(server, number)?, key })
            }
            _ => None,
        }
    }

    /// The event an owner change reports; from it on, the new owner, now `owner`, speaks for
    /// the server.
    fn change_event(
        owner: &mut Option<NotificationServer>,
        change: OwnerChange,
    ) -> NotificationEvent {
        let current = change.new.map(NotificationServer);
        owner.clone_from(&current);
        NotificationEvent::ServerChanged { previous: change.old.map(NotificationServer), current }
    }
}

impl Stream for Incoming {
    type Item = Received;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Received>> {
        self.get_mut().poll_received(cx)
    }
}

impl Stream for NotificationEvents {
    type Item = NotificationEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<NotificationEvent>> {
        self.get_mut().poll_event(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::pin::{Pin, pin};
    use std::task::{Context, Poll, Waker};

    use eileen_dbus_linux::{OwnerChange, Stream};
    use zbus::message::Sequence;
    use zbus::names::{OwnedBusName, OwnedUniqueName};

    use super::{Held, NAME, NotificationCloseReason, start};

    /// The connection a unique bus name names, such as `:1.7`.
    fn server(unique_name: &str) -> OwnedUniqueName {
        OwnedUniqueName::try_from(unique_name).unwrap()
    }

    /// The bus's word that the server's name went from `old` to `new`.
    fn change(old: Option<&str>, new: Option<&str>) -> OwnerChange {
        OwnerChange {
            name: OwnedBusName::try_from(NAME).unwrap(),
            old: old.map(server),
            new: new.map(server),
            received: Sequence::default(),
        }
    }

    /// Owner changes that have the one result they hold ready at the first poll, and nothing
    /// after it.
    struct Owners(Poll<Option<OwnerChange>>);

    impl Stream for Owners {
        type Item = OwnerChange;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<OwnerChange>> {
            std::mem::replace(&mut self.0, Poll::Pending)
        }
    }

    /// Where the stream starts for the lookup's `answer` when the owner changes have `ready`
    /// ready. One poll finishes `start`, since it never waits for an owner change.
    fn start_with(
        answer: Option<&str>,
        ready: Poll<Option<OwnerChange>>,
    ) -> (Option<OwnedUniqueName>, Held<OwnerChange>) {
        let mut owners = Owners(ready);
        let mut cx = Context::from_waker(Waker::noop());
        match pin!(start(answer.map(server), &mut owners)).poll(&mut cx) {
            Poll::Ready(started) => started,
            Poll::Pending => panic!("start waited for an owner change"),
        }
    }

    #[test]
    fn with_no_change_ready_the_stream_starts_from_the_answer() {
        assert_eq!(
            start_with(Some(":1.7"), Poll::Pending),
            (Some(server(":1.7")), Held::Nothing),
            "no change came in by the answer, so its owner is the one the subscriptions began with"
        );
        assert_eq!(start_with(None, Poll::Pending), (None, Held::Nothing), "no server, none came");
        assert_eq!(
            start_with(Some(":1.7"), Poll::Ready(None)),
            (Some(server(":1.7")), Held::End),
            "the connection ended: the end is held for the stream to report"
        );
    }

    #[test]
    fn a_change_ready_at_the_answer_starts_the_stream_from_its_previous_owner() {
        let takeover = change(Some(":1.7"), Some(":1.9"));
        assert_eq!(
            start_with(Some(":1.7"), Poll::Ready(Some(takeover.clone()))),
            (Some(server(":1.7")), Held::Item(takeover.clone())),
            "a takeover that came in after the answer: the answer's owner, and the change held"
        );
        assert_eq!(
            start_with(Some(":1.9"), Poll::Ready(Some(takeover.clone()))),
            (Some(server(":1.7")), Held::Item(takeover)),
            "one that came in before the answer, which already names the new owner: the signals \
             before the change are still the previous owner's"
        );
        let started = change(None, Some(":1.9"));
        assert_eq!(
            start_with(Some(":1.9"), Poll::Ready(Some(started.clone()))),
            (None, Held::Item(started)),
            "a server that started before the answer: nothing spoke for the server before it"
        );
    }

    #[test]
    fn close_reasons_follow_the_specification() {
        let reasons: Vec<NotificationCloseReason> =
            (0..=5).map(NotificationCloseReason::from_code).collect();
        assert_eq!(
            reasons,
            [
                NotificationCloseReason::Undefined,
                NotificationCloseReason::Expired,
                NotificationCloseReason::Dismissed,
                NotificationCloseReason::Closed,
                NotificationCloseReason::Undefined,
                NotificationCloseReason::Undefined,
            ],
            "1 to 3 are named, 4 is undefined, and 0 or 5 are no reason the specification gives"
        );
    }
}

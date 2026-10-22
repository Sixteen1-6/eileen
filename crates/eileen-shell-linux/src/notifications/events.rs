//! The notification server's events: [`NotificationEvents`], from [`Notifier::events`], yields
//! the server's `NotificationClosed` and `ActionInvoked` signals, each id with the server that
//! sent it, and [`NotificationEvent::ServerChanged`] when the name changes owner (the bus's
//! `NameOwnerChanged`), in the order the connection received them. The [specification][spec]
//! defines the two signals. They serve no trait method yet: they tell when the user dismissed a
//! toast.
//!
//! [spec]: https://specifications.freedesktop.org/notification-spec/latest/
//!
//! The rules:
//!
//! - The caller polls the stream; the client spawns nothing of its own. The stream keeps
//!   `eileen-dbus-linux`'s conventions for signal streams: its queue size, at most
//!   [`DRAIN_CAP`] messages passed over per poll, and a read error passed over before the end.
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
use std::task::{Context, Poll, ready};

use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::{
    DRAIN_CAP, DbusError, Names, OwnerChange, OwnerChanges, QUEUE_SIZE, Stream, name_owner,
    owner_changes,
};
use zbus::{MatchRule, MessageStream};

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
        let failed = |error: zbus::Error| DbusError::from_zbus(SIGNALS, &error);
        // The owner changes first, then the signals, then the owner itself: no change after the
        // first subscription goes unseen, and no signal is judged by the wrong owner.
        let mut owners = owner_changes(&self.connection, Names::Name(NAME)).await?;
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(NAME)
            .and_then(|rule| rule.path(PATH))
            .and_then(|rule| rule.interface(INTERFACE))
            .map_err(failed)?
            .build();
        let signals = MessageStream::for_match_rule(rule, &self.connection, Some(QUEUE_SIZE))
            .await
            .map_err(failed)?;
        let owner = name_owner(&self.connection, NAME).await?;
        // A change the bus made before it answered came in ahead of the answer, and zbus hands
        // each message to its streams before it reads the next. So a change ready now is the
        // first since the subscriptions began, and its previous owner is the one they began with;
        // with none ready, the answer is.
        let ready = std::future::poll_fn(|cx| Poll::Ready(Pin::new(&mut owners).poll_next(cx)));
        let (owner, change) = match ready.await {
            Poll::Ready(Some(change)) => (change.old.clone(), Held::Item(change)),
            Poll::Ready(None) => (owner, Held::End),
            Poll::Pending => (owner, Held::Nothing),
        };
        Ok(NotificationEvents {
            signals,
            owners,
            owner: owner.map(NotificationServer),
            signal: Held::Nothing,
            change,
        })
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
    /// The server's signals as the bus routes them; their sender is checked here.
    signals: MessageStream,
    /// The bus's owner changes for the name.
    owners: OwnerChanges,
    /// The name's owner where the stream has read to: only its signals count.
    owner: Option<NotificationServer>,
    /// What `signals` gave that the stream has not passed on.
    signal: Held<zbus::Message>,
    /// What `owners` gave that the stream has not passed on.
    change: Held<OwnerChange>,
}

/// What one of the two subscriptions gave that the stream has not passed on.
#[derive(Debug)]
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

    /// Polls for the next event. It passes over at most [`DRAIN_CAP`] messages that make none,
    /// then yields.
    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<NotificationEvent>> {
        for _ in 0..DRAIN_CAP {
            let event = match ready!(self.poll_received(cx)) {
                Some(Received::Signal(signal)) => self.signal_event(&signal),
                Some(Received::Change(change)) => Some(self.change_event(change)),
                None => return Poll::Ready(None),
            };
            if event.is_some() {
                return Poll::Ready(event);
            }
        }
        // A run of messages to pass over: yield, and come back for the rest.
        cx.waker().wake_by_ref();
        Poll::Pending
    }

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

    /// Takes the server's next signal, unless one is held or the subscription has ended. A read
    /// error is passed over: the end follows it.
    fn take_signal(&mut self, cx: &mut Context<'_>) {
        while matches!(self.signal, Held::Nothing) {
            match Pin::new(&mut self.signals).poll_next(cx) {
                Poll::Ready(Some(Ok(signal))) => self.signal = Held::Item(signal),
                Poll::Ready(Some(Err(_))) => {}
                Poll::Ready(None) => self.signal = Held::End,
                Poll::Pending => return,
            }
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

    /// The event `signal` reports: one the specification defines, with the arguments it
    /// defines, from the name's owner.
    fn signal_event(&self, signal: &zbus::Message) -> Option<NotificationEvent> {
        let header = signal.header();
        let sender = header.sender()?;
        if self.owner.as_ref().is_none_or(|owner| owner.as_str() != sender.as_str()) {
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

    /// The event an owner change reports; from it on, the new owner speaks for the server.
    fn change_event(&mut self, change: OwnerChange) -> NotificationEvent {
        let current = change.new.map(NotificationServer);
        self.owner.clone_from(&current);
        NotificationEvent::ServerChanged { previous: change.old.map(NotificationServer), current }
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
    use super::NotificationCloseReason;

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

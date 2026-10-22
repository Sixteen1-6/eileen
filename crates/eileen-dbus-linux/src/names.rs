//! Who owns a bus name: [`name_owner`] asks the bus now (`GetNameOwner`), and [`owner_changes`]
//! follows the bus's `NameOwnerChanged` for one name or a namespace of names
//! ([the specification's bus messages](https://dbus.freedesktop.org/doc/dbus-specification.html#bus-messages-name-owner-changed)).
//!
//! - Owners are unique names, such as `:1.42`, checked as such: zbus's `UniqueName` also takes
//!   `org.freedesktop.DBus`, so a reply or signal that names anything else is not an owner.
//! - The subscription names the bus as the sender, its object and its interface, and zbus checks
//!   the sender of every message against it too, so a `NameOwnerChanged` that another
//!   connection sends straight to this one is dropped.
//! - Subscribe before asking: a change between [`name_owner`]'s answer and the subscription
//!   would otherwise go unseen.

use std::pin::Pin;
use std::task::{Context, Poll};

use zbus::match_rule::Builder;
use zbus::message::Sequence;
use zbus::names::{BusName, OwnedBusName, OwnedUniqueName};
use zbus::{Connection, MatchRule, Message};

use crate::DbusError;
use crate::stream::{Signals, Stream, drain, subscribe};

/// The bus's own name, which is also its interface's.
const BUS: &str = "org.freedesktop.DBus";
/// The bus's object.
const BUS_PATH: &str = "/org/freedesktop/DBus";
/// The bus's answer to `GetNameOwner` for a name nothing owns.
const NAME_HAS_NO_OWNER: &str = "org.freedesktop.DBus.Error.NameHasNoOwner";
/// What [`name_owner`]'s errors name.
const LOOKUP: &str = "name owner";
/// What [`owner_changes`]' errors name.
const SUBSCRIPTION: &str = "owner-change subscription";

/// The unique name of the connection that owns `name` now, or `None` while nothing does. `name`
/// is a well-known or unique bus name; any other text is [`DbusError::Invalid`] before the bus is
/// asked. Asking never starts a service.
pub async fn name_owner(
    conn: &Connection,
    name: &str,
) -> Result<Option<OwnedUniqueName>, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(LOOKUP, &error);
    let name = BusName::try_from(name)
        .map_err(|_invalid| DbusError::Invalid { what: LOOKUP, detail: "not a bus name".into() })?;
    let reply = conn.call_method(Some(BUS), BUS_PATH, Some(BUS), "GetNameOwner", &name).await;
    let reply = match reply {
        Ok(reply) => reply,
        Err(zbus::Error::MethodError(error, _, _)) if error.as_str() == NAME_HAS_NO_OWNER => {
            return Ok(None);
        }
        Err(error) => return Err(failed(error)),
    };
    let owner: String = reply.body().deserialize().map_err(failed)?;
    unique(&owner).map(Some).ok_or_else(|| DbusError::Invalid {
        what: LOOKUP,
        detail: "the bus answered no unique name".to_owned(),
    })
}

/// Which names [`owner_changes`] follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Names<'a> {
    /// One bus name, such as `org.freedesktop.Notifications` (the rule's `arg0`).
    Name(&'a str),
    /// Every name in a namespace: the name itself and those that start with it and a `.`, such
    /// as `org.mpris.MediaPlayer2` for every player (the rule's `arg0namespace`).
    Namespace(&'a str),
}

/// One `NameOwnerChanged`: a name gained, lost or changed its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerChange {
    /// The name.
    pub name: OwnedBusName,
    /// Its owner before, or `None` when it had none.
    pub old: Option<OwnedUniqueName>,
    /// Its owner now, or `None` when it has none.
    pub new: Option<OwnedUniqueName>,
    /// Where the signal came in on the connection: comparable with the `recv_position` of
    /// every other message the same connection received, so a client that follows several
    /// streams can put an owner change in order with its service's signals.
    pub received: Sequence,
}

impl OwnerChange {
    /// The change `message` reports; `None` when it is not a well-formed `NameOwnerChanged`.
    #[must_use]
    pub fn of(message: &Message) -> Option<Self> {
        let header = message.header();
        if header.member().is_none_or(|member| member != "NameOwnerChanged") {
            return None;
        }
        let body = message.body();
        let (name, old, new) = body.deserialize::<(&str, &str, &str)>().ok()?;
        let owner = |text: &str| if text.is_empty() { Some(None) } else { unique(text).map(Some) };
        Some(Self {
            name: OwnedBusName::try_from(name).ok()?,
            old: owner(old)?,
            new: owner(new)?,
            received: message.recv_position(),
        })
    }
}

/// Subscribes to the owner changes of `names` from now on. Make it before [`name_owner`] is
/// asked, so that no change between the two goes unseen; the crate's Traps say how to poll and
/// drop it.
pub async fn owner_changes(conn: &Connection, names: Names<'_>) -> Result<OwnerChanges, DbusError> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(BUS)
        .and_then(|rule| rule.path(BUS_PATH))
        .and_then(|rule| rule.interface(BUS))
        .and_then(|rule| rule.member("NameOwnerChanged"))
        .and_then(|rule| match names {
            Names::Name(name) => rule.arg(0, name),
            Names::Namespace(namespace) => rule.arg0ns(namespace),
        })
        .map(Builder::build);
    Ok(OwnerChanges { messages: subscribe(conn, rule, SUBSCRIPTION).await? })
}

/// The owner changes [`owner_changes`] subscribed to, in the order the bus sent them; it ends
/// when the connection does. A read error or a malformed signal is passed over, and a read
/// error is followed by the end.
#[derive(Debug)]
pub struct OwnerChanges {
    messages: Signals,
}

impl OwnerChanges {
    /// The next owner change; `None` once the connection has ended. Cancel-safe.
    pub async fn next(&mut self) -> Option<OwnerChange> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

impl Stream for OwnerChanges {
    type Item = OwnerChange;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<OwnerChange>> {
        drain(&mut self.messages, cx, |message| OwnerChange::of(&message))
    }
}

/// `text` as a connection's unique name, such as `:1.42`; `None` for anything else.
fn unique(text: &str) -> Option<OwnedUniqueName> {
    if !text.starts_with(':') {
        return None;
    }
    OwnedUniqueName::try_from(text).ok()
}

#[cfg(test)]
mod tests {
    use super::{OwnerChange, unique};

    /// A `NameOwnerChanged` from the bus with these arguments.
    fn signal(args: (&str, &str, &str)) -> zbus::Message {
        zbus::Message::signal("/org/freedesktop/DBus", "org.freedesktop.DBus", "NameOwnerChanged")
            .unwrap()
            .build(&args)
            .unwrap()
    }

    #[test]
    fn only_a_unique_name_is_an_owner() {
        assert!(unique(":1.42").is_some(), "a unique name");
        assert!(unique("org.freedesktop.DBus").is_none(), "the bus's name, which zbus takes");
        assert!(unique("org.example.App").is_none(), "a well-known name");
        assert!(unique(":").is_none(), "not a valid name");
        assert!(unique("").is_none(), "empty");
    }

    #[test]
    fn an_owner_change_reads_its_three_names() {
        let change = OwnerChange::of(&signal(("org.example.App", "", ":1.7"))).unwrap();
        assert_eq!(change.name.as_str(), "org.example.App", "the name");
        assert_eq!(change.old, None, "no owner before");
        assert_eq!(change.new.as_ref().map(|name| name.as_str()), Some(":1.7"), "the new owner");
    }

    #[test]
    fn a_malformed_owner_change_is_none() {
        for args in [
            ("org.example.App", "", "org.example.Other"),
            ("org.example.App", "not a name", ""),
            ("", "", ":1.7"),
        ] {
            assert_eq!(OwnerChange::of(&signal(args)), None, "{args:?}");
        }
        let other =
            zbus::Message::signal("/org/freedesktop/DBus", "org.freedesktop.DBus", "NameLost")
                .unwrap()
                .build(&("org.example.App",))
                .unwrap();
        assert_eq!(OwnerChange::of(&other), None, "another signal");
    }
}

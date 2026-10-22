//! [`Request`]: the client side of an xdg-desktop-portal request
//! ([the Request documentation](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Request.html)).
//! A portal call such as `OpenURI` or `Inhibit` answers at once with a request handle, and its
//! outcome arrives later as that request's `Response` signal.
//!
//! The order every caller follows:
//!
//! 1. [`Request::new`] makes a `handle_token` and predicts the handle the portal will give it:
//!    `/org/freedesktop/portal/desktop/request/SENDER/TOKEN`, where `SENDER` is the connection's
//!    unique name without its `:` and with each `.` as `_`.
//! 2. [`Request::responses`] subscribes to `Response` on that handle before the call, as the
//!    documentation asks, so an answer that comes before the call's reply waits in the stream.
//! 3. The caller makes the portal call with [`Request::token`] as its `handle_token` option.
//! 4. [`Request::follow`] reads the reply: it follows the handle the portal names when that is
//!    another (portals before 0.9 ignore the token), and gives the reply's sender.
//! 5. [`Request::wait`] waits for the `Response` from that sender, within the caller's deadline:
//!    the answer is a signal, which no method timeout bounds. Past the deadline it closes the
//!    request, which dismisses a dialog the portal still shows.
//! 6. A request that holds until it is closed, as an inhibition does, ends with
//!    [`close_request`]: `Close`, answered within the connection's method timeout. It takes the
//!    handle alone, so the holder need not keep the [`Request`] and its signal stream.
//!
//! Only a `Response` from the connection that answered the call counts. The match rule names the
//! portal, which keeps other connections' broadcasts out on the bus, but the bus delivers a
//! signal addressed to this connection whatever its rules say, and zbus cannot compare the
//! rule's well-known name with a signal's unique sender: [`Request::wait`] checks each sender.
//!
//! The codes [`Request::wait`] gives are the documentation's: 0 success, 1 cancelled by the user,
//! 2 ended another way. What 2 means (a backend's failure, a target not opened) is the caller's.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use zbus::match_rule::Builder;
use zbus::message::Flags;
use zbus::names::{OwnedUniqueName, UniqueName};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, Message};

use crate::DbusError;
use crate::stream::{Signals, subscribe};

/// The portal's bus name.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// The interface of a portal request.
const REQUEST: &str = "org.freedesktop.portal.Request";
/// Where the portal keeps its requests: `<prefix>/<sender>/<token>`.
const REQUEST_PREFIX: &str = "/org/freedesktop/portal/desktop/request";
/// What errors name.
const WHAT: &str = "portal request";
/// What a passed deadline names.
const RESPONSE: &str = "portal response";

/// How long [`Request::close`] may spend sending `Close`: one write to the bus socket, which
/// takes microseconds unless the bus has stopped reading.
pub const CLOSE_GRACE: Duration = Duration::from_millis(100);

/// One portal request: its `handle_token`, its handle, and the subscription to its `Response`.
#[derive(Debug)]
pub struct Request {
    conn: Connection,
    token: String,
    path: OwnedObjectPath,
    responses: Option<Signals>,
}

impl Request {
    /// A new request from `conn`, with a fresh token and the handle the portal will give it.
    /// [`DbusError::Invalid`] when the connection has no unique name, as a peer-to-peer one
    /// has not.
    pub fn new(conn: &Connection) -> Result<Self, DbusError> {
        let unnamed = || DbusError::Invalid {
            what: WHAT,
            detail: "no unique name to make the request's handle from".to_owned(),
        };
        let sender = conn.unique_name().ok_or_else(unnamed)?;
        let token = token();
        let path = request_path(sender.as_str(), &token).ok_or_else(unnamed)?;
        Ok(Self { conn: conn.clone(), token, path, responses: None })
    }

    /// The `handle_token` to pass in the portal call's options: Eileen's prefix, a count that
    /// keeps it unique in the process, and a random part that keeps other clients from guessing
    /// it, as the Request documentation asks.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The request's handle: the predicted one, or the one [`Self::follow`] took from the reply.
    #[must_use]
    pub fn path(&self) -> &ObjectPath<'_> {
        &self.path
    }

    /// Subscribes to the request's `Response`. Call it before the portal call.
    pub async fn responses(&mut self) -> Result<(), DbusError> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(PORTAL)
            .and_then(|rule| rule.path(self.path.clone()))
            .and_then(|rule| rule.interface(REQUEST))
            .and_then(|rule| rule.member("Response"))
            .map(Builder::build);
        self.responses = Some(subscribe(&self.conn, rule, WHAT).await?);
        Ok(())
    }

    /// Reads the portal call's `reply`: the handle it names, which the request follows when it
    /// is not the predicted one, and the unique name of the connection that sent it, which
    /// [`Self::wait`] takes.
    pub async fn follow(&mut self, reply: &Message) -> Result<OwnedUniqueName, DbusError> {
        let header = reply.header();
        let sender = header.sender().map(|sender| sender.to_owned().into()).ok_or_else(|| {
            DbusError::Invalid { what: WHAT, detail: "the reply has no sender".to_owned() }
        })?;
        let handle: OwnedObjectPath =
            reply.body().deserialize().map_err(|error| DbusError::from_zbus(WHAT, &error))?;
        if handle != self.path {
            // A portal before 0.9 ignores the token and names a handle of its own.
            self.path = handle;
            self.responses().await?;
        }
        Ok(sender)
    }

    /// The code of the first `Response` that `portal`, the connection that answered the call,
    /// sends within `deadline`; a `Response` from anyone else is passed over. Past the deadline
    /// it closes the request and answers [`DbusError::DeadlineExceeded`]. Without
    /// [`Self::responses`] first, [`DbusError::Invalid`].
    pub async fn wait(
        &mut self,
        portal: &UniqueName<'_>,
        deadline: Duration,
    ) -> Result<u32, DbusError> {
        let Some(responses) = self.responses.as_mut() else {
            return Err(DbusError::Invalid {
                what: WHAT,
                detail: "waited without subscribing first".to_owned(),
            });
        };
        let Ok(answer) = tokio::time::timeout(deadline, response(responses, portal)).await else {
            self.close().await;
            return Err(DbusError::DeadlineExceeded { call: RESPONSE });
        };
        answer
    }

    /// Closes the request, best effort: `Close` is sent without waiting for an answer, within
    /// [`CLOSE_GRACE`]. The portal ends the request and dismisses any dialog it shows; a request
    /// that has already ended is not there to close. [`close_request`] waits for the answer.
    pub async fn close(&self) {
        let close = async {
            let message = Message::method_call(self.path.clone(), "Close")?
                .destination(PORTAL)?
                .interface(REQUEST)?
                .with_flags(Flags::NoReplyExpected)?
                .build(&())?;
            self.conn.send(&message).await
        };
        let _ignored = tokio::time::timeout(CLOSE_GRACE, close).await;
    }
}

/// Closes the portal request at `handle` with its `Close`, and waits for the portal's answer
/// within the connection's method timeout, as every method call does: for a request that holds
/// until it is closed, whose end the caller must know. Errors name `what`; a request the portal
/// no longer serves is [`DbusError::NotFound`].
pub async fn close_request(
    conn: &Connection,
    handle: &ObjectPath<'_>,
    what: &'static str,
) -> Result<(), DbusError> {
    conn.call_method(Some(PORTAL), handle, Some(REQUEST), "Close", &())
        .await
        .map(|_reply| ())
        .map_err(|error| DbusError::from_zbus(what, &error))
}

/// The code of the first `Response` in `responses` that `portal` sent.
async fn response(responses: &mut Signals, portal: &UniqueName<'_>) -> Result<u32, DbusError> {
    loop {
        let message = responses.next().await.ok_or_else(|| DbusError::Io {
            what: WHAT,
            detail: "the bus connection closed".to_owned(),
        })?;
        if message.header().sender() != Some(portal) {
            continue;
        }
        let (code, _results): (u32, HashMap<String, OwnedValue>) =
            message.body().deserialize().map_err(|error| DbusError::from_zbus(RESPONSE, &error))?;
        return Ok(code);
    }
}

/// A fresh `handle_token`.
fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("eileen_{count}_{:016x}", RandomState::new().hash_one(count))
}

/// The handle the portal gives the request `token` from the connection `sender`.
fn request_path(sender: &str, token: &str) -> Option<OwnedObjectPath> {
    let sender = sender.strip_prefix(':').unwrap_or(sender).replace('.', "_");
    OwnedObjectPath::try_from(format!("{REQUEST_PREFIX}/{sender}/{token}")).ok()
}

#[cfg(test)]
mod tests {
    use super::{request_path, token};

    #[test]
    fn the_handle_is_the_sender_and_the_token_under_the_portals_prefix() {
        assert_eq!(
            request_path(":1.42", "eileen_0_ab").unwrap().as_str(),
            "/org/freedesktop/portal/desktop/request/1_42/eileen_0_ab",
            "the unique name without its colon, each dot as an underscore"
        );
    }

    #[test]
    fn tokens_are_unique_and_valid_in_a_path() {
        let (first, second) = (token(), token());
        assert_ne!(first, second, "a count keeps them unique");
        assert!(first.starts_with("eileen_"), "Eileen's prefix: {first}");
        assert!(request_path(":1.7", &first).is_some(), "a valid path element: {first}");
    }
}

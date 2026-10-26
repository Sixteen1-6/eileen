//! A client of the Secret Service (`org.freedesktop.secrets` on the session bus, as GNOME
//! Keyring, `KWallet` and `KeePassXC` serve it), for [`crate::LinuxSecretStore`]
//! ([the specification]; docs/LINUX.md section 4). It is written on zbus with its default
//! features off, since the `secret-service` crate and the keyring store over it turn them on.
//!
//! - An item is found by its lookup attributes, `service` (the settings' service, `eileen`) and
//!   `username` (the secret's name), as keyring's Secret Service store writes them, so
//!   `secret-tool lookup service eileen username <name>` reads what Eileen stored, and a later
//!   keyring route reads the same items. `Service.SearchItems` looks in every collection.
//! - Secrets cross the bus in a `plain` session: no encryption, so no Diffie-Hellman, hash or
//!   cipher crate. The session bus is the user's own, and any process of the user can read an
//!   unlocked item by asking the service, so the encryption guards only against a monitor on
//!   the bus run by the same user, which could ask too. Each session is closed with
//!   `Session.Close` before the call answers.
//! - It never prompts. A locked match, or a change the service answers with a prompt, is
//!   [`DbusError::Refused`]: "locked", or "needs the user's confirmation", after the prompt is
//!   dismissed with `Prompt.Dismiss`. A prompt can wait on a password dialog for good, past any
//!   deadline `SecretStore`'s methods could keep, since they take none.
//! - Every call is a method call bounded by the connection's method timeout; no signal is waited
//!   for.
//!
//! # Traps
//!
//! - The value sits in clear in the bus daemon's buffers and in zbus's message buffers, which
//!   are not zeroized; only the `SecretString` that holds it here is.
//! - An item another tool stored with the same attributes, in another collection, may be the one
//!   [`SecretService::get`] reads: the service lists matches in no stated order.
//! - [`SecretService::set`] writes to the `default` alias's collection. Making one needs a
//!   prompt, so with none `set` answers [`DbusError::NotFound`].
//! - A call cut short by its deadline leaves its session open until the connection closes, as a
//!   session is bound to the connection that opened it.
//! - [`SecretService::delete`] deletes one match at a time: a later match that answers with a
//!   prompt leaves the earlier ones deleted.
//!
//! [the specification]: https://specifications.freedesktop.org/secret-service/latest/

use std::collections::HashMap;

use secrecy::{ExposeSecret, SecretString};
use zbus::Connection;
use zbus::export::serde::{Deserialize, Serialize};
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Type, Value};

use crate::DbusError;

/// The service, its object and the interfaces this client calls.
const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const SESSION_IFACE: &str = "org.freedesktop.Secret.Session";
const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";

/// The algorithm of the session, whose input, output and parameters are all empty.
const PLAIN: &str = "plain";
/// The alias of the collection new items go to.
const DEFAULT_ALIAS: &str = "default";
/// The object path the service answers for "none": no collection, or no prompt needed.
const NONE: &str = "/";
/// The content type of a stored value.
const CONTENT_TYPE: &str = "text/plain";
/// The item properties `CreateItem` sets.
const LABEL: &str = "org.freedesktop.Secret.Item.Label";
const ATTRIBUTES: &str = "org.freedesktop.Secret.Item.Attributes";
/// The lookup attributes, as keyring's Secret Service store names them.
const SERVICE_ATTRIBUTE: &str = "service";
const USERNAME_ATTRIBUTE: &str = "username";

/// What the errors name.
const SECRET_SERVICE: &str = "Secret Service";
const SECRET: &str = "secret";
const DEFAULT_COLLECTION: &str = "default secret collection";

/// Why a change is refused.
const LOCKED: &str = "locked";
const NEEDS_CONFIRMATION: &str = "needs the user's confirmation";

/// A secret as the service sends and takes it: its session, the algorithm's parameters (none
/// for `plain`), the value and its content type.
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

/// The Secret Service on a session connection, for the items of one service name.
#[derive(Debug)]
pub(crate) struct SecretService<'a> {
    conn: &'a Connection,
    service: &'a str,
}

impl<'a> SecretService<'a> {
    /// A client on `conn`, the session bus, for the items whose `service` attribute is `service`.
    pub(crate) const fn new(conn: &'a Connection, service: &'a str) -> Self {
        Self { conn, service }
    }

    /// The secret stored under `name`: the first unlocked match's, `None` when nothing matches,
    /// and [`DbusError::Refused`] "locked" when only locked items do.
    pub(crate) async fn get(&self, name: &str) -> Result<Option<SecretString>, DbusError> {
        let (unlocked, locked) = self.search(name).await?;
        let Some(item) = unlocked.first() else {
            return if locked.is_empty() { Ok(None) } else { Err(refused(LOCKED)) };
        };
        let session = self.open_session().await?;
        let secret = self.item_secret(item, &session).await;
        self.close_session(&session).await;
        let (_session, _parameters, value, _content_type) = secret?;
        let value = String::from_utf8(value).map_err(|_not_utf8| DbusError::Invalid {
            what: SECRET,
            detail: "the stored value is not UTF-8".to_owned(),
        })?;
        Ok(Some(SecretString::from(value)))
    }

    /// Stores `value` under `name` in the default collection, replacing the item there with the
    /// same attributes.
    pub(crate) async fn set(&self, name: &str, value: &SecretString) -> Result<(), DbusError> {
        let collection: OwnedObjectPath = self.service_call("ReadAlias", &(DEFAULT_ALIAS,)).await?;
        if collection.as_str() == NONE {
            return Err(DbusError::NotFound { what: DEFAULT_COLLECTION });
        }
        let session = self.open_session().await?;
        let created = self.create_item(&collection, &session, name, value).await;
        self.close_session(&session).await;
        let (_item, prompt) = created?;
        self.refuse_prompt(&prompt).await
    }

    /// Removes every item stored under `name`; with none, there is nothing to do. When any match
    /// is locked, it is [`DbusError::Refused`] "locked" before anything is deleted.
    pub(crate) async fn delete(&self, name: &str) -> Result<(), DbusError> {
        let (unlocked, locked) = self.search(name).await?;
        if !locked.is_empty() {
            return Err(refused(LOCKED));
        }
        for item in &unlocked {
            let prompt: OwnedObjectPath = self.call(item, ITEM_IFACE, "Delete", &()).await?;
            self.refuse_prompt(&prompt).await?;
        }
        Ok(())
    }

    /// The items stored under `name`, unlocked and locked, in every collection.
    async fn search(
        &self,
        name: &str,
    ) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), DbusError> {
        self.service_call("SearchItems", &(self.attributes(name),)).await
    }

    /// The lookup attributes of `name`'s item.
    fn attributes<'n>(&'n self, name: &'n str) -> HashMap<&'n str, &'n str> {
        HashMap::from([(SERVICE_ATTRIBUTE, self.service), (USERNAME_ATTRIBUTE, name)])
    }

    /// A new `plain` session.
    async fn open_session(&self) -> Result<OwnedObjectPath, DbusError> {
        let (_output, session): (OwnedValue, OwnedObjectPath) =
            self.service_call("OpenSession", &(PLAIN, Value::from(""))).await?;
        Ok(session)
    }

    /// Closes `session`. Best effort: the service closes it anyway once the connection does.
    async fn close_session(&self, session: &ObjectPath<'_>) {
        let _ignored: Result<(), DbusError> = self.call(session, SESSION_IFACE, "Close", &()).await;
    }

    /// `item`'s secret, sent in `session`.
    async fn item_secret(
        &self,
        item: &ObjectPath<'_>,
        session: &ObjectPath<'_>,
    ) -> Result<Secret, DbusError> {
        self.call(item, ITEM_IFACE, "GetSecret", &(session,)).await
    }

    /// Creates `name`'s item in `collection` with `value`, sent in `session`, replacing one with
    /// the same attributes: the new item and the prompt the service asks for, if any.
    async fn create_item(
        &self,
        collection: &ObjectPath<'_>,
        session: &ObjectPath<'_>,
        name: &str,
        value: &SecretString,
    ) -> Result<(OwnedObjectPath, OwnedObjectPath), DbusError> {
        let label = format!("Eileen: {name}");
        let properties = HashMap::from([
            (LABEL, Value::from(label.as_str())),
            (ATTRIBUTES, Value::from(self.attributes(name))),
        ]);
        let no_parameters: &[u8] = &[];
        let secret = (session, no_parameters, value.expose_secret().as_bytes(), CONTENT_TYPE);
        self.call(collection, COLLECTION_IFACE, "CreateItem", &(properties, secret, true)).await
    }

    /// `Ok` when `prompt` is none; otherwise dismisses it, unrun, and refuses the change.
    async fn refuse_prompt(&self, prompt: &ObjectPath<'_>) -> Result<(), DbusError> {
        if prompt.as_str() == NONE {
            return Ok(());
        }
        let _ignored: Result<(), DbusError> = self.call(prompt, PROMPT_IFACE, "Dismiss", &()).await;
        Err(refused(NEEDS_CONFIRMATION))
    }

    /// Calls `method` on the service's own object.
    async fn service_call<B, R>(&self, method: &str, body: &B) -> Result<R, DbusError>
    where
        B: Serialize + DynamicType,
        R: for<'d> Deserialize<'d> + Type,
    {
        let path = ObjectPath::from_static_str_unchecked(SERVICE_PATH);
        self.call(&path, SERVICE_IFACE, method, body).await
    }

    /// Calls `method` of `interface` at `path`, answering its reply's body. Its errors name the
    /// secret, as the client's own refusals do, except a service that is not there.
    async fn call<B, R>(
        &self,
        path: &ObjectPath<'_>,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<R, DbusError>
    where
        B: Serialize + DynamicType,
        R: for<'d> Deserialize<'d> + Type,
    {
        let failed = |error: zbus::Error| {
            let error = DbusError::from_zbus(SECRET, &error);
            if matches!(error, DbusError::Unavailable { .. }) {
                DbusError::Unavailable { what: SECRET_SERVICE }
            } else {
                error
            }
        };
        let reply = self
            .conn
            .call_method(Some(SERVICE), path, Some(interface), method, body)
            .await
            .map_err(failed)?;
        reply.body().deserialize().map_err(failed)
    }
}

/// A change the service refuses for `reason`.
const fn refused(reason: &'static str) -> DbusError {
    DbusError::Refused { what: SECRET, reason }
}

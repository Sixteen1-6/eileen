//! `SecretStore` on Linux: `LinuxSecretStore` through a bridge on the private bus, against a fake
//! Secret Service. The fake serves the service, a default collection, items, sessions and a
//! prompt at their paths, supports only the `plain` algorithm, and has modes for a locked
//! collection, changes that need a prompt and no default collection; a service that never
//! answers stands beside it. The test's thread calls the store as the core does, blocking; the
//! fakes run on a runtime of their own.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use eileen_dbus_linux::testing::PrivateBus;
use eileen_dbus_linux::{Bridge, DEFAULT_DEADLINE};
use eileen_platform::{Deadline, PlatformError, SecretStore, SecretString};
use eileen_shell_linux::{LinuxSecretStore, SecretSettings};
use secrecy::ExposeSecret;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// The service's name and objects.
const NAME: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const COLLECTION_PATH: &str = "/org/freedesktop/secrets/collection/login";
const PROMPT_PATH: &str = "/org/freedesktop/secrets/prompt/1";
/// How many items and sessions the fake serves paths for.
const SLOTS: u32 = 8;
/// The `service` attribute the store's settings give.
const SERVICE: &str = "eileen";
/// A secret value that must never show.
const VALUE: &str = "sk-test-value";

/// One stored item: its lookup attributes and value.
#[derive(Debug, Clone)]
struct Item {
    attributes: HashMap<String, String>,
    value: Vec<u8>,
}

/// What a `CreateItem` call carried, besides the value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Created {
    label: String,
    attributes: BTreeMap<String, String>,
    content_type: String,
    session_open: bool,
}

/// The fake's state, which the tests set up and read.
#[derive(Debug, Default)]
struct State {
    /// The collection is locked: its items are listed as locked and no change is allowed.
    locked: bool,
    /// Creating and deleting an item answer with the prompt.
    prompts: bool,
    /// `ReadAlias("default")` answers `/`.
    no_default: bool,
    /// The items, by path.
    items: BTreeMap<String, Item>,
    next_item: u32,
    /// The algorithms `OpenSession` was asked for, and the sessions still open.
    algorithms: Vec<String>,
    open_sessions: BTreeSet<String>,
    next_session: u32,
    created: Vec<Created>,
    prompted: u32,
    dismissed: u32,
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap()
}

/// The Secret Service's own error names.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.Secret.Error")]
enum SecretError {
    #[zbus(error)]
    ZBus(zbus::Error),
    IsLocked(String),
    NoSuchObject(String),
    NoSession(String),
}

/// What D-Bus answers for an algorithm the service does not support.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.DBus.Error")]
enum BusError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotSupported(String),
}

fn path(text: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(text.to_owned()).unwrap()
}

/// The path a method was called at.
fn called_at(header: &Header<'_>) -> String {
    header.path().unwrap().to_string()
}

/// Whether `item`'s attributes hold every one of `wanted`.
fn matches(item: &Item, wanted: &HashMap<String, String>) -> bool {
    wanted.iter().all(|(key, value)| item.attributes.get(key) == Some(value))
}

struct FakeService(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl FakeService {
    fn open_session(
        &self,
        algorithm: String,
        _input: OwnedValue,
    ) -> Result<(OwnedValue, OwnedObjectPath), BusError> {
        let mut state = lock(&self.0);
        state.algorithms.push(algorithm.clone());
        if algorithm != "plain" {
            return Err(BusError::NotSupported(algorithm));
        }
        state.next_session += 1;
        let session = format!("{SERVICE_PATH}/session/{}", state.next_session);
        state.open_sessions.insert(session.clone());
        Ok((OwnedValue::try_from(zbus::zvariant::Value::from("")).unwrap(), path(&session)))
    }

    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let state = lock(&self.0);
        let found: Vec<_> = state
            .items
            .iter()
            .filter(|(_, item)| matches(item, &attributes))
            .map(|(item, _)| path(item))
            .collect();
        if state.locked { (Vec::new(), found) } else { (found, Vec::new()) }
    }

    fn read_alias(&self, name: String) -> OwnedObjectPath {
        let state = lock(&self.0);
        if name != "default" || state.no_default { path("/") } else { path(COLLECTION_PATH) }
    }
}

struct FakeCollection(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl FakeCollection {
    fn create_item(
        &self,
        properties: HashMap<String, OwnedValue>,
        secret: (OwnedObjectPath, Vec<u8>, Vec<u8>, String),
        replace: bool,
    ) -> Result<(OwnedObjectPath, OwnedObjectPath), SecretError> {
        let mut state = lock(&self.0);
        if state.locked {
            return Err(SecretError::IsLocked("the collection is locked".to_owned()));
        }
        let label = properties.get("org.freedesktop.Secret.Item.Label").unwrap();
        let attributes = properties.get("org.freedesktop.Secret.Item.Attributes").unwrap();
        let attributes =
            HashMap::<String, String>::try_from(attributes.try_clone().unwrap()).unwrap();
        let (session, parameters, value, content_type) = secret;
        assert!(parameters.is_empty(), "plain has no parameters");
        let session_open = state.open_sessions.contains(session.as_str());
        state.created.push(Created {
            label: String::try_from(label.try_clone().unwrap()).unwrap(),
            attributes: attributes.clone().into_iter().collect(),
            content_type,
            session_open,
        });
        if state.prompts {
            return Ok((path("/"), path(PROMPT_PATH)));
        }
        let item = Item { attributes, value };
        let same = state.items.iter().find(|(_, old)| old.attributes == item.attributes);
        let at = match same.map(|(at, _)| at.clone()) {
            Some(at) if replace => at,
            _ => {
                state.next_item += 1;
                format!("{COLLECTION_PATH}/{}", state.next_item)
            }
        };
        state.items.insert(at.clone(), item);
        Ok((path(&at), path("/")))
    }
}

struct FakeItem(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl FakeItem {
    fn get_secret(
        &self,
        session: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<((OwnedObjectPath, Vec<u8>, Vec<u8>, String),), SecretError> {
        let state = lock(&self.0);
        let item = state.items.get(&called_at(&header));
        let item = item.ok_or_else(|| SecretError::NoSuchObject("no item".to_owned()))?;
        if state.locked {
            return Err(SecretError::IsLocked("the item is locked".to_owned()));
        }
        if !state.open_sessions.contains(session.as_str()) {
            return Err(SecretError::NoSession("no such session".to_owned()));
        }
        Ok(((session, Vec::new(), item.value.clone(), "text/plain".to_owned()),))
    }

    fn delete(&self, #[zbus(header)] header: Header<'_>) -> Result<OwnedObjectPath, SecretError> {
        let mut state = lock(&self.0);
        let at = called_at(&header);
        if !state.items.contains_key(&at) {
            return Err(SecretError::NoSuchObject("no item".to_owned()));
        }
        if state.locked {
            return Err(SecretError::IsLocked("the item is locked".to_owned()));
        }
        if state.prompts {
            return Ok(path(PROMPT_PATH));
        }
        state.items.remove(&at);
        Ok(path("/"))
    }
}

struct FakeSession(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Session")]
impl FakeSession {
    fn close(&self, #[zbus(header)] header: Header<'_>) {
        lock(&self.0).open_sessions.remove(&called_at(&header));
    }
}

struct FakePrompt(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Prompt")]
impl FakePrompt {
    fn prompt(&self, _window_id: String) {
        lock(&self.0).prompted += 1;
    }

    fn dismiss(&self) {
        lock(&self.0).dismissed += 1;
    }
}

/// A Secret Service that never answers a search or an alias.
struct Silent;

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Silent {
    async fn search_items(&self, _attributes: HashMap<String, String>) -> Vec<OwnedObjectPath> {
        std::future::pending().await
    }

    async fn read_alias(&self, _name: String) -> OwnedObjectPath {
        std::future::pending().await
    }
}

/// The fake on a multi-thread runtime of its own, its state shared with the test.
struct Fake {
    runtime: tokio::runtime::Runtime,
    service: Option<zbus::Connection>,
    state: Shared,
}

impl Fake {
    /// Serves a Secret Service on `bus` whose state starts as `state`.
    fn start(bus: &PrivateBus, state: State) -> Self {
        let runtime = runtime();
        let state = Arc::new(Mutex::new(state));
        let service = runtime.block_on(async {
            let service = eileen_dbus_linux::testing::serve(
                bus,
                NAME,
                SERVICE_PATH,
                FakeService(state.clone()),
            )
            .await;
            let server = service.object_server();
            server.at(COLLECTION_PATH, FakeCollection(state.clone())).await.unwrap();
            server.at(PROMPT_PATH, FakePrompt(state.clone())).await.unwrap();
            for n in 1..=SLOTS {
                let item = format!("{COLLECTION_PATH}/{n}");
                server.at(item, FakeItem(state.clone())).await.unwrap();
                let session = format!("{SERVICE_PATH}/session/{n}");
                server.at(session, FakeSession(state.clone())).await.unwrap();
            }
            service
        });
        Self { runtime, service: Some(service), state }
    }

    /// Serves a Secret Service on `bus` that never answers.
    fn silent(bus: &PrivateBus) -> Self {
        let runtime = runtime();
        let service =
            runtime.block_on(eileen_dbus_linux::testing::serve(bus, NAME, SERVICE_PATH, Silent));
        Self { runtime, service: Some(service), state: Shared::default() }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Stores `value` under `name` as another tool would, with the store's attributes.
    fn insert(&self, name: &str, value: &[u8]) {
        let mut state = self.state();
        state.next_item += 1;
        let at = format!("{COLLECTION_PATH}/{}", state.next_item);
        let attributes = HashMap::from([
            ("service".to_owned(), SERVICE.to_owned()),
            ("username".to_owned(), name.to_owned()),
        ]);
        state.items.insert(at, Item { attributes, value: value.to_vec() });
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        // The connection ends inside its runtime's context, a panicking test's included.
        let _context = self.runtime.enter();
        self.service = None;
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

/// A store on a bridge to `bus`, with `deadline`.
fn store_on(bus: &PrivateBus, deadline: Deadline) -> LinuxSecretStore {
    let settings = SecretSettings { service: SERVICE.to_owned(), deadline };
    LinuxSecretStore::new(Bridge::on_private_bus(bus).unwrap(), settings)
}

fn secret(text: &str) -> SecretString {
    SecretString::from(text)
}

/// The secret under `name`, exposed for the assertion.
fn got(store: &LinuxSecretStore, name: &str) -> Option<String> {
    store.get(name).unwrap().map(|value| value.expose_secret().to_owned())
}

const LOCKED: PlatformError = PlatformError::Refused { what: "secret", reason: "locked" };
const NEEDS_CONFIRMATION: PlatformError =
    PlatformError::Refused { what: "secret", reason: "needs the user's confirmation" };

#[test]
fn a_secret_round_trips_through_a_plain_session() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    let store = store_on(&bus, DEFAULT_DEADLINE);
    store.set("deepseek", &secret(VALUE)).unwrap();
    assert_eq!(got(&store, "deepseek").as_deref(), Some(VALUE), "round trip");
    {
        let state = fake.state();
        assert_eq!(state.algorithms, ["plain", "plain"], "one plain session per call");
        assert!(state.open_sessions.is_empty(), "every session closed");
        let attributes = BTreeMap::from([
            ("service".to_owned(), SERVICE.to_owned()),
            ("username".to_owned(), "deepseek".to_owned()),
        ]);
        let created = Created {
            label: "Eileen: deepseek".to_owned(),
            attributes,
            content_type: "text/plain".to_owned(),
            session_open: true,
        };
        assert_eq!(state.created, [created], "keyring's attributes, in an open session");
    }
    // Another tool's item under the same attributes is removed too.
    fake.insert("deepseek", b"from another tool");
    store.delete("deepseek").unwrap();
    assert!(fake.state().items.is_empty(), "every match deleted");
    assert_eq!(got(&store, "deepseek"), None, "gone");
}

#[test]
fn setting_a_name_again_replaces_its_item() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    let store = store_on(&bus, DEFAULT_DEADLINE);
    store.set("deepseek", &secret("first")).unwrap();
    store.set("other", &secret("kept")).unwrap();
    store.set("deepseek", &secret("second")).unwrap();
    assert_eq!(got(&store, "deepseek").as_deref(), Some("second"), "the new value");
    assert_eq!(got(&store, "other").as_deref(), Some("kept"), "another name is untouched");
    assert_eq!(fake.state().items.len(), 2, "replaced, not added");
}

#[test]
fn a_missing_secret_is_none_and_deleting_it_is_no_error() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    let store = store_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(got(&store, "deepseek"), None, "nothing stored");
    store.delete("deepseek").unwrap();
    store.delete("deepseek").unwrap();
    assert!(fake.state().algorithms.is_empty(), "a miss opens no session");
}

#[test]
fn a_locked_collection_is_refused_and_no_prompt_is_shown() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    fake.insert("deepseek", VALUE.as_bytes());
    fake.state().locked = true;
    let store = store_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(store.get("deepseek").err(), Some(LOCKED), "only a locked match");
    assert_eq!(store.delete("deepseek"), Err(LOCKED), "a locked match");
    assert_eq!(store.set("deepseek", &secret("new")), Err(LOCKED), "a locked collection");
    let state = fake.state();
    assert_eq!(state.items.len(), 1, "nothing deleted");
    assert_eq!((state.prompted, state.dismissed), (0, 0), "no prompt");
    assert!(state.open_sessions.is_empty(), "every session closed");
}

#[test]
fn a_change_that_needs_a_prompt_is_refused_and_the_prompt_dismissed() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State { prompts: true, ..State::default() });
    fake.insert("deepseek", VALUE.as_bytes());
    let store = store_on(&bus, DEFAULT_DEADLINE);
    assert_eq!(store.delete("deepseek"), Err(NEEDS_CONFIRMATION), "a delete");
    assert_eq!(store.set("deepseek", &secret("new")), Err(NEEDS_CONFIRMATION), "a store");
    let state = fake.state();
    assert_eq!(state.items.len(), 1, "nothing deleted");
    assert_eq!(state.prompted, 0, "never prompted");
    assert_eq!(state.dismissed, 2, "each prompt dismissed");
    assert!(state.open_sessions.is_empty(), "every session closed");
}

#[test]
fn without_a_default_collection_set_is_not_found_and_get_is_none() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State { no_default: true, ..State::default() });
    let store = store_on(&bus, DEFAULT_DEADLINE);
    let missing = PlatformError::NotFound { what: "default secret collection" };
    assert_eq!(store.set("deepseek", &secret(VALUE)), Err(missing), "making one needs a prompt");
    assert_eq!(got(&store, "deepseek"), None, "nothing stored");
    assert!(fake.state().created.is_empty(), "no item created");
}

#[test]
fn no_secret_service_is_unavailable() {
    let bus = PrivateBus::start();
    let store = store_on(&bus, DEFAULT_DEADLINE);
    let absent = PlatformError::Unavailable { what: "Secret Service" };
    assert_eq!(store.get("deepseek").err(), Some(absent.clone()), "get");
    assert_eq!(store.set("deepseek", &secret(VALUE)), Err(absent.clone()), "set");
    assert_eq!(store.delete("deepseek"), Err(absent), "delete");
}

#[test]
fn a_service_that_never_answers_passes_the_deadline() {
    let bus = PrivateBus::start();
    let _fake = Fake::silent(&bus);
    let store = store_on(&bus, Deadline::after(Duration::from_millis(100)));
    let late = |call| Err(PlatformError::DeadlineExceeded { call });
    assert_eq!(store.get("deepseek").map(|_| ()), late("secret lookup"), "get");
    assert_eq!(store.set("deepseek", &secret(VALUE)), late("secret store"), "set");
    assert_eq!(store.delete("deepseek"), late("secret removal"), "delete");
}

#[test]
fn a_value_that_is_not_utf8_is_invalid_and_never_shown() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    fake.insert("deepseek", b"sk-\xff\xfe-hidden");
    let store = store_on(&bus, DEFAULT_DEADLINE);
    let error = store.get("deepseek").unwrap_err();
    assert!(matches!(error, PlatformError::Invalid { what: "secret", .. }), "{error:?}");
    for shown in [format!("{error}"), format!("{error:?}")] {
        assert!(!shown.contains("hidden") && !shown.contains("sk-"), "never shown: {shown}");
    }
    assert!(fake.state().open_sessions.is_empty(), "the session closed");
}

#[test]
fn no_secret_shows_in_debug_or_in_an_error() {
    let bus = PrivateBus::start();
    let fake = Fake::start(&bus, State::default());
    let store = store_on(&bus, DEFAULT_DEADLINE);
    let value = secret(VALUE);
    store.set("deepseek", &value).unwrap();
    fake.state().locked = true;
    let errors = [store.get("deepseek"), store.set("deepseek", &value).map(|()| None)];
    let mut shown = vec![format!("{store:?}"), format!("{value:?}")];
    for error in errors {
        let error = error.unwrap_err();
        shown.extend([format!("{error}"), format!("{error:?}")]);
    }
    for text in shown {
        assert!(!text.contains(VALUE), "never shown: {text}");
    }
}

#[test]
fn a_name_that_is_empty_or_holds_a_nul_is_invalid_before_any_call() {
    // No service is on the bus: a call would answer `Unavailable`.
    let bus = PrivateBus::start();
    let store = store_on(&bus, DEFAULT_DEADLINE);
    for (name, detail) in [("", "empty"), ("deep\0seek", "holds a NUL character")] {
        let invalid =
            Err(PlatformError::Invalid { what: "secret name", detail: detail.to_owned() });
        assert_eq!(store.get(name).map(|_| ()), invalid, "get {name:?}");
        assert_eq!(store.set(name, &secret(VALUE)), invalid, "set {name:?}");
        assert_eq!(store.delete(name), invalid, "delete {name:?}");
    }
}

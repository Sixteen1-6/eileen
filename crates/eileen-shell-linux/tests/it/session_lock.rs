//! The session's lock state against fake logind sessions on a private bus.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use eileen_dbus_linux::testing::{PrivateBus, serve};
use eileen_shell_linux::{DbusError, SessionLock};

/// logind's name.
const LOGIND: &str = "org.freedesktop.login1";
/// The caller's session object.
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
/// Another session's object, such as a greeter's.
const OTHER_SESSION_PATH: &str = "/org/freedesktop/login1/session/c1";
/// The interface of a session object.
const SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";
/// What logind's bus answers on an object it does not find.
const UNKNOWN_OBJECT: &str = "org.freedesktop.DBus.Error.UnknownObject";
/// What the errors name.
const SESSION: &str = "logind session";

/// A fake logind session: its `LockedHint`, and how many times it was asked to lock.
#[derive(Debug, Default, Clone)]
struct FakeSession {
    locked: Arc<AtomicBool>,
    locks: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl FakeSession {
    fn lock(&self) {
        self.locks.fetch_add(1, Ordering::SeqCst);
    }

    #[zbus(property)]
    fn locked_hint(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }
}

/// A session from a logind older than `LockedHint` (systemd 230), counting its locks.
#[derive(Debug, Default, Clone)]
struct OldSession {
    locks: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl OldSession {
    fn lock(&self) {
        self.locks.fetch_add(1, Ordering::SeqCst);
    }
}

/// A session whose `LockedHint` is text rather than a boolean.
struct OddSession {
    hint: &'static str,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl OddSession {
    #[zbus(property)]
    fn locked_hint(&self) -> String {
        self.hint.to_owned()
    }
}

/// logind's own error names.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.login1")]
enum LoginError {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The caller has no session and no display session: what logind's manager calls answer.
    #[zbus(name = "NoSessionForPID")]
    NoSession(String),
}

/// A session object that answers `Lock()` with `NoSessionForPID` and logind's message, which
/// logind's own session objects never do: the guard's case.
struct NoSession {
    message: &'static str,
}

#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl NoSession {
    fn lock(&self) -> Result<(), LoginError> {
        Err(LoginError::NoSession(self.message.to_owned()))
    }
}

/// Serves `session` as the caller's logind session.
async fn serve_session<I: zbus::object_server::Interface>(
    bus: &PrivateBus,
    session: I,
) -> zbus::Connection {
    serve(bus, LOGIND, SESSION_PATH, session).await
}

#[tokio::test]
async fn the_lock_state_is_the_sessions_locked_hint() {
    let bus = PrivateBus::start();
    let session = FakeSession::default();
    let _logind = serve_session(&bus, session.clone()).await;
    let lock = SessionLock::new(&bus.connect().await);
    assert!(!lock.session_locked().await.unwrap(), "unlocked");
    session.locked.store(true, Ordering::SeqCst);
    assert!(lock.session_locked().await.unwrap(), "locked, read afresh");
}

#[tokio::test]
async fn locking_asks_the_session_once() {
    let bus = PrivateBus::start();
    let session = FakeSession::default();
    let _logind = serve_session(&bus, session.clone()).await;
    let lock = SessionLock::new(&bus.connect().await);
    lock.lock_workstation().await.unwrap();
    assert_eq!(session.locks.load(Ordering::SeqCst), 1, "Lock() was called");
}

#[tokio::test]
async fn an_older_logind_without_the_hint_has_no_such_property() {
    let bus = PrivateBus::start();
    let session = OldSession::default();
    let _logind = serve_session(&bus, session.clone()).await;
    let lock = SessionLock::new(&bus.connect().await);
    let error = lock.session_locked().await.unwrap_err();
    assert_eq!(error, DbusError::NotFound { what: SESSION }, "no LockedHint");
    lock.lock_workstation().await.unwrap();
    assert_eq!(session.locks.load(Ordering::SeqCst), 1, "it can still be asked to lock");
}

#[tokio::test]
async fn a_hint_that_is_not_a_boolean_is_invalid() {
    let bus = PrivateBus::start();
    let _logind = serve_session(&bus, OddSession { hint: "yes" }).await;
    let lock = SessionLock::new(&bus.connect().await);
    let error = lock.session_locked().await.unwrap_err();
    assert!(matches!(error, DbusError::Invalid { what: SESSION, .. }), "{error}");
}

#[tokio::test]
async fn a_caller_outside_every_session_finds_none() {
    let bus = PrivateBus::start();
    // logind serves the sessions it knows, and for a caller with no session and no display
    // session none at `session/auto`: its bus answers UnknownObject there.
    let _logind = serve(&bus, LOGIND, OTHER_SESSION_PATH, FakeSession::default()).await;
    let caller = bus.connect().await;
    let raw = caller
        .call_method(Some(LOGIND), SESSION_PATH, Some(SESSION_INTERFACE), "Lock", &())
        .await
        .unwrap_err();
    assert!(
        matches!(&raw, zbus::Error::MethodError(name, _, _) if name.as_str() == UNKNOWN_OBJECT),
        "the fake answers as logind does: {raw}"
    );
    let lock = SessionLock::new(&caller);
    let none = DbusError::NotFound { what: SESSION };
    assert_eq!(lock.session_locked().await.unwrap_err(), none, "no hint to read");
    let error = lock.lock_workstation().await.unwrap_err();
    assert_eq!(error, none, "no session to lock");
    assert!(!error.to_string().contains("/org"), "no service text in the error: {error}");
}

#[tokio::test]
async fn a_no_session_for_pid_answer_finds_none_too() {
    let bus = PrivateBus::start();
    let session = NoSession { message: "Caller does not belong to any known session." };
    let _logind = serve_session(&bus, session).await;
    let lock = SessionLock::new(&bus.connect().await);
    let error = lock.lock_workstation().await.unwrap_err();
    assert_eq!(error, DbusError::NotFound { what: SESSION }, "NoSessionForPID, the guard");
    assert!(!error.to_string().contains("Caller"), "no service text in the error: {error}");
}

#[tokio::test]
async fn without_logind_the_lock_state_is_unavailable() {
    let bus = PrivateBus::start();
    let lock = SessionLock::new(&bus.connect().await);
    let unavailable = DbusError::Unavailable { what: SESSION };
    assert_eq!(lock.session_locked().await.unwrap_err(), unavailable, "no hint to read");
    assert_eq!(lock.lock_workstation().await.unwrap_err(), unavailable, "no session to lock");
}

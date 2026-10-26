//! The session's lock state, and the request to lock it, on logind for the lane's own session.
//! Nothing in the headless session locks the screen, so the state stays unlocked whatever the
//! tests ask; what a request does is make logind send the session's `Lock` signal, which a
//! desktop's screen locker acts on. So these tests run in the lane's session alone (`main.rs`): on
//! a developer's desktop the request can lock the screen, after which the unlocked check can fail,
//! or, where the desktop's apps run in the user's own systemd units rather than in the session, as
//! on GNOME, the lock test finds no session for its process.

use eileen_dbus_linux::testing::{ready_now, round_trip};
use eileen_platform::ShellHost;
use zbus::message::Type;
use zbus::zvariant::OwnedObjectPath;
use zbus::{MatchRule, MessageStream};

use super::{LOGIND, MANAGER, MANAGER_PATH, Session, shell};

/// The interface of a logind session object.
const SESSION: &str = "org.freedesktop.login1.Session";

#[test]
fn the_session_reads_unlocked() {
    assert_eq!(
        shell().session_locked(),
        Ok(false),
        "the lane's own session, unlocked: outside every session, logind's answer reads NotFound"
    );
}

#[test]
fn a_lock_request_reaches_logind_for_this_session() {
    let session = Session::new();
    let system = session.system();
    let shell = shell();
    let lock = session.block_on(async {
        let pid = std::process::id();
        let reply = system
            .call_method(Some(LOGIND), MANAGER_PATH, Some(MANAGER), "GetSessionByPID", &(pid,))
            .await
            .unwrap();
        let path: OwnedObjectPath = reply.body().deserialize().unwrap();
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(LOGIND)
            .unwrap()
            .path(path.as_str())
            .unwrap()
            .interface(SESSION)
            .unwrap()
            .member("Lock")
            .unwrap()
            .build();
        let mut locks = MessageStream::for_match_rule(rule, system, None).await.unwrap();
        // This blocks the test's thread, not the workers, which queue the signal meanwhile.
        shell.lock_workstation().unwrap();
        // logind sends the signal before it answers `Lock()`, so once a round trip to logind
        // has returned, the signal is in the stream if logind sent it.
        round_trip(system, Some(LOGIND)).await;
        ready_now(&mut locks)
    });
    assert!(
        matches!(lock, Some(Ok(_))),
        "logind sent the Lock signal of the session this process is in: {lock:?}"
    );
}

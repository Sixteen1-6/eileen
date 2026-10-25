//! Keeping the session awake. Through the shell, on logind: its lock is listed while the request
//! lives, as `systemd-inhibit --list` shows it, and gone once the request drops; the trait's
//! `PowerRequest` does not say what holds. Through the client, `LinuxPowerRequest`, which does:
//! the session has no GNOME session manager, so the desktop's hold is the Inhibit portal's (its
//! GTK backend then refuses suspend with idle and takes idle alone, which the client asks next),
//! and releasing the request closes the portal's.

use std::time::Duration;

use eileen_dbus_linux::session_bus;
use eileen_dbus_linux::testing::next_message;
use eileen_platform::ShellHost;
use eileen_shell_linux::{DesktopHolder, LinuxPowerRequest};
use zbus::message::Type;
use zbus::{MatchRule, MessageStream};

use super::{LOGIND, MANAGER, MANAGER_PATH, Session, shell};

/// The lock `keep_awake` takes, as logind lists it: what it inhibits, who, why and its mode.
const LOCK: [&str; 4] = ["sleep:idle", "Eileen", "An armed run is in progress", "block"];

/// Why the client's test keeps the session awake, as the desktop and logind show it.
const WHY: &str = "A session test of the keep-awake holders";

/// The client's bound on the portal's answers: a deadline it keeps, which the lane's portal
/// meets in milliseconds.
const PORTAL_BUDGET: Duration = Duration::from_secs(5);

/// An entry of logind's `ListInhibitors`: what, who, why, mode, user ID and process ID.
type Inhibitor = (String, String, String, String, u32, u32);

/// The locks logind lists for this process: what, who, why and mode of each.
async fn locks_of_this_process(system: &zbus::Connection) -> Vec<[String; 4]> {
    let reply = system
        .call_method(Some(LOGIND), MANAGER_PATH, Some(MANAGER), "ListInhibitors", &())
        .await
        .unwrap();
    let inhibitors: Vec<Inhibitor> = reply.body().deserialize().unwrap();
    inhibitors
        .into_iter()
        .filter(|&(.., pid)| pid == std::process::id())
        .map(|(what, who, why, mode, ..)| [what, who, why, mode])
        .collect()
}

#[test]
fn logind_lists_the_lock_until_the_request_drops() {
    let session = Session::new();
    let system = session.system();
    let shell = shell();
    session.block_on(async {
        let request = shell.keep_awake().unwrap();
        assert_eq!(
            locks_of_this_process(system).await,
            [LOCK.map(str::to_owned)],
            "logind holds Eileen's lock on sleep and its idle action for this process"
        );
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(LOGIND)
            .unwrap()
            .path(MANAGER_PATH)
            .unwrap()
            .interface("org.freedesktop.DBus.Properties")
            .unwrap()
            .member("PropertiesChanged")
            .unwrap()
            .arg(0, MANAGER)
            .unwrap()
            .build();
        let mut changes = MessageStream::for_match_rule(rule, system, None).await.unwrap();
        // The drop returns once the holder has closed logind's descriptor. logind sees it close
        // in its own time, drops the lock and then says so in the manager's `BlockInhibited`.
        drop(request);
        while !locks_of_this_process(system).await.is_empty() {
            let _changed = next_message(&mut changes).await;
        }
    });
}

#[test]
fn the_desktop_hold_comes_from_the_inhibit_portal() {
    let session = Session::new();
    let system = session.system();
    session.block_on(async {
        // A session connection of the request's own, as `keep_awake` asks.
        let own = session_bus().await.unwrap();
        let request = LinuxPowerRequest::keep_awake(Some(system), Some(own), WHY, PORTAL_BUDGET)
            .await
            .unwrap();
        assert!(request.holds_sleep(), "logind's lock holds: {request:?}");
        assert_eq!(
            request.desktop(),
            Some(DesktopHolder::Portal),
            "with no GNOME session manager on the bus, the portal holds: {request:?}"
        );
        assert!(request.missed().is_empty(), "neither holder missed: {request:?}");
        assert_eq!(request.release().await, Ok(()), "the portal answered the request's Close");
    });
}

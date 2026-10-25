//! Keeping the session awake. Through the shell, on logind: its lock is listed while the request
//! lives, as `systemd-inhibit --list` shows it, and gone once the request drops; the trait's
//! `PowerRequest` does not say what holds. Through the client, `LinuxPowerRequest`, which does:
//! with no GNOME session manager in the session, the desktop's request goes to the Inhibit
//! portal, whose GTK backend refuses suspend with idle and takes idle alone, which the client asks
//! next. Taking it is all the portal's `Response` 0 says: the backend answers before its own call
//! to `org.freedesktop.ScreenSaver` returns (xdg-desktop-portal-gtk's `handle_inhibit_fdo`), and
//! in the lane that call fails, since nothing owns the name. So the test checks the portal's
//! protocol, not that the screen stays on. Releasing the request closes the portal's, which the
//! test sees leave the requests the portal exports for the request's connection.

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

/// The portal's name, the prefix of its requests' handles, under which a caller's are at
/// `SENDER/TOKEN` (the Request documentation), and the interface that lists an object's children.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const REQUESTS: &str = "/org/freedesktop/portal/desktop/request";
const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

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

/// Where the portal exports the requests of the caller on `connection`: the handles' prefix and
/// the caller's unique name without its `:` and with each `.` as `_`.
fn request_folder(connection: &zbus::Connection) -> String {
    let name = connection.unique_name().unwrap().as_str();
    format!("{REQUESTS}/{}", name.trim_start_matches(':').replace('.', "_"))
}

/// The requests the portal exports in `folder`, by token: the children its introspection lists.
async fn exported_requests(session: &zbus::Connection, folder: &str) -> Vec<String> {
    let reply = session
        .call_method(Some(PORTAL), folder, Some(INTROSPECTABLE), "Introspect", &())
        .await
        .unwrap();
    let xml: String = reply.body().deserialize().unwrap();
    xml.split("<node name=\"")
        .skip(1)
        .filter_map(|node| node.split_once('"').map(|(token, _rest)| token.to_owned()))
        .collect()
}

#[test]
fn the_inhibit_portal_takes_the_desktop_request() {
    let session = Session::new();
    let system = session.system();
    session.block_on(async {
        // A session connection for the request, as `keep_awake` asks. The test keeps a clone and
        // calls `release`, as the client asks of a caller that keeps one, so only the request's
        // `Close` can end the portal's request, not the bus seeing the connection close.
        let own = session_bus().await.unwrap();
        let folder = request_folder(&own);
        let request =
            LinuxPowerRequest::keep_awake(Some(system), Some(own.clone()), WHY, PORTAL_BUDGET)
                .await
                .unwrap();
        assert!(request.holds_sleep(), "logind's lock holds: {request:?}");
        assert_eq!(
            request.desktop(),
            Some(DesktopHolder::Portal),
            "with no GNOME session manager, the portal took the request (Response 0): {request:?}"
        );
        assert!(request.missed().is_empty(), "neither holder missed: {request:?}");
        let before = exported_requests(&own, &folder).await;
        assert_eq!(request.release().await, Ok(()), "release() answers Ok");
        // The portal unexports a request it closes before it answers the `Close`. It keeps the
        // refused one until the connection closes, so the check counts the requests that left.
        let after = exported_requests(&own, &folder).await;
        let closed = before.iter().filter(|token| !after.contains(token)).count();
        assert_eq!(
            closed, 1,
            "release closed the portal's request: it exported {before:?}, then {after:?}"
        );
    });
}

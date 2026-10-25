//! Keeping the session awake, on logind: its lock is listed while the request lives, as
//! `systemd-inhibit --list` shows it, and gone once the request drops. The session has no GNOME
//! session manager, so the desktop's hold is the Inhibit portal's; the trait's `PowerRequest`
//! does not say what holds, so only logind's lock is checked.

use eileen_dbus_linux::testing::next_message;
use eileen_platform::ShellHost;
use zbus::message::Type;
use zbus::{MatchRule, MessageStream};

use super::{LOGIND, MANAGER, MANAGER_PATH, Session, shell};

/// The lock `keep_awake` takes, as logind lists it: what it inhibits, who, why and its mode.
const LOCK: [&str; 4] = ["sleep:idle", "Eileen", "An armed run is in progress", "block"];

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

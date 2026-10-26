//! A toast, to the session's notification server: notification-daemon in the Linux desktop lane,
//! which takes Eileen's `Notify` as the notifier sends it.

use eileen_contracts::core_io::Toast;
use eileen_platform::ShellHost;

use super::shell;

#[test]
fn the_notification_server_takes_a_toast() {
    let toast = Toast {
        title: "Session test".to_owned(),
        body: "A toast from the Linux desktop lane".to_owned(),
        job: None,
    };
    assert_eq!(
        shell().toast(&toast),
        Ok(()),
        "the server answered Notify with an id: with no server, the toast is Unavailable"
    );
}

//! The notification client against the session's notification server, notification-daemon in
//! the Linux desktop lane: what the server says it is and can do, and a notification closed by
//! the call, whose end comes back as an event. `ShellHost::toast` drops the notification's id, so
//! these go through the client.

use eileen_contracts::core_io::Toast;
use eileen_dbus_linux::session_bus;
use eileen_shell_linux::{NotificationCloseReason, NotificationEvent, Notifier, NotifierSettings};

use super::Session;

/// A notifier on a session connection of its own, which names Eileen as the shell's does.
async fn notifier() -> Notifier {
    Notifier::new(&session_bus().await.unwrap(), &NotifierSettings::default())
}

#[test]
fn the_server_names_itself_and_shows_a_body() {
    let session = Session::new();
    let (info, capabilities) = session.block_on(async {
        let notifier = notifier().await;
        (notifier.server_information().await.unwrap(), notifier.capabilities().await.unwrap())
    });
    assert!(!info.name.get().is_empty(), "the server names itself: {info:?}");
    assert!(!info.spec_version.get().is_empty(), "and the specification it follows: {info:?}");
    assert!(capabilities.contains("body"), "it shows a toast's body: {capabilities:?}");
}

#[test]
fn closing_a_notification_reports_the_call() {
    let session = Session::new();
    let reason = session.block_on(async {
        let notifier = notifier().await;
        // The stream first: a signal sent before it exists never reaches it.
        let mut events = notifier.events().await.unwrap();
        let toast = Toast {
            title: "Session test".to_owned(),
            body: "A notification the test closes".to_owned(),
            job: None,
        };
        let id = notifier.notify(&toast).await.unwrap();
        notifier.close(&id).await.unwrap();
        // The server broadcasts every notification's events, so others' pass by.
        loop {
            if let NotificationEvent::Closed { id: closed, reason } = events.next().await.unwrap()
                && closed == id
            {
                break reason;
            }
        }
    });
    assert_eq!(reason, NotificationCloseReason::Closed, "closed by the call, reason 3");
}

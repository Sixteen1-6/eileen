//! Name owners and their changes, against fake services on the private bus.

use eileen_dbus_linux::testing::{PrivateBus, nothing_ready, round_trip, serve};
use eileen_dbus_linux::{DbusError, Names, OwnerChange, name_owner, owner_changes};

/// The name the tests watch.
const NAME: &str = "org.eileen.Test.Player";
/// The object every fake serves.
const PATH: &str = "/org/eileen/Test";

/// A fake service with nothing to call.
struct Idle;

#[zbus::interface(name = "org.eileen.Test.Idle")]
impl Idle {}

/// A fake owning `name`.
async fn fake(bus: &PrivateBus, name: &str) -> zbus::Connection {
    serve(bus, name, PATH, Idle).await
}

/// The unique name of `conn`.
fn unique(conn: &zbus::Connection) -> String {
    conn.unique_name().unwrap().to_string()
}

/// The change's name and owners, as text.
fn names(change: &OwnerChange) -> (String, Option<String>, Option<String>) {
    let text = |owner: Option<&zbus::names::OwnedUniqueName>| owner.map(ToString::to_string);
    (change.name.to_string(), text(change.old.as_ref()), text(change.new.as_ref()))
}

#[tokio::test]
async fn the_owner_is_none_while_nothing_owns_the_name() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    assert_eq!(name_owner(&client, NAME).await, Ok(None), "no owner, not an error");
}

#[tokio::test]
async fn the_owner_is_the_unique_name_of_the_connection_that_holds_the_name() {
    let bus = PrivateBus::start();
    let service = fake(&bus, NAME).await;
    let client = bus.connect().await;
    let owner = name_owner(&client, NAME).await.unwrap().expect("an owner");
    assert_eq!(owner.as_str(), unique(&service), "the fake's connection");
}

#[tokio::test]
async fn a_name_that_is_not_a_bus_name_is_invalid_before_the_bus_is_asked() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    for name in ["", "no dots allowed here", "org..eileen", ":"] {
        assert_eq!(
            name_owner(&client, name).await,
            Err(DbusError::Invalid { what: "name owner", detail: "not a bus name".to_owned() }),
            "{name:?}"
        );
    }
}

#[tokio::test]
async fn owner_changes_follow_a_name_starting_and_stopping_and_nothing_else() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    let mut changes = owner_changes(&client, Names::Name(NAME)).await.unwrap();
    let service = fake(&bus, NAME).await;
    let _other = fake(&bus, "org.eileen.Test.Player.Other").await;
    let started = changes.next().await.unwrap();
    assert_eq!(names(&started), (NAME.to_owned(), None, Some(unique(&service))), "started");
    drop(service);
    let stopped = changes.next().await.unwrap();
    assert_eq!(names(&stopped).2, None, "stopped: no owner now");
    assert!(stopped.received > started.received, "in the order the connection received them");
    round_trip(&client, None).await;
    assert!(nothing_ready(&mut changes), "no change of another name");
}

#[tokio::test]
async fn a_namespace_follows_its_name_and_the_names_under_it_only() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    let mut changes = owner_changes(&client, Names::Namespace(NAME)).await.unwrap();
    let _outside = fake(&bus, "org.eileen.Test.PlayerTwo").await;
    let _root = fake(&bus, NAME).await;
    let _under = fake(&bus, "org.eileen.Test.Player.Music").await;
    round_trip(&client, None).await;
    let first = changes.next().await.unwrap();
    let second = changes.next().await.unwrap();
    assert_eq!(first.name.as_str(), NAME, "the namespace's own name");
    assert_eq!(second.name.as_str(), "org.eileen.Test.Player.Music", "a name under it");
    assert!(nothing_ready(&mut changes), "not a name that only starts with the same text");
}

#[tokio::test]
async fn an_owner_change_that_another_connection_sends_is_dropped() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    let mut changes = owner_changes(&client, Names::Name(NAME)).await.unwrap();
    let forger = fake(&bus, "org.eileen.Test.Forger").await;
    round_trip(&client, None).await;
    let destination = unique(&client);
    let claim = (NAME, "", unique(&forger));
    forger
        .emit_signal(
            Some(destination.as_str()),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameOwnerChanged",
            &claim,
        )
        .await
        .unwrap();
    round_trip(&client, Some("org.eileen.Test.Forger")).await;
    assert!(nothing_ready(&mut changes), "only the bus's own NameOwnerChanged counts");
}

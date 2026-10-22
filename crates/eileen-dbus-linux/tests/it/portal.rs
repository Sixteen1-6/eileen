//! The portal `Request` client against a fake portal on the private bus.

use std::collections::HashMap;
use std::time::Duration;

use eileen_dbus_linux::testing::{PrivateBus, next_message, serve};
use eileen_dbus_linux::{DbusError, Request};
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, Value};

/// The portal's bus name.
const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// Its object.
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
/// The fake's own interface, standing in for a portal's.
const FAKE: &str = "org.eileen.Test.Portal";
/// The handle a portal before 0.9 names, ignoring the token.
const OLD_HANDLE: &str = "/org/freedesktop/portal/desktop/request/old/1";
/// The code that means "do not answer yet".
const SILENT: u32 = u32::MAX;
/// How long a test waits for an answer the fake sends.
const DEADLINE: Duration = Duration::from_secs(30);

/// A fake portal. `Ask` makes a request and answers with its handle: the one the token
/// predicts, or [`OLD_HANDLE`]; with a code other than [`SILENT`], it first sends that
/// `Response`. `Respond` sends a `Response` on a handle later, or a malformed one.
struct FakePortal;

#[zbus::interface(name = "org.eileen.Test.Portal")]
impl FakePortal {
    async fn ask(
        &self,
        token: &str,
        code: u32,
        old: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let sender = header.sender().unwrap().as_str().trim_start_matches(':').replace('.', "_");
        let handle = if old {
            OLD_HANDLE.to_owned()
        } else {
            format!("/org/freedesktop/portal/desktop/request/{sender}/{token}")
        };
        if code != SILENT {
            respond(conn, header.sender().unwrap().as_str(), &handle, code, false).await;
        }
        Ok(OwnedObjectPath::try_from(handle).unwrap())
    }

    async fn respond(
        &self,
        handle: &str,
        code: u32,
        malformed: bool,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) {
        respond(conn, header.sender().unwrap().as_str(), handle, code, malformed).await;
    }
}

/// Sends `Response(code, {})` on `handle` from `conn` to `to`, or one with a wrong body.
async fn respond(conn: &zbus::Connection, to: &str, handle: &str, code: u32, malformed: bool) {
    let (path, interface) = (handle, "org.freedesktop.portal.Request");
    if malformed {
        conn.emit_signal(Some(to), path, interface, "Response", &("not a code",)).await.unwrap();
    } else {
        let body = (code, HashMap::<&str, Value<'_>>::new());
        conn.emit_signal(Some(to), path, interface, "Response", &body).await.unwrap();
    }
}

/// The fake portal, and a client with a request subscribed to its responses.
async fn setup(bus: &PrivateBus) -> (zbus::Connection, zbus::Connection, Request) {
    let portal = serve(bus, PORTAL, PORTAL_PATH, FakePortal).await;
    let client = bus.connect().await;
    let mut request = Request::new(&client).unwrap();
    request.responses().await.unwrap();
    (portal, client, request)
}

/// Calls the fake's `member` with `body` from `client`.
async fn call<B>(client: &zbus::Connection, member: &str, body: &B) -> zbus::Message
where
    B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
{
    client.call_method(Some(PORTAL), PORTAL_PATH, Some(FAKE), member, body).await.unwrap()
}

#[tokio::test]
async fn the_portals_response_gives_its_code_even_when_it_comes_before_the_reply() {
    for code in [0_u32, 1, 2] {
        let bus = PrivateBus::start();
        let (_portal, client, mut request) = setup(&bus).await;
        let reply = call(&client, "Ask", &(request.token(), code, false)).await;
        let sender = request.follow(&reply).await.unwrap();
        assert_eq!(request.wait(&sender, DEADLINE).await, Ok(code), "code {code}");
    }
}

#[tokio::test]
async fn a_response_from_another_connection_is_passed_over() {
    let bus = PrivateBus::start();
    let (_portal, client, mut request) = setup(&bus).await;
    let reply = call(&client, "Ask", &(request.token(), SILENT, false)).await;
    let sender = request.follow(&reply).await.unwrap();
    let to = client.unique_name().unwrap().to_string();
    let stranger = bus.connect().await;
    respond(&stranger, &to, request.path().as_str(), 0, false).await;
    let handle = request.path().to_string();
    call(&client, "Respond", &(handle.as_str(), 2_u32, false)).await;
    assert_eq!(request.wait(&sender, DEADLINE).await, Ok(2), "only the portal's own answer");
}

#[tokio::test]
async fn a_handle_of_the_portals_own_is_followed() {
    let bus = PrivateBus::start();
    let (_portal, client, mut request) = setup(&bus).await;
    let predicted = request.path().to_string();
    let reply = call(&client, "Ask", &(request.token(), SILENT, true)).await;
    let sender = request.follow(&reply).await.unwrap();
    assert_eq!(request.path().as_str(), OLD_HANDLE, "the handle the portal named");
    assert_ne!(predicted, OLD_HANDLE, "not the predicted one");
    call(&client, "Respond", &(OLD_HANDLE, 1_u32, false)).await;
    assert_eq!(request.wait(&sender, DEADLINE).await, Ok(1), "the answer on that handle");
}

#[tokio::test]
async fn a_malformed_response_is_invalid() {
    let bus = PrivateBus::start();
    let (_portal, client, mut request) = setup(&bus).await;
    let reply = call(&client, "Ask", &(request.token(), SILENT, false)).await;
    let sender = request.follow(&reply).await.unwrap();
    let handle = request.path().to_string();
    call(&client, "Respond", &(handle.as_str(), 0_u32, true)).await;
    let answer = request.wait(&sender, DEADLINE).await;
    assert!(
        matches!(answer, Err(DbusError::Invalid { what: "portal response", .. })),
        "a body that is not (code, results): {answer:?}"
    );
}

#[tokio::test]
async fn waiting_without_subscribing_first_is_invalid() {
    let bus = PrivateBus::start();
    let client = bus.connect().await;
    let mut request = Request::new(&client).unwrap();
    let sender = client.unique_name().unwrap().to_owned();
    assert!(
        matches!(request.wait(&sender, DEADLINE).await, Err(DbusError::Invalid { .. })),
        "an answer could have been missed"
    );
}

#[tokio::test(start_paused = true)]
async fn a_request_that_misses_its_deadline_is_closed() {
    let bus = PrivateBus::start();
    let (portal, client, mut request) = setup(&bus).await;
    let mut received = zbus::MessageStream::from(&portal);
    let reply = call(&client, "Ask", &(request.token(), SILENT, false)).await;
    let sender = request.follow(&reply).await.unwrap();
    assert_eq!(
        request.wait(&sender, DEADLINE).await,
        Err(DbusError::DeadlineExceeded { call: "portal response" }),
        "the portal never answered"
    );
    loop {
        let message = next_message(&mut received).await;
        let header = message.header();
        if header.member().is_some_and(|member| member == "Close") {
            assert_eq!(header.path(), Some(request.path()), "the request's own handle");
            assert!(
                header.interface().is_some_and(|name| name == "org.freedesktop.portal.Request"),
                "the Request interface's Close"
            );
            break;
        }
    }
}

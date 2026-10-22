//! Fake MPRIS players: a root interface and a `Player` interface that report what a test sets
//! and record the calls they receive, three players in the shapes real ones send, and the
//! helpers that put them on a private bus.

use std::collections::HashMap;
use std::sync::Arc;

use eileen_contracts::desktop::MediaState;
use eileen_contracts::ids::Untrusted;
use eileen_dbus_linux::testing::PrivateBus;
use tokio::sync::Notify;
use zbus::object_server::InterfaceRef;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

/// The object every player serves.
pub(super) const OBJECT: &str = "/org/mpris/MediaPlayer2";

/// A player that reports everything the specification asks for, and can do everything.
pub(super) const FULL: &str = "org.mpris.MediaPlayer2.full";

/// A second instance of a player that leaves out everything optional and sends odd types.
pub(super) const ODD: &str = "org.mpris.MediaPlayer2.odd.instance42";

/// A player with a floating length, a title in a second variant and a track id as a string.
pub(super) const FLOAT: &str = "org.mpris.MediaPlayer2.float";

/// The full player's track.
pub(super) const TRACK: &str = "/org/eileen/Track/1";

/// A call a fake player received.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Call {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
    Seek(i64),
    SetPosition(OwnedObjectPath, i64),
}

/// A fake player's root interface; `None` leaves the property out.
pub(super) struct Root {
    pub(super) identity: Option<&'static str>,
    pub(super) desktop_entry: Option<&'static str>,
}

/// What a player answers for a property it does not have: an error, which leaves the property
/// out of `GetAll`, as players built on `GLib` do.
fn missing<T>(value: Option<T>) -> zbus::fdo::Result<T> {
    value.ok_or_else(|| zbus::fdo::Error::NotSupported("no such property".to_owned()))
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> zbus::fdo::Result<String> {
        missing(self.identity.map(str::to_owned))
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> zbus::fdo::Result<String> {
        missing(self.desktop_entry.map(str::to_owned))
    }
}

/// Where a fake player's reads wait: a read that reaches the gate notifies `entered`, then
/// waits until `open` lets it through. No read reaches it unless a test sets it.
#[derive(Debug, Clone, Default)]
pub(super) struct Gate {
    /// Notified when a read reaches the gate.
    pub(super) entered: Arc<Notify>,
    /// Notified to let one waiting read through.
    pub(super) open: Arc<Notify>,
}

/// A fake player's `Player` interface; each `None` leaves the property out. It records the
/// calls it receives.
pub(super) struct Player {
    pub(super) status: &'static str,
    pub(super) metadata: Vec<(&'static str, Value<'static>)>,
    pub(super) position: Option<i64>,
    pub(super) volume: f64,
    pub(super) can_control: Option<bool>,
    pub(super) can_play: Option<bool>,
    pub(super) can_pause: Option<bool>,
    pub(super) can_go_next: Option<bool>,
    pub(super) can_go_previous: Option<bool>,
    pub(super) can_seek: Option<bool>,
    pub(super) calls: Vec<Call>,
    /// When set, reading `PlaybackStatus`, and so `GetAll`, waits at the gate.
    pub(super) gate: Option<Gate>,
}

impl Player {
    /// A player in `status` that says it can do everything.
    pub(super) fn new(
        status: &'static str,
        metadata: Vec<(&'static str, Value<'static>)>,
        position: Option<i64>,
    ) -> Self {
        Self {
            status,
            metadata,
            position,
            volume: 1.0,
            can_control: Some(true),
            can_play: Some(true),
            can_pause: Some(true),
            can_go_next: Some(true),
            can_go_previous: Some(true),
            can_seek: Some(true),
            calls: Vec::new(),
            gate: None,
        }
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&mut self) {
        self.calls.push(Call::Play);
    }

    fn pause(&mut self) {
        self.calls.push(Call::Pause);
    }

    fn play_pause(&mut self) {
        self.calls.push(Call::PlayPause);
    }

    fn next(&mut self) {
        self.calls.push(Call::Next);
    }

    fn previous(&mut self) {
        self.calls.push(Call::Previous);
    }

    fn seek(&mut self, offset: i64) {
        self.calls.push(Call::Seek(offset));
    }

    fn set_position(&mut self, track_id: ObjectPath<'_>, position: i64) {
        self.calls.push(Call::SetPosition(track_id.into(), position));
    }

    #[zbus(property)]
    async fn playback_status(&self) -> String {
        if let Some(gate) = &self.gate {
            gate.entered.notify_one();
            gate.open.notified().await;
        }
        self.status.to_owned()
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.metadata
            .iter()
            .map(|(key, value)| {
                ((*key).to_owned(), value.try_clone().unwrap().try_into_owned().unwrap())
            })
            .collect()
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> zbus::fdo::Result<i64> {
        missing(self.position)
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.volume
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn can_control(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_control)
    }

    #[zbus(property)]
    fn can_play(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_play)
    }

    #[zbus(property)]
    fn can_pause(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_pause)
    }

    #[zbus(property)]
    fn can_go_next(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_go_next)
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_go_previous)
    }

    #[zbus(property)]
    fn can_seek(&self) -> zbus::fdo::Result<bool> {
        missing(self.can_seek)
    }
}

/// A track id, as the specification gives it: an object path.
pub(super) fn track_path(path: &'static str) -> Value<'static> {
    Value::ObjectPath(ObjectPath::from_static_str(path).unwrap())
}

/// Playing, with full metadata: the track id as an object path, the artists as a list and the
/// length as a signed 64-bit number, as the specification gives them.
pub(super) fn full_player() -> Player {
    let metadata = vec![
        ("mpris:trackid", track_path(TRACK)),
        ("xesam:title", Value::from("Song One")),
        ("xesam:artist", Value::from(vec!["Ada", "Grace"])),
        ("xesam:album", Value::from("Album")),
        ("mpris:length", Value::I64(240_000_000)),
    ];
    Player::new("Playing", metadata, Some(42_500_000))
}

pub(super) const fn full_root() -> Root {
    Root { identity: Some("Full Player"), desktop_entry: Some("full-player") }
}

pub(super) fn full_state() -> MediaState {
    MediaState {
        session: FULL.to_owned(),
        app: "Full Player".to_owned(),
        title: Untrusted::new("Song One".to_owned()),
        artist: Some(Untrusted::new("Ada".to_owned())),
        position_s: Some(42.5),
        duration_s: Some(240.0),
        playing: true,
    }
}

/// Paused, with odd metadata: no title and no track id, the artist as one string and the length
/// as an unsigned number. It leaves out `Position`, every `Can` property and both names.
pub(super) fn odd_player() -> Player {
    let metadata =
        vec![("xesam:artist", Value::from("Solo")), ("mpris:length", Value::U64(90_000_000))];
    Player {
        can_control: None,
        can_play: None,
        can_pause: None,
        can_go_next: None,
        can_go_previous: None,
        can_seek: None,
        ..Player::new("Paused", metadata, None)
    }
}

pub(super) const fn odd_root() -> Root {
    Root { identity: None, desktop_entry: None }
}

pub(super) fn odd_state() -> MediaState {
    MediaState {
        session: ODD.to_owned(),
        app: "odd".to_owned(),
        title: Untrusted::new(String::new()),
        artist: Some(Untrusted::new("Solo".to_owned())),
        position_s: None,
        duration_s: Some(90.0),
        playing: false,
    }
}

/// Stopped, with the length as a floating number, the title inside a second variant and the
/// track id as a string; its only name is its desktop entry.
pub(super) fn float_player() -> Player {
    let metadata = vec![
        ("mpris:trackid", Value::from("/org/eileen/Track/9")),
        ("xesam:title", Value::Value(Box::new(Value::from("Deep")))),
        ("mpris:length", Value::F64(30_500_000.0)),
    ];
    Player::new("Stopped", metadata, Some(0))
}

pub(super) const fn float_root() -> Root {
    Root { identity: None, desktop_entry: Some("float-player") }
}

pub(super) fn float_state() -> MediaState {
    MediaState {
        session: FLOAT.to_owned(),
        app: "float-player".to_owned(),
        title: Untrusted::new("Deep".to_owned()),
        artist: None,
        position_s: Some(0.0),
        duration_s: Some(30.5),
        playing: false,
    }
}

/// Serves a fake player called `name`; it stays on the bus while the connection lives.
pub(super) async fn serve(
    bus: &PrivateBus,
    name: &'static str,
    root: Root,
    player: Player,
) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address())
        .unwrap()
        .name(name)
        .unwrap()
        .serve_at(OBJECT, root)
        .unwrap()
        .serve_at(OBJECT, player)
        .unwrap()
        .build()
        .await
        .unwrap()
}

/// A connection that owns `name` and serves nothing.
pub(super) async fn named(bus: &PrivateBus, name: &'static str) -> zbus::Connection {
    zbus::connection::Builder::address(bus.address())
        .unwrap()
        .name(name)
        .unwrap()
        .build()
        .await
        .unwrap()
}

/// The fake player a connection serves.
pub(super) async fn interface(service: &zbus::Connection) -> InterfaceRef<Player> {
    service.object_server().interface::<_, Player>(OBJECT).await.unwrap()
}

/// The calls the fake player has received.
pub(super) async fn calls(service: &zbus::Connection) -> Vec<Call> {
    interface(service).await.get().await.calls.clone()
}

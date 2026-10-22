//! Media sessions over MPRIS 2.2, the D-Bus interfaces that browsers and media players serve on
//! the session bus (<https://specifications.freedesktop.org/mpris-spec/latest/>): on Linux,
//! what SMTC gives `eileen-apps-win` on Windows (BUILD.md section 2.7; docs/LINUX.md
//! section 4). Eileen writes the client itself: the `mpris-server` crate is MPL-2.0, which
//! `deny.toml` does not allow.
//!
//! [`MprisSessions`] serves the `MediaSessions` trait (`eileen-platform`, on the unmerged base
//! crates) method for method, and each method's doc gives its rules:
//!
//! - `sessions`: [`MprisSessions::sessions`], each player as a `MediaState`.
//! - `control`: [`MprisSessions::control`], each `MediaAction` as a `Player` method call.
//! - `subscribe`: [`MprisSessions::subscribe`], which opens a [`MprisWatch`]: a stream the caller
//!   polls, so nothing runs in the background, of each player's state and then each change.
//!
//! # Linux sources
//!
//! - The players: every bus name that starts with `org.mpris.MediaPlayer2.`, from
//!   `org.freedesktop.DBus.ListNames`. Each is called through the unique name that owns it
//!   (`eileen_dbus_linux::name_owner`), so a call never starts a player through D-Bus
//!   activation, as one to a well-known name could right after the player quits.
//! - Each player's object `/org/mpris/MediaPlayer2`: the root interface `org.mpris.MediaPlayer2`
//!   for its name and `org.mpris.MediaPlayer2.Player` for playback, each read whole with
//!   `org.freedesktop.DBus.Properties.GetAll`, and the `Player` interface's methods
//!   (<https://specifications.freedesktop.org/mpris-spec/latest/Player_Interface.html>).
//! - The changes: `PropertiesChanged` and `Seeked` from the players, and `NameOwnerChanged`
//!   for their names (the `watch` module).
//!
//! Players differ from the specification in small ways, and the reads take each difference
//! they can use (the `value` module). A player's text, its app's name included, is untrusted:
//! it stays `Untrusted` inside this crate, it is sanitized before it enters a `MediaState`, and
//! errors never carry it.

mod value;
mod watch;

use eileen_contracts::desktop::MediaState;
use eileen_contracts::ids::Untrusted;
use eileen_contracts::tools::apps::MediaAction;
use eileen_dbus_linux::{DbusError, name_owner};
use zbus::Connection;
use zbus::names::{OwnedUniqueName, WellKnownName};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use self::value::{Playback, Properties, app_name, can, media_state, micros_of};
pub use self::watch::{MprisEvent, MprisWatch};

/// The start of every MPRIS player's bus name.
const PREFIX: &str = "org.mpris.MediaPlayer2.";

/// The object every player serves its interfaces at.
const OBJECT: &str = "/org/mpris/MediaPlayer2";

/// The root interface, which names the player.
const ROOT: &str = "org.mpris.MediaPlayer2";

/// The playback interface.
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

/// The bus's own name, which is also its interface's.
const BUS: &str = "org.freedesktop.DBus";

/// The bus's object.
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// What the errors of a player's calls name.
const PLAYER_CALL: &str = "media player";

/// What the errors of the bus's name list name.
const NAME_LIST: &str = "bus name list";

/// Why every action but `Status` is refused when `CanControl` is false.
const NOT_CONTROLLABLE: &str = "the player cannot be controlled";

/// Why `Play`, and `Toggle` while paused, are refused.
const CANNOT_PLAY: &str = "the player cannot play";

/// Why `Pause`, and `Toggle` while playing, are refused.
const CANNOT_PAUSE: &str = "the player cannot pause";

/// Why `Seek` and `SeekBy` are refused.
const CANNOT_SEEK: &str = "the player cannot seek";

/// The `Player` interface's methods that [`MprisSessions::control`] calls.
#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2",
    gen_blocking = false
)]
trait Player {
    /// Starts or resumes playback.
    fn play(&self) -> zbus::Result<()>;
    /// Pauses playback.
    fn pause(&self) -> zbus::Result<()>;
    /// Pauses, or resumes or starts playback.
    fn play_pause(&self) -> zbus::Result<()>;
    /// Skips to the next track.
    fn next(&self) -> zbus::Result<()>;
    /// Skips to the previous track.
    fn previous(&self) -> zbus::Result<()>;
    /// Seeks by `offset` microseconds, back when negative.
    fn seek(&self, offset: i64) -> zbus::Result<()>;
    /// Seeks the track `track_id` to `position` microseconds.
    fn set_position(&self, track_id: &ObjectPath<'_>, position: i64) -> zbus::Result<()>;
}

/// The call an action makes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    /// None: the read was the action.
    Read,
    /// `Play`.
    Play,
    /// `Pause`.
    Pause,
    /// `PlayPause`.
    PlayPause,
    /// `Next`.
    Next,
    /// `Previous`.
    Previous,
    /// `Seek`, microseconds.
    Seek(i64),
    /// `SetPosition`: the track and the position, microseconds.
    SetPosition(OwnedObjectPath, i64),
}

/// The media sessions of one bus: `MediaSessions` over MPRIS. It takes the bus from its
/// caller, `eileen_dbus_linux::session_bus` in the product and a private bus in tests.
///
/// Every call waits at most the connection's method timeout: a player that never answers holds
/// a call until then, which fails as [`DbusError::DeadlineExceeded`]. The buses
/// `eileen_dbus_linux` opens have one (`METHOD_TIMEOUT`, 5 s); on a connection made without
/// one, such a player holds the call for good.
#[derive(Debug, Clone)]
pub struct MprisSessions {
    conn: Connection,
}

impl MprisSessions {
    /// The sessions on `conn`'s bus.
    #[must_use]
    pub fn new(conn: &Connection) -> Self {
        Self { conn: conn.clone() }
    }

    /// Every player on the bus, sorted by bus name (`MediaSessions::sessions`). Each player is
    /// one `MediaState`:
    ///
    /// - `session`: the bus name, such as `org.mpris.MediaPlayer2.vlc`.
    /// - `app`: the root interface's `Identity`, else its `DesktopEntry`, else the player's part
    ///   of the bus name (`vlc` in `org.mpris.MediaPlayer2.vlc.instance7389`).
    /// - `title`: `xesam:title`, empty when the player has none.
    /// - `artist`: the first entry of `xesam:artist` that is not blank once sanitized.
    /// - `position_s`: `Position`, microseconds as seconds; unknown when the player does not
    ///   report it or reports a negative one.
    /// - `duration_s`: `mpris:length`, microseconds as seconds; unknown unless above zero.
    /// - `playing`: `PlaybackStatus` is `Playing`.
    ///
    /// Text is sanitized and kept to 512 characters. Odd values are read where they can be: a
    /// list of strings sent as one string, a number of any D-Bus number type where the
    /// specification gives a signed 64-bit one, a value inside a second variant. A property
    /// that is missing or of an unusable type is unknown.
    ///
    /// A player that does not answer, has no `Player` interface or sends something other than
    /// a property map is left out, so one broken player never hides the others; only a failure
    /// of the bus itself is an error.
    pub async fn sessions(&self) -> Result<Vec<MediaState>, DbusError> {
        let mut sessions = Vec::new();
        for (name, owner) in players(&self.conn).await? {
            if let Ok((_, state)) = read(self.conn.clone(), name, owner, None).await {
                sessions.push(state);
            }
        }
        Ok(sessions)
    }

    /// Sends `action` to the player `session`, a bus name from [`Self::sessions`]
    /// (`MediaSessions::control`). It reads the player's `Player` properties, then calls the
    /// method the action names:
    ///
    /// | `MediaAction` | Call | Refused when false |
    /// |---|---|---|
    /// | `Status` | none: the read itself | |
    /// | `Play` | `Play` | `CanControl`, `CanPlay` |
    /// | `Pause` | `Pause` | `CanControl`, `CanPause` |
    /// | `Toggle` | `PlayPause` | `CanControl`; `CanPause` while playing, else `CanPlay` |
    /// | `Next` | `Next` | `CanControl`, `CanGoNext` |
    /// | `Previous` | `Previous` | `CanControl`, `CanGoPrevious` |
    /// | `SeekBy` | `Seek` with the offset | `CanControl`, `CanSeek` |
    /// | `Seek` | `SetPosition` with `mpris:trackid` | `CanControl`, `CanSeek` |
    ///
    /// - A refusal is [`DbusError::Refused`] and makes no call. The specification makes every
    ///   other `Can` property false when `CanControl` is, and has the `Player` methods do
    ///   nothing then, so `CanControl` refuses every action but `Status`.
    /// - A `Can` property the player leaves out, or sends as another type, refuses nothing: the
    ///   call is made and the player decides.
    /// - `Toggle` on a player that is not playing and cannot pause calls `Play`, since the
    ///   specification has `PlayPause` raise an error when `CanPause` is false.
    /// - `Seek` to a time before the start, or past `mpris:length` when the length is known,
    ///   is [`DbusError::Invalid`], since the specification has the player ignore both, and so
    ///   is a time that is not a finite number. Without a track id (and `SetPosition` never
    ///   takes the no-track path), it seeks by the distance from `Position`; with neither, it
    ///   is refused.
    /// - A player that is not on the bus is [`DbusError::Unavailable`]; a session that is not
    ///   an MPRIS player's bus name is [`DbusError::Invalid`].
    ///
    /// It returns once the player has answered. That the player acted shows in the next change
    /// a [`MprisWatch`] reports, as an action is confirmed on Windows (BUILD.md section 2.7).
    pub async fn control(&self, session: &str, action: &MediaAction) -> Result<(), DbusError> {
        if !session.starts_with(PREFIX) || WellKnownName::try_from(session).is_err() {
            return Err(DbusError::Invalid {
                what: "media session",
                detail: "not an MPRIS player's bus name".to_owned(),
            });
        }
        let owner = name_owner(&self.conn, session)
            .await?
            .ok_or(DbusError::Unavailable { what: PLAYER_CALL })?;
        let player = properties(&self.conn, &owner, PLAYER).await?;
        let call = plan(action, &player)?;
        let failed = |error: zbus::Error| DbusError::from_zbus(PLAYER_CALL, &error);
        let proxy = PlayerProxy::builder(&self.conn)
            .destination(owner)
            .map_err(failed)?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(failed)?;
        match call {
            Call::Read => Ok(()),
            Call::Play => proxy.play().await,
            Call::Pause => proxy.pause().await,
            Call::PlayPause => proxy.play_pause().await,
            Call::Next => proxy.next().await,
            Call::Previous => proxy.previous().await,
            Call::Seek(offset) => proxy.seek(offset).await,
            Call::SetPosition(track, position) => proxy.set_position(&track, position).await,
        }
        .map_err(failed)
    }

    /// Opens a [`MprisWatch`] on every player (`MediaSessions::subscribe`): each player's state,
    /// then each change. It fails only when the watch's match rules cannot be added.
    pub async fn subscribe(&self) -> Result<MprisWatch, DbusError> {
        MprisWatch::open(&self.conn).await
    }
}

/// The call `action` makes on a player whose `Player` properties are `player`, or why it is
/// refused.
fn plan(action: &MediaAction, player: &Properties) -> Result<Call, DbusError> {
    let allow = |what: &'static str, property: &str, reason: &'static str| {
        if !can(player, "CanControl") {
            Err(DbusError::Refused { what, reason: NOT_CONTROLLABLE })
        } else if can(player, property) {
            Ok(())
        } else {
            Err(DbusError::Refused { what, reason })
        }
    };
    match *action {
        MediaAction::Status => Ok(Call::Read),
        MediaAction::Play => allow("playing", "CanPlay", CANNOT_PLAY).map(|()| Call::Play),
        MediaAction::Pause => allow("pausing", "CanPause", CANNOT_PAUSE).map(|()| Call::Pause),
        MediaAction::Toggle if Playback::read(player).playing => {
            allow("pausing", "CanPause", CANNOT_PAUSE).map(|()| Call::PlayPause)
        }
        MediaAction::Toggle => allow("playing", "CanPlay", CANNOT_PLAY)
            .map(|()| if can(player, "CanPause") { Call::PlayPause } else { Call::Play }),
        MediaAction::Next => allow("the next track", "CanGoNext", "the player has no next track")
            .map(|()| Call::Next),
        MediaAction::Previous => {
            allow("the previous track", "CanGoPrevious", "the player has no previous track")
                .map(|()| Call::Previous)
        }
        MediaAction::SeekBy { seconds } => {
            allow("seeking", "CanSeek", CANNOT_SEEK)?;
            micros_of(seconds).map(Call::Seek).ok_or_else(|| DbusError::Invalid {
                what: "seek offset",
                detail: "not a finite number of seconds".to_owned(),
            })
        }
        MediaAction::Seek { seconds } => {
            allow("seeking", "CanSeek", CANNOT_SEEK)?;
            seek_to(seconds, &Playback::read(player))
        }
    }
}

/// The call that seeks to `seconds` from the start of the current track.
fn seek_to(seconds: f64, playback: &Playback) -> Result<Call, DbusError> {
    let invalid =
        |detail: &str| DbusError::Invalid { what: "seek position", detail: detail.to_owned() };
    let target = micros_of(seconds)
        .filter(|target| *target >= 0)
        .ok_or_else(|| invalid("not a time in the track"))?;
    if playback.length_us.is_some_and(|length| length > 0 && target > length) {
        return Err(invalid("past the end of the track"));
    }
    match (&playback.track, playback.position_us) {
        (Some(track), _) => Ok(Call::SetPosition(track.clone(), target)),
        (None, Some(position)) => Ok(Call::Seek(target.saturating_sub(position))),
        (None, None) => Err(DbusError::Refused {
            what: "seeking",
            reason: "the player gives no track id and no position",
        }),
    }
}

/// Every player on the bus with the unique name that owns it, sorted by bus name. A player
/// that leaves before its owner is read is left out.
async fn players(conn: &Connection) -> Result<Vec<(String, OwnedUniqueName)>, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(NAME_LIST, &error);
    let reply =
        conn.call_method(Some(BUS), BUS_PATH, Some(BUS), "ListNames", &()).await.map_err(failed)?;
    let mut names: Vec<String> = reply.body().deserialize().map_err(failed)?;
    names.retain(|name| name.starts_with(PREFIX) && WellKnownName::try_from(name.as_str()).is_ok());
    names.sort();
    let mut players = Vec::with_capacity(names.len());
    for name in names {
        // None: the player left between the two calls.
        if let Some(owner) = name_owner(conn, &name).await? {
            players.push((name, owner));
        }
    }
    Ok(players)
}

/// One interface's properties on the player `owner`.
async fn properties(
    conn: &Connection,
    owner: &OwnedUniqueName,
    interface: &str,
) -> Result<Properties, DbusError> {
    let failed = |error: zbus::Error| DbusError::from_zbus(PLAYER_CALL, &error);
    let reply = conn
        .call_method(
            Some(owner.as_str()),
            OBJECT,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(interface,),
        )
        .await
        .map_err(failed)?;
    reply.body().deserialize().map_err(failed)
}

/// The player `name`, read from its owner: its app's name and its state from its `Player`
/// properties. The app's name is `app` when the caller knows it already, and is read from the
/// root interface only when it does not; a root interface that cannot be read leaves the bus
/// name to name the app. It owns its arguments, so the watch can keep it while it waits.
async fn read(
    conn: Connection,
    name: String,
    owner: OwnedUniqueName,
    app: Option<Untrusted<String>>,
) -> Result<(Untrusted<String>, MediaState), DbusError> {
    let player = properties(&conn, &owner, PLAYER).await?;
    let app = match app {
        Some(app) => app,
        None => app_name(&name, &properties(&conn, &owner, ROOT).await.unwrap_or_default()),
    };
    let state = media_state(&name, app.clone(), &player);
    Ok((app, state))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::tools::apps::MediaAction;
    use eileen_dbus_linux::DbusError;
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

    use super::value::Properties;
    use super::{Call, plan};

    fn player(entries: Vec<(&str, Value<'static>)>) -> Properties {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.try_into_owned().unwrap()))
            .collect()
    }

    fn track(length_us: i64) -> Value<'static> {
        Value::from(std::collections::HashMap::from([
            (
                "mpris:trackid",
                Value::ObjectPath(ObjectPath::from_static_str("/org/eileen/Track/1").unwrap()),
            ),
            ("mpris:length", Value::I64(length_us)),
        ]))
    }

    #[test]
    fn each_action_makes_its_call() {
        let full = player(vec![
            ("PlaybackStatus", Value::from("Playing")),
            ("Metadata", track(240_000_000)),
        ]);
        let id = OwnedObjectPath::try_from("/org/eileen/Track/1").unwrap();
        let cases = [
            (MediaAction::Status, Call::Read),
            (MediaAction::Play, Call::Play),
            (MediaAction::Pause, Call::Pause),
            (MediaAction::Toggle, Call::PlayPause),
            (MediaAction::Next, Call::Next),
            (MediaAction::Previous, Call::Previous),
            (MediaAction::SeekBy { seconds: -5.0 }, Call::Seek(-5_000_000)),
            (MediaAction::Seek { seconds: 60.0 }, Call::SetPosition(id, 60_000_000)),
        ];
        for (action, call) in cases {
            assert_eq!(plan(&action, &full).unwrap(), call, "{action:?}");
        }
    }

    #[test]
    fn a_toggle_that_cannot_pause_plays() {
        let paused = player(vec![
            ("PlaybackStatus", Value::from("Paused")),
            ("CanPause", Value::Bool(false)),
        ]);
        assert_eq!(plan(&MediaAction::Toggle, &paused).unwrap(), Call::Play, "Play, not PlayPause");
        let playing = player(vec![
            ("PlaybackStatus", Value::from("Playing")),
            ("CanPause", Value::Bool(false)),
        ]);
        assert!(
            matches!(plan(&MediaAction::Toggle, &playing), Err(DbusError::Refused { .. })),
            "playing and cannot pause"
        );
    }

    #[test]
    fn a_seek_without_a_track_id_moves_from_the_position() {
        let no_track = player(vec![("Position", Value::I64(20_000_000))]);
        assert_eq!(
            plan(&MediaAction::Seek { seconds: 50.0 }, &no_track).unwrap(),
            Call::Seek(30_000_000),
            "by the distance"
        );
        assert!(
            matches!(
                plan(&MediaAction::Seek { seconds: 50.0 }, &player(vec![])),
                Err(DbusError::Refused { .. })
            ),
            "no track id and no position"
        );
    }

    #[test]
    fn a_seek_outside_the_track_is_invalid() {
        let full = player(vec![("Metadata", track(240_000_000))]);
        for seconds in [-1.0, f64::NAN, 240.5] {
            assert!(
                matches!(
                    plan(&MediaAction::Seek { seconds }, &full),
                    Err(DbusError::Invalid { .. })
                ),
                "{seconds}"
            );
        }
        assert!(plan(&MediaAction::Seek { seconds: 240.0 }, &full).is_ok(), "the very end");
        assert!(
            matches!(
                plan(&MediaAction::SeekBy { seconds: f64::INFINITY }, &full),
                Err(DbusError::Invalid { .. })
            ),
            "an offset that is not finite"
        );
        let unknown = player(vec![("Metadata", track(0))]);
        assert!(plan(&MediaAction::Seek { seconds: 999.0 }, &unknown).is_ok(), "no length known");
    }
}

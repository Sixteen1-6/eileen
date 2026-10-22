//! Reading a player's properties leniently. Players send other types than the specification
//! gives (`xesam:artist` as one string, `mpris:length` as an unsigned or floating number),
//! wrap values in a second variant, and leave optional properties out; each reader takes what
//! it can use and leaves the rest unknown. The types the specification gives are in its
//! metadata map
//! (<https://specifications.freedesktop.org/mpris-spec/latest/Track_List_Interface.html#Mapping:Metadata_Map>)
//! and its `Player` interface
//! (<https://specifications.freedesktop.org/mpris-spec/latest/Player_Interface.html>).
//!
//! Text leaves these readers `Untrusted`, sanitized (`eileen_contracts::desktop::sanitize`) and
//! kept to 512 characters: a title, an artist and an app's name.

use std::collections::HashMap;

use eileen_contracts::desktop::{MediaState, sanitize};
use eileen_contracts::ids::Untrusted;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::PREFIX;

/// The most characters kept of a title, an artist or an app's name.
const MAX_TEXT_CHARS: usize = 512;

/// The track id that means no track (the specification's `Track_Id`), which `SetPosition`
/// does not take.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// How many variant layers a reader looks through.
const MAX_VARIANT_DEPTH: usize = 4;

/// One interface's properties, as `org.freedesktop.DBus.Properties.GetAll` returns them.
pub(super) type Properties = HashMap<String, OwnedValue>;

/// What the `Player` interface's properties say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Playback {
    /// `PlaybackStatus` is `Playing`.
    pub(super) playing: bool,
    /// `xesam:title`, sanitized.
    pub(super) title: Option<Untrusted<String>>,
    /// The first entry of `xesam:artist` that is not blank once sanitized, sanitized.
    pub(super) artist: Option<Untrusted<String>>,
    /// `mpris:length`, microseconds.
    pub(super) length_us: Option<i64>,
    /// `Position`, microseconds.
    pub(super) position_us: Option<i64>,
    /// `mpris:trackid`, when it names a track.
    pub(super) track: Option<OwnedObjectPath>,
}

impl Playback {
    /// Reads the `Player` interface's properties.
    pub(super) fn read(player: &Properties) -> Self {
        let mut playback = Self {
            playing: player
                .get("PlaybackStatus")
                .and_then(|status| text(status))
                .is_some_and(|status| status.eq_ignore_ascii_case("Playing")),
            position_us: player.get("Position").and_then(|position| micros(position)),
            ..Self::default()
        };
        let metadata = player.get("Metadata").map(|metadata| unwrapped(metadata));
        if let Some(Value::Dict(entries)) = metadata {
            for (key, value) in entries.iter() {
                let Some(key) = text(key) else {
                    continue;
                };
                match key {
                    "xesam:title" => {
                        playback.title = text(value).map(|title| sanitize(title, MAX_TEXT_CHARS));
                    }
                    "xesam:artist" => playback.artist = first_text(value),
                    "mpris:length" => playback.length_us = micros(value),
                    "mpris:trackid" => playback.track = track_id(value),
                    _ => {}
                }
            }
        }
        playback
    }
}

/// The contract's state of the player called `session`: `app` is its name, `player` its
/// `Player` properties. A missing title is empty, a negative position is unknown, and a length
/// that is not above zero is unknown. The app's name leaves `Untrusted` here, since the
/// contract's `app` is plain text.
pub(super) fn media_state(
    session: &str,
    app: Untrusted<String>,
    player: &Properties,
) -> MediaState {
    let playback = Playback::read(player);
    MediaState {
        session: session.to_owned(),
        app: app.into_inner(),
        title: playback.title.unwrap_or_default(),
        artist: playback.artist,
        position_s: playback.position_us.filter(|position| *position >= 0).map(seconds),
        duration_s: playback.length_us.filter(|length| *length > 0).map(seconds),
        playing: playback.playing,
    }
}

/// The app that owns the player called `session`, from its root interface's properties `root`:
/// its `Identity`, else its `DesktopEntry`, passing over one that is blank once sanitized, else
/// the player's part of the bus name.
pub(super) fn app_name(session: &str, root: &Properties) -> Untrusted<String> {
    ["Identity", "DesktopEntry"]
        .iter()
        .filter_map(|key| root.get(*key).and_then(|name| text(name)))
        .map(|name| sanitize(name, MAX_TEXT_CHARS))
        .find(|name| !name.get().trim().is_empty())
        .unwrap_or_else(|| sanitize(bus_name_part(session), MAX_TEXT_CHARS))
}

/// The player's own part of its bus name: the last element after `org.mpris.MediaPlayer2.`,
/// once an instance suffix (`.instance7389`, Firefox's `.instance_1_84`) is dropped. So
/// `org.mpris.MediaPlayer2.vlc.instance7389` gives `vlc`, and
/// `org.mpris.MediaPlayer2.io.github.celluloid_player.Celluloid` gives `Celluloid`.
fn bus_name_part(session: &str) -> &str {
    let rest = session.strip_prefix(PREFIX).unwrap_or(session);
    let rest = match rest.rsplit_once('.') {
        Some((player, instance)) if instance.starts_with("instance") => player,
        _ => rest,
    };
    rest.rsplit('.').next().unwrap_or(rest)
}

/// Whether the `Can` property `name` allows its action. A property the player leaves out, or
/// sends as another type, allows it: the call is made and the player decides.
pub(super) fn can(player: &Properties, name: &str) -> bool {
    player.get(name).and_then(|value| flag(value)).unwrap_or(true)
}

/// Microseconds as seconds.
#[expect(clippy::cast_precision_loss, reason = "exact below 2^53 microseconds, which is 285 years")]
fn seconds(micros: i64) -> f64 {
    micros as f64 / 1e6
}

/// Seconds as whole microseconds; `None` for a value that is not finite.
pub(super) fn micros_of(seconds: f64) -> Option<i64> {
    whole(seconds * 1e6)
}

/// A finite value rounded to a whole number.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a finite, rounded value; the cast saturates beyond the range, which no track reaches"
)]
fn whole(value: f64) -> Option<i64> {
    value.is_finite().then(|| value.round() as i64)
}

/// The value inside its variant layers.
fn unwrapped<'v>(value: &'v Value<'v>) -> &'v Value<'v> {
    let mut value = value;
    for _ in 0..MAX_VARIANT_DEPTH {
        let Value::Value(inner) = value else {
            break;
        };
        value = inner;
    }
    value
}

/// A string value.
fn text<'v>(value: &'v Value<'v>) -> Option<&'v str> {
    if let Value::Str(text) = unwrapped(value) { Some(text.as_str()) } else { None }
}

/// A boolean value.
fn flag(value: &Value<'_>) -> Option<bool> {
    if let Value::Bool(flag) = unwrapped(value) { Some(*flag) } else { None }
}

/// A whole number of microseconds, from any of D-Bus's number types: `x` as the specification
/// gives `mpris:length` and `Position`, but also `t`, `d` and the narrower integers.
#[expect(clippy::wildcard_enum_match_arm, reason = "every other D-Bus type is not a number")]
fn micros(value: &Value<'_>) -> Option<i64> {
    match unwrapped(value) {
        Value::I64(number) => Some(*number),
        Value::U64(number) => i64::try_from(*number).ok(),
        Value::F64(number) => whole(*number),
        Value::I32(number) => Some(i64::from(*number)),
        Value::U32(number) => Some(i64::from(*number)),
        Value::I16(number) => Some(i64::from(*number)),
        Value::U16(number) => Some(i64::from(*number)),
        Value::U8(number) => Some(i64::from(*number)),
        _ => None,
    }
}

/// The first string of a list of strings that is not blank once sanitized, sanitized; or a lone
/// string where a player sends one in place of the list. An entry that sanitizing empties, such
/// as a lone bidirectional mark, is passed over.
fn first_text(value: &Value<'_>) -> Option<Untrusted<String>> {
    let value = unwrapped(value);
    let items =
        if let Value::Array(items) = value { items.inner() } else { std::slice::from_ref(value) };
    items
        .iter()
        .filter_map(|item| text(item))
        .map(|item| sanitize(item, MAX_TEXT_CHARS))
        .find(|item| !item.get().trim().is_empty())
}

/// The track an `mpris:trackid` names: an object path, or a string that is one, as some
/// players send it; never the no-track path.
fn track_id(value: &Value<'_>) -> Option<OwnedObjectPath> {
    let value = unwrapped(value);
    let path = if let Value::ObjectPath(path) = value { path.as_str() } else { text(value)? };
    if path == NO_TRACK {
        return None;
    }
    ObjectPath::try_from(path).ok().map(OwnedObjectPath::from)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use eileen_contracts::ids::Untrusted;
    use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

    use super::{
        Playback, Properties, app_name, bus_name_part, can, first_text, media_state, micros,
        micros_of, seconds, track_id,
    };

    fn owned(value: Value<'static>) -> OwnedValue {
        value.try_into_owned().unwrap()
    }

    fn properties(entries: Vec<(&str, Value<'static>)>) -> Properties {
        entries.into_iter().map(|(key, value)| (key.to_owned(), owned(value))).collect()
    }

    fn nested(value: Value<'static>) -> Value<'static> {
        Value::Value(Box::new(value))
    }

    /// `text` as the readers give it.
    fn untrusted(text: &str) -> Untrusted<String> {
        Untrusted::new(text.to_owned())
    }

    #[test]
    fn numbers_of_every_type_are_microseconds() {
        assert_eq!(micros(&Value::I64(-5)), Some(-5), "x, the specification's type");
        assert_eq!(micros(&Value::U64(90_000_000)), Some(90_000_000), "t");
        assert_eq!(micros(&Value::U64(u64::MAX)), None, "t beyond x");
        assert_eq!(micros(&Value::F64(30.6)), Some(31), "d, rounded");
        assert_eq!(micros(&Value::F64(f64::NAN)), None, "d, not a number");
        assert_eq!(micros(&Value::I32(-7)), Some(-7), "i");
        assert_eq!(micros(&Value::U32(7)), Some(7), "u");
        assert_eq!(micros(&Value::U8(7)), Some(7), "y");
        assert_eq!(micros(&nested(Value::I64(12))), Some(12), "inside a second variant");
        assert_eq!(micros(&Value::Str(Str::from("12"))), None, "a string is not a number");
        assert_eq!(micros(&Value::Bool(true)), None, "nor is a boolean");
    }

    #[test]
    fn seconds_and_microseconds_convert_both_ways() {
        assert!((seconds(42_500_000) - 42.5).abs() < 1e-9, "to seconds");
        assert_eq!(micros_of(1.5), Some(1_500_000), "to microseconds");
        assert_eq!(micros_of(-0.25), Some(-250_000), "backwards");
        assert_eq!(micros_of(f64::INFINITY), None, "not finite");
        assert_eq!(micros_of(1e30), Some(i64::MAX), "saturated");
    }

    #[test]
    fn the_artist_is_the_first_name_of_a_list_or_a_lone_string() {
        let list = Value::from(vec!["", "Ada", "Grace"]);
        assert_eq!(first_text(&list), Some(untrusted("Ada")), "the first non-empty entry");
        assert_eq!(first_text(&Value::from("Solo")), Some(untrusted("Solo")), "a lone string");
        assert_eq!(first_text(&Value::from(Vec::<&str>::new())), None, "an empty list");
        assert_eq!(first_text(&Value::from("  ")), None, "blank");
        assert_eq!(first_text(&Value::I64(3)), None, "not text");
    }

    #[test]
    fn an_artist_that_sanitizing_empties_is_passed_over() {
        let marked = Value::from(vec!["\u{200E}", "\u{FEFF} \u{0007}", "Ada"]);
        assert_eq!(
            first_text(&marked),
            Some(untrusted("Ada")),
            "marks and controls alone are blank"
        );
        assert_eq!(
            first_text(&Value::from("\u{200F}Grace\u{202E}")),
            Some(untrusted("Grace")),
            "the entry chosen is sanitized"
        );
        assert_eq!(first_text(&Value::from("\u{200E}")), None, "a lone string of marks");
        let metadata =
            Value::from(HashMap::from([("xesam:artist", Value::from(vec!["\u{200E}", "Ada"]))]));
        let player = properties(vec![("Metadata", metadata)]);
        let state = media_state("org.mpris.MediaPlayer2.web", untrusted("Web"), &player);
        assert_eq!(state.artist, Some(untrusted("Ada")), "the state's artist");
    }

    #[test]
    fn a_players_text_is_sanitized_and_kept_to_512_characters() {
        let long = "a".repeat(600);
        let metadata = Value::from(HashMap::from([
            ("xesam:title", Value::from("\u{202E}Deep\u{0007} Blue")),
            ("xesam:artist", Value::from(long.clone())),
        ]));
        let playback = Playback::read(&properties(vec![("Metadata", metadata)]));
        assert_eq!(playback.title, Some(untrusted("Deep Blue")), "the title, without its controls");
        assert_eq!(playback.artist, Some(untrusted(&"a".repeat(512))), "the artist, cut to 512");
        let root = properties(vec![("Identity", Value::from(long))]);
        let app = app_name("org.mpris.MediaPlayer2.long", &root);
        assert_eq!(app.get().chars().count(), 512, "the app's name, cut to 512");
    }

    #[test]
    fn a_track_id_is_an_object_path_but_never_no_track() {
        let path =
            |text: &'static str| Value::ObjectPath(ObjectPath::from_static_str(text).unwrap());
        assert_eq!(
            track_id(&path("/org/eileen/Track/1")).as_deref().map(ObjectPath::as_str),
            Some("/org/eileen/Track/1"),
            "o, the specification's type"
        );
        assert_eq!(
            track_id(&Value::from("/com/example/track/7")).as_deref().map(ObjectPath::as_str),
            Some("/com/example/track/7"),
            "a string that is a path"
        );
        assert_eq!(track_id(&Value::from("example:track:7")), None, "a string that is not");
        assert_eq!(track_id(&path("/org/mpris/MediaPlayer2/TrackList/NoTrack")), None, "no track");
    }

    #[test]
    fn the_bus_name_gives_the_players_own_part() {
        assert_eq!(bus_name_part("org.mpris.MediaPlayer2.vlc"), "vlc", "plain");
        assert_eq!(bus_name_part("org.mpris.MediaPlayer2.vlc.instance7389"), "vlc", "instance");
        assert_eq!(
            bus_name_part("org.mpris.MediaPlayer2.firefox.instance_1_84"),
            "firefox",
            "Firefox's instance"
        );
        assert_eq!(
            bus_name_part("org.mpris.MediaPlayer2.io.github.celluloid_player.Celluloid"),
            "Celluloid",
            "an app id"
        );
    }

    #[test]
    fn the_app_is_the_identity_then_the_desktop_entry_then_the_bus_name() {
        let name = "org.mpris.MediaPlayer2.vlc.instance7389";
        let both = properties(vec![
            ("Identity", Value::from("VLC")),
            ("DesktopEntry", Value::from("vlc")),
        ]);
        assert_eq!(app_name(name, &both), untrusted("VLC"), "Identity first");
        let entry =
            properties(vec![("Identity", Value::from(" ")), ("DesktopEntry", Value::from("vlc"))]);
        assert_eq!(app_name(name, &entry), untrusted("vlc"), "a blank Identity is passed over");
        let odd = properties(vec![("Identity", Value::I64(4))]);
        assert_eq!(app_name(name, &odd), untrusted("vlc"), "an Identity that is not text");
        let hostile = properties(vec![("Identity", Value::from("V\u{202E}LC\u{0007}"))]);
        assert_eq!(app_name(name, &hostile), untrusted("VLC"), "sanitized");
    }

    #[test]
    fn a_can_property_left_out_or_mistyped_allows_its_action() {
        let player =
            properties(vec![("CanPlay", Value::Bool(false)), ("CanSeek", Value::from("no"))]);
        assert!(!can(&player, "CanPlay"), "false refuses");
        assert!(can(&player, "CanSeek"), "a string is not a boolean");
        assert!(can(&player, "CanPause"), "left out");
    }

    #[test]
    fn metadata_in_odd_shapes_is_still_read() {
        let metadata = Value::from(HashMap::from([
            ("xesam:title", nested(Value::from("Deep"))),
            ("xesam:artist", Value::from("Solo")),
            ("mpris:length", Value::F64(90e6)),
            ("mpris:trackid", Value::from("not a path")),
            ("xesam:album", Value::from("Ignored")),
        ]));
        let player = properties(vec![
            ("PlaybackStatus", Value::from("playing")),
            ("Metadata", nested(metadata)),
            ("Position", Value::U64(3)),
        ]);
        let playback = Playback::read(&player);
        assert!(playback.playing, "the status, whatever its case");
        assert_eq!(playback.title, Some(untrusted("Deep")), "a title in a second variant");
        assert_eq!(playback.artist, Some(untrusted("Solo")), "one string");
        assert_eq!(playback.length_us, Some(90_000_000), "a floating length");
        assert_eq!(playback.position_us, Some(3), "an unsigned position");
        assert_eq!(playback.track, None, "a track id that is no path");
        let junk = properties(vec![("Metadata", Value::from("junk"))]);
        assert_eq!(Playback::read(&junk), Playback::default(), "metadata that is no map");
    }

    #[test]
    fn a_state_leaves_out_what_the_player_does_not_say() {
        let player = properties(vec![
            ("Position", Value::I64(-1)),
            ("Metadata", Value::from(HashMap::from([("mpris:length", Value::I64(0))]))),
        ]);
        let state = media_state("org.mpris.MediaPlayer2.quiet", untrusted("Quiet"), &player);
        assert_eq!(state.app, "Quiet", "the app's name, as the contract takes it");
        assert_eq!(state.title, untrusted(""), "no title is an empty one");
        assert_eq!(state.artist, None, "no artist");
        assert_eq!(state.position_s, None, "a negative position is unknown");
        assert_eq!(state.duration_s, None, "a length of zero is unknown");
        assert!(!state.playing, "no status is not playing");
    }
}

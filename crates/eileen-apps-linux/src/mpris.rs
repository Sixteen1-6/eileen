//! Media sessions over MPRIS 2.2, the D-Bus interfaces that browsers and media players serve on
//! the session bus (<https://specifications.freedesktop.org/mpris-spec/latest/>): on Linux,
//! what SMTC gives `eileen-apps-win` on Windows (BUILD.md section 2.7; docs/LINUX.md
//! section 4). They serve the `MediaSessions` trait (`eileen-platform`, on the unmerged base
//! crates). Eileen writes the client itself: the `mpris-server` crate is MPL-2.0, which
//! `deny.toml` does not allow.
//!
//! # Linux sources
//!
//! - Each player's object `/org/mpris/MediaPlayer2`: the root interface `org.mpris.MediaPlayer2`
//!   for its name and `org.mpris.MediaPlayer2.Player` for playback, each read whole with
//!   `org.freedesktop.DBus.Properties.GetAll`
//!   (<https://specifications.freedesktop.org/mpris-spec/latest/Player_Interface.html>).
//!
//! Players differ from the specification in small ways, and the reads take each difference
//! they can use (the `value` module). A player's text, its app's name included, is untrusted:
//! it stays `Untrusted` inside this crate, it is sanitized before it enters a `MediaState`, and
//! errors never carry it.

mod value;

/// The start of every MPRIS player's bus name.
const PREFIX: &str = "org.mpris.MediaPlayer2.";

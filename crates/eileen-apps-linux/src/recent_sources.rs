//! [`LinuxRecent`]: `eileen-platform`'s `RecentSources` over `recently-used.xbel`. `recent` is
//! [`RecentlyUsed::recent`], whose doc gives the rules, read with std file reads on the caller's
//! thread: no bus, so no bridge. Its [`FileError`](crate::FileError) becomes its `PlatformError`
//! twin, and each [`RecentItem`] the trait's.
//!
//! The trait's item keeps the app's name as a plain `String`, so the `Untrusted` mark the crate's
//! item gives it, as another app wrote it, ends here. The name is sanitized first
//! (`eileen_contracts::desktop::sanitize`, kept to [`APP_NAME_CHARS`] characters, as the MPRIS
//! readers keep a player's text), which removes the bidirectional and invisible formatting
//! characters and every control character but line breaks and tabs. A name is one line, so each
//! run of whitespace, those breaks and tabs included, then becomes one space, and the ends are
//! trimmed: what leaves holds no control, bidirectional or invisible formatting character, and a
//! name left empty is no name.

use eileen_contracts::desktop::sanitize;
use eileen_platform::{self as platform, PlatformError, RecentSources, RgbaImage, Subscription};

use crate::{RecentItem, RecentlyUsed};

/// How many characters of an app's name are kept.
const APP_NAME_CHARS: usize = 512;

/// What `watch` names as unavailable.
const WATCH: &str = "watching recent files";
/// What `thumbnail` names as unavailable.
const THUMBNAIL: &str = "a recent file's thumbnail";

/// The user's recent files: `RecentSources` on Linux, from one [`RecentlyUsed`] list.
///
/// - `recent` is [`RecentlyUsed::recent`]: newest first, each path once, a missing file
///   holding none. The read has no deadline, as the trait gives none and a file read cannot be
///   cancelled: a data folder on a network mount whose server hangs holds the call.
/// - `watch` is `Unavailable` until phase L2 watches the file with `inotify` (docs/LINUX.md
///   section 4).
/// - `thumbnail` is `Unavailable` until phase L2 reads the freedesktop thumbnail cache.
#[derive(Debug, Clone)]
pub struct LinuxRecent {
    list: RecentlyUsed,
}

impl LinuxRecent {
    /// The recent files of `list`: in the apps process, the session's,
    /// [`RecentlyUsed::for_session`].
    #[must_use]
    pub const fn new(list: RecentlyUsed) -> Self {
        Self { list }
    }
}

impl RecentSources for LinuxRecent {
    fn recent(&self, limit: usize) -> Result<Vec<platform::RecentItem>, PlatformError> {
        Ok(self.list.recent(limit)?.into_iter().map(to_platform).collect())
    }

    /// `Unavailable` until phase L2 brings `inotify`.
    fn watch(
        &self,
        _roots: &[String],
        _sink: Box<dyn Fn(platform::RecentItem) + Send + Sync>,
    ) -> Result<Subscription, PlatformError> {
        Err(PlatformError::Unavailable { what: WATCH })
    }

    /// `Unavailable` until phase L2 reads the thumbnail cache.
    fn thumbnail(
        &self,
        _item: &platform::RecentItem,
        _edge_px: u32,
    ) -> Result<Option<RgbaImage>, PlatformError> {
        Err(PlatformError::Unavailable { what: THUMBNAIL })
    }
}

/// The trait's item, field for field, with the app's name sanitized, on one line and unwrapped.
fn to_platform(item: RecentItem) -> platform::RecentItem {
    let app = item.app.and_then(|app| one_line(sanitize(app.get(), APP_NAME_CHARS).get()));
    platform::RecentItem { path: item.path, app, kind: item.kind, opened_at: item.opened_at }
}

/// `name` on one line: each run of whitespace one space, the ends trimmed; `None` when nothing
/// is left.
fn one_line(name: &str) -> Option<String> {
    let line = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

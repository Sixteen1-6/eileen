//! [`LinuxRecent`]: `eileen-platform`'s `RecentSources` over `recently-used.xbel`. `recent` is
//! [`RecentlyUsed::recent`], whose doc gives the rules, read with std file reads on the caller's
//! thread: no bus, so no bridge. Its [`FileError`](crate::FileError) becomes its `PlatformError`
//! twin, and each [`RecentItem`] the trait's.
//!
//! The trait's item keeps the app's name as a plain `String`, so the `Untrusted` mark the crate's
//! item gives it, as another app wrote it, ends here. The name is sanitized first
//! (`eileen_contracts::desktop::sanitize`, kept to [`APP_NAME_CHARS`] characters, as the MPRIS
//! readers keep a player's text), so what leaves holds no control, bidirectional or invisible
//! formatting character; a name that sanitizing empties is no name.

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

/// The trait's item, field for field, with the app's name sanitized and unwrapped.
fn to_platform(item: RecentItem) -> platform::RecentItem {
    let app = item
        .app
        .map(|app| sanitize(app.get(), APP_NAME_CHARS).into_inner())
        .filter(|app| !app.trim().is_empty());
    platform::RecentItem { path: item.path, app, kind: item.kind, opened_at: item.opened_at }
}

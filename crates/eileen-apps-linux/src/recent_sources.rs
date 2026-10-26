//! [`LinuxRecent`]: `eileen-platform`'s `RecentSources` over `recently-used.xbel`. `recent` is
//! [`RecentlyUsed::recent`], whose doc gives the rules, read with std file reads on a thread of
//! its own within the caller's `Deadline` (the `bounded` module's rules): no bus, so no bridge.
//! Its [`FileError`](crate::FileError) becomes its `PlatformError` twin, and each [`RecentItem`]
//! the trait's.
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
use eileen_platform::{
    self as platform, Deadline, PlatformError, RecentSources, RgbaImage, Subscription,
};
use tokio::runtime::Handle;

use crate::bounded;
use crate::{RecentItem, RecentlyUsed};

/// How many characters of an app's name are kept.
const APP_NAME_CHARS: usize = 512;

/// What `recent` names when its read passes its deadline.
const RECENT: &str = "recent files";
/// What `watch` names as unavailable.
const WATCH: &str = "watching recent files";
/// What `thumbnail` names as unavailable.
const THUMBNAIL: &str = "a recent file's thumbnail";

/// The user's recent files: `RecentSources` on Linux, from one [`RecentlyUsed`] list.
///
/// - `recent` is [`RecentlyUsed::recent`]: newest first, each path once, a missing file
///   holding none. A data folder on a network mount whose server hangs can hold the read, so it
///   runs on a thread of its own, or the pool that [`Self::reading_files_on`] gives: past the
///   caller's deadline the call answers `DeadlineExceeded` naming the recent files and leaves
///   the read to its thread.
/// - `watch` is `Unavailable` until phase L2 watches the file with `inotify` (docs/LINUX.md
///   section 4).
/// - `thumbnail` is `Unavailable` until phase L2 reads the freedesktop thumbnail cache.
///
/// `watch` and `thumbnail` open no file yet, so each answers at once, whatever its deadline.
#[derive(Debug, Clone)]
pub struct LinuxRecent {
    list: RecentlyUsed,
    /// The runtime whose blocking pool reads the list, if not a thread for each read.
    pool: Option<Handle>,
}

impl LinuxRecent {
    /// The recent files of `list`: in the apps process, the session's,
    /// [`RecentlyUsed::for_session`]. Each read of the list runs on a thread of its own.
    #[must_use]
    pub const fn new(list: RecentlyUsed) -> Self {
        Self { list, pool: None }
    }

    /// The same recent files, each read of the list run on `pool`'s blocking pool instead, as
    /// [`LinuxApps::reading_files_on`](crate::LinuxApps::reading_files_on) runs the reads of
    /// the index: it caps the threads that reads hung on a file system hold, at the cost of every
    /// later read waiting once they hold them all, until its call's deadline. Once the runtime
    /// has shut down, a read answers `Io` at once, unrun. Dropping it, as `#[tokio::main]` does
    /// on return, waits for each read it is running, a hung one included: end it with
    /// `Runtime::shutdown_background` or `shutdown_timeout`. Never the D-Bus bridge's runtime,
    /// whose blocking threads zbus needs to connect a bus.
    #[must_use]
    pub fn reading_files_on(mut self, pool: Handle) -> Self {
        self.pool = Some(pool);
        self
    }
}

impl RecentSources for LinuxRecent {
    fn recent(
        &self,
        limit: usize,
        deadline: Deadline,
    ) -> Result<Vec<platform::RecentItem>, PlatformError> {
        let list = self.list.clone();
        let items =
            bounded::read(RECENT, deadline, self.pool.as_ref(), move || list.recent(limit))??;
        Ok(items.into_iter().map(to_platform).collect())
    }

    /// `Unavailable` until phase L2 brings `inotify`.
    fn watch(
        &self,
        _roots: &[String],
        _sink: Box<dyn Fn(platform::RecentItem) + Send + Sync>,
        _deadline: Deadline,
    ) -> Result<Subscription, PlatformError> {
        Err(PlatformError::Unavailable { what: WATCH })
    }

    /// `Unavailable` until phase L2 reads the thumbnail cache.
    fn thumbnail(
        &self,
        _item: &platform::RecentItem,
        _edge_px: u32,
        _deadline: Deadline,
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

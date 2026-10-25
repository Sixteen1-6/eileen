//! Recent files from `recently-used.xbel`, the list that GTK apps and KDE's KIO keep under the
//! Desktop Bookmark Specification, version 0.8.5
//! (<https://www.freedesktop.org/wiki/Specifications/desktop-bookmark-spec/>), for
//! `RecentSources::recent` (BUILD.md section 3.4; docs/LINUX.md section 4). [`RecentItem`]
//! stands in for that trait's item type.
//!
//! [`RecentlyUsed::bookmarks`] reads the file's usable bookmarks in the file's order. A missing
//! file holds none, as GTK waits for its first write. A file that cannot be read, or is not a
//! bookmark file, is a [`FileError`] that holds no path and none of the file's text; GTK and KIO
//! replace such a file on their next write. The open never waits, as a FIFO's would for a
//! writer, and what is neither a file nor a folder, such as a FIFO or a device, is refused
//! before it is read. A file over 16 MiB is refused whole, once one byte past that limit is
//! read. The submodules hold the rules: `xbel` the file's layout and which bookmarks count,
//! `uri` the path, `time` the times, `kind` the kind of file.
//!
//! # The list
//!
//! [`RecentlyUsed::recent`] serves `RecentSources::recent`: the bookmarks newest first, as the
//! trait asks, cut to the limit. That departs from the specification, which asks that bookmarks
//! be shown in the file's order, so equal times keep the file's order, as close to the
//! specification as the trait allows. A path appears once, at its newest time. The
//! specification allows no two bookmarks with one URI, and `GLib` refuses a file that has them,
//! but two URIs can still name one path, as `file:///x` and `file://localhost/x` do. The files
//! are not checked to still exist. [`RecentlyUsed::for_session`] finds the file in the
//! session's data home, [`Session::data_home`], the folder the index searches first, so
//! `XDG_DATA_HOME` is read once.
//!
//! # Not here
//!
//! - `RecentSources::watch`: it needs `inotify` (docs/LINUX.md section 4), a new dependency.
//! - `RecentSources::thumbnail`: it reads the freedesktop thumbnail cache.
//! - KDE's `RecentDocuments` folder: KDE Frameworks 6's KIO writes only `recently-used.xbel`,
//!   and the last KDE Frameworks 5 releases write it beside the folder.

mod kind;
mod time;
mod uri;
mod xbel;

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, Read as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;

use eileen_contracts::ids::{Personal, Untrusted, WallTime};
use eileen_contracts::tools::apps::FileKind;

use crate::open::OPEN_FLAGS;
use crate::{FileError, Session};

/// The file's name in the user's data folder, and what its errors name.
const FILE_NAME: &str = "recently-used.xbel";

/// The most of the file that is read: 16 MiB. As measured with GTK 3.24.41, its recent manager
/// keeps the newest 1000 bookmarks, and 1000 with one app each took 0.71 MiB, or 12.25 MiB
/// with every path 4094 bytes long, nearly all of it escaped. A longer file, such as a sparse
/// one that takes no disk space, is refused whole: GTK adds new bookmarks at the end, so a cut
/// list would lose the newest.
const MAX_FILE_BYTES: u64 = 16 << 20;

/// A recently used file: the `RecentSources` trait's `RecentItem` (`eileen-platform`, on the
/// unmerged base crates) field for field, except that the app's name, which another app wrote,
/// is [`Untrusted`] here and unwrapped by the trait's implementation. This type gives way to the
/// trait's when that merges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentItem {
    /// Its path.
    pub path: Personal<String>,
    /// The app that opened it, when known: the name the bookmark file gives it.
    pub app: Option<Untrusted<String>>,
    /// What kind of file.
    pub kind: Option<FileKind>,
    /// When it was last opened.
    pub opened_at: WallTime,
}

/// The user's list of recently used files, `recently-used.xbel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentlyUsed {
    /// The file, under the user's home folder.
    path: Personal<PathBuf>,
}

impl RecentlyUsed {
    /// The list in the file at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: Personal::new(path.into()) }
    }

    /// The session's list, `recently-used.xbel` in its data home ([`Session::data_home`]):
    /// `$XDG_DATA_HOME`, else `$HOME/.local/share`. `None` when the session has no data home.
    #[must_use]
    pub fn for_session(session: &Session) -> Option<Self> {
        session.data_home.as_ref().map(|home| Self::at(home.join(FILE_NAME)))
    }

    /// The most recent `limit` files, newest first: `RecentSources::recent`. Equal times keep
    /// the file's order, and a path the file lists twice appears once, at its newest time.
    pub fn recent(&self, limit: usize) -> Result<Vec<RecentItem>, FileError> {
        let mut items = self.bookmarks()?;
        // A stable sort, so equal times keep the file's order.
        items.sort_by_key(|item| std::cmp::Reverse(item.opened_at));
        let mut listed = HashSet::new();
        Ok(items
            .into_iter()
            .filter(|item| listed.insert(item.path.get().clone()))
            .take(limit)
            .collect())
    }

    /// Every usable bookmark in the file, in the file's order: each local file that is not
    /// private and has a time, with its app and its kind. A path the file lists twice is here
    /// twice. A missing file, or one with no root element, has none. A FIFO or a device in the
    /// file's place is [`FileError::Invalid`], opened without waiting and not read, and so is a
    /// file over 16 MiB, of which 16 MiB and one byte are read.
    pub fn bookmarks(&self) -> Result<Vec<RecentItem>, FileError> {
        let opened = File::options().read(true).custom_flags(OPEN_FLAGS).open(self.path.get());
        let file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(FileError::io(FILE_NAME, &error)),
        };
        // A folder is left to fail at its first read, as it fails in GLib.
        let meta = file.metadata().map_err(|error| FileError::io(FILE_NAME, &error))?;
        if !meta.is_file() && !meta.is_dir() {
            return Err(FileError::Invalid { what: FILE_NAME, detail: "not a file".to_owned() });
        }
        // The read stops one byte past the limit, and a file that reaches that byte is refused,
        // even when the cut leaves what was read broken.
        let mut limited = BufReader::new(file.take(MAX_FILE_BYTES + 1));
        let read = xbel::read_bookmarks(&mut limited);
        if limited.get_ref().limit() == 0 {
            let detail = format!("larger than {} MiB", MAX_FILE_BYTES >> 20);
            return Err(FileError::Invalid { what: FILE_NAME, detail });
        }
        read
    }
}

//! Recent files from `recently-used.xbel`, the list that GTK apps and KDE's KIO keep under the
//! Desktop Bookmark Specification, version 0.8.5
//! (<https://www.freedesktop.org/wiki/Specifications/desktop-bookmark-spec/>), for
//! `RecentSources::recent` (BUILD.md section 3.4; docs/LINUX.md section 4).
//!
//! The submodules read the values a bookmark holds, and each one's doc states its rules: `uri`
//! the path its `href` names, `time` its times, `kind` the kind of file its MIME type names.
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

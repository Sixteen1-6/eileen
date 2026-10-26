//! The app index: every application the desktop's menu shows, from the desktop entries under
//! each data folder's `applications/` (BUILD.md section 3.4's `Apps::index`; docs/LINUX.md
//! section 4).
//!
//! Data folders are searched in precedence order, `XDG_DATA_HOME` first, and the first file
//! with a desktop file ID decides it: a user's copy replaces the system's, and a user's
//! `Hidden=true` copy removes the app.
//!
//! [`entry_for`] finds the indexed entry of the trait's [`AppEntry`], for `Apps::launch` and
//! `Apps::open_path` with an app.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use eileen_contracts::ids::Personal;

use crate::FileError;
use crate::entry::{DesktopEntry, Meaning, interpret};
use crate::keyfile::{locale_keys, messages_locale, parse};
use crate::open::OPEN_FLAGS;

/// How deep the index looks below an `applications/` folder (`kde4/kate.desktop` is one deep).
const MAX_DEPTH: usize = 8;

/// The most folders the index reads under one `applications/` folder, links followed: far more
/// than real trees take (a Wine prefix's menu takes tens), so a link to a big tree, such as `/`,
/// costs a bounded walk where a whole one could take hours.
const MAX_FOLDERS: usize = 1024;

/// The most of one desktop entry the index reads: 1 MiB, far more than real entries take, so a
/// huge file, such as a sparse one that takes no disk space, cannot fill memory. A longer file
/// is skipped whole, as a cut could change its `Exec` line.
const MAX_ENTRY_BYTES: u64 = 1 << 20;

/// The export folders of Flatpak and Snap, relative to the data folders they extend. The
/// session's `XDG_DATA_DIRS` normally lists them; the index adds any it lacks, so a runtime
/// started without the login profile still finds those apps.
const SYSTEM_EXPORTS: [&str; 2] = ["/var/lib/flatpak/exports/share", "/var/lib/snapd/desktop"];

/// How an app is installed, as the `Apps` trait's index tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    /// A packaged app (Flatpak, Snap), started by its ID.
    Packaged,
    /// A classic desktop app.
    Desktop,
}

/// One app in the index, field for field the `Apps` trait's `AppEntry` (`eileen-platform`, on
/// the unmerged base crates), which [`LinuxApps`](crate::LinuxApps) converts it into and back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// The desktop file ID.
    pub id: String,
    /// The name the menu shows.
    pub name: String,
    /// How it is installed.
    pub kind: AppKind,
    /// The desktop entry file, which may be under the user's home folder.
    pub path: Option<Personal<String>>,
}

impl From<&DesktopEntry> for AppEntry {
    fn from(entry: &DesktopEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.name.clone(),
            kind: entry.kind,
            path: Some(Personal::new(entry.path.get().to_string_lossy().into_owned())),
        }
    }
}

/// What the index and the recent files read from the session's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The user's data folder, `XDG_DATA_HOME`, else `$HOME/.local/share`; `None` when neither
    /// is an absolute path. It leads [`Self::data_dirs`] and holds the recent files.
    pub data_home: Option<PathBuf>,
    /// The data folders in precedence order.
    pub data_dirs: Vec<PathBuf>,
    /// The locale keys to try for localized values, most specific first.
    pub locales: Vec<String>,
    /// The desktop's names from `XDG_CURRENT_DESKTOP`, such as `ubuntu` and `GNOME`.
    pub desktops: Vec<String>,
    /// The `PATH` folders, for `TryExec`.
    pub path: Vec<PathBuf>,
}

impl Session {
    /// The session this process runs in.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var_os(name))
    }

    /// The session the variables describe: `XDG_DATA_HOME`, `XDG_DATA_DIRS`, `HOME`, `LANGUAGE`,
    /// `LC_ALL`, `LC_MESSAGES`, `LANG`, `XDG_CURRENT_DESKTOP` and `PATH`. As gettext does,
    /// `LANGUAGE`'s list comes before the messages locale, and is ignored under the C locale.
    #[must_use]
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
        let absolute = |value: PathBuf| value.is_absolute().then_some(value);
        let home = var("HOME").map(PathBuf::from).and_then(absolute);
        let data_home = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .and_then(absolute)
            .or_else(|| home.map(|home| home.join(".local").join("share")));
        let configured: Vec<PathBuf> = text("XDG_DATA_DIRS")
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned())
            .split(':')
            .map(PathBuf::from)
            .filter_map(absolute)
            .collect();
        let user_exports =
            data_home.as_ref().map(|home| home.join("flatpak").join("exports").join("share"));
        let mut data_dirs = Vec::new();
        let candidates = data_home
            .iter()
            .cloned()
            .chain(configured)
            .chain(user_exports)
            .chain(SYSTEM_EXPORTS.iter().map(PathBuf::from));
        for dir in candidates {
            if !data_dirs.contains(&dir) {
                data_dirs.push(dir);
            }
        }
        let locale = messages_locale(
            text("LC_ALL").as_deref(),
            text("LC_MESSAGES").as_deref(),
            text("LANG").as_deref(),
        );
        let mut locales: Vec<String> = Vec::new();
        if let Some(locale) = locale {
            let preferred = text("LANGUAGE").unwrap_or_default();
            let wanted = preferred.split(':').filter(|entry| !entry.is_empty()).map(str::to_owned);
            for key in wanted.chain([locale]).flat_map(|entry| locale_keys(&entry)) {
                if !locales.contains(&key) {
                    locales.push(key);
                }
            }
        }
        Self {
            data_home,
            data_dirs,
            locales,
            desktops: text("XDG_CURRENT_DESKTOP")
                .map(|names| {
                    names.split(':').filter(|name| !name.is_empty()).map(str::to_owned).collect()
                })
                .unwrap_or_default(),
            path: text("PATH")
                .map(|path| path.split(':').map(PathBuf::from).filter_map(absolute).collect())
                .unwrap_or_default(),
        }
    }
}

/// Every app the session's menu shows, sorted by name.
///
/// An entry is opened without waiting, where the open of a FIFO would wait for a writer. What
/// is not a regular file, such as a FIFO or a device, is skipped without being read, and a file
/// is read only up to 1 MiB and skipped when it is longer. Like a file that cannot be read, a
/// skipped one does not decide its desktop file ID.
///
/// Links to folders are followed, but a data folder's walk reads each folder once, however many
/// paths lead to it, such as links to `.` or to a folder above: its entries take their IDs from
/// the first of those paths in the walk's order. The walk reads at most 1024 folders, so a link
/// to a big tree, such as `/`, cannot keep it going: folders past the 1024th are not read.
#[must_use]
pub fn index(session: &Session) -> Vec<DesktopEntry> {
    let mut decided: BTreeSet<String> = BTreeSet::new();
    let mut apps: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for dir in &session.data_dirs {
        let packaged = is_export(dir);
        let mut files = Vec::new();
        collect(&dir.join("applications"), "", 0, &mut BTreeSet::new(), &mut files);
        for (id, path) in files {
            if decided.contains(&id) {
                continue;
            }
            let Some(file) = read_entry(&path).and_then(|text| parse(&text)) else {
                continue;
            };
            let Some(meaning) = interpret(&file, &id, &path, packaged, session) else {
                continue;
            };
            decided.insert(id.clone());
            if let Meaning::App(entry) = meaning {
                apps.insert(id, entry);
            }
        }
    }
    let mut apps: Vec<DesktopEntry> = apps.into_values().collect();
    apps.sort_by(|a, b| {
        a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id))
    });
    apps
}

/// The indexed entry of the trait's `app`: the one in `entries`, an earlier [`index`], with its
/// desktop file ID. `Apps::launch` and `Apps::open_path` name an app by its [`AppEntry`], and the
/// [`Opener`](crate::Opener) needs its [`DesktopEntry`], so the trait's implementation keeps the
/// entries of its last index for this. [`FileError::NotFound`] when none has that ID now, as
/// after the app was removed.
pub fn entry_for<'a>(
    entries: &'a [DesktopEntry],
    app: &AppEntry,
) -> Result<&'a DesktopEntry, FileError> {
    entries
        .iter()
        .find(|entry| entry.id == app.id)
        .ok_or(FileError::NotFound { what: "desktop entry" })
}

/// The text of the desktop entry at `path`; `None` when it cannot be opened or read, is not
/// UTF-8, is over [`MAX_ENTRY_BYTES`], or is not a regular file. The open never waits (see
/// [`OPEN_FLAGS`]), and the check is of the opened descriptor, so it holds for what is read even
/// when the path changed after the folder was listed.
fn read_entry(path: &Path) -> Option<String> {
    let file = File::options().read(true).custom_flags(OPEN_FLAGS).open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    // Up to one byte over the limit is read: a file that reaches it is too long.
    let mut limited = file.take(MAX_ENTRY_BYTES + 1);
    let mut text = String::new();
    limited.read_to_string(&mut text).ok()?;
    (limited.limit() > 0).then_some(text)
}

/// Whether a data folder is one of Flatpak's or Snap's export folders, whose apps are packaged.
fn is_export(dir: &Path) -> bool {
    dir.ends_with("flatpak/exports/share") || dir.ends_with("snapd/desktop")
}

/// The `.desktop` files under `dir`, with their desktop file IDs (`prefix` is the ID's start
/// for a subfolder, such as `kde4-`), sorted by path so a folder's files keep one order.
///
/// `walked` holds the folders this walk has read, by the device and inode that every path to a
/// folder shares. A folder in it is not read again, so a link back to it leads nowhere and its
/// files keep the IDs of the path that reached it first. Once it holds [`MAX_FOLDERS`], the walk
/// reads no more folders, but keeps the files it has listed.
fn collect(
    dir: &Path,
    prefix: &str,
    depth: usize,
    walked: &mut BTreeSet<(u64, u64)>,
    out: &mut Vec<(String, PathBuf)>,
) {
    if walked.len() >= MAX_FOLDERS {
        return;
    }
    let Ok(folder) = std::fs::metadata(dir) else {
        return;
    };
    if !walked.insert((folder.dev(), folder.ino())) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            Some((entry.file_name().into_string().ok()?, entry.path()))
        })
        .collect();
    entries.sort();
    for (name, path) in entries {
        if path.is_dir() {
            if depth < MAX_DEPTH {
                collect(&path, &format!("{prefix}{name}-"), depth + 1, walked, out);
            }
        } else if name.ends_with(".desktop") {
            out.push((format!("{prefix}{name}"), path));
        }
    }
}

//! The app index: every application the desktop's menu shows, from the desktop entries under
//! each data folder's `applications/` (BUILD.md section 3.4's `Apps::index`; docs/LINUX.md
//! section 4).
//!
//! Data folders are searched in precedence order, `XDG_DATA_HOME` first, and the first file
//! with a desktop file ID decides it: a user's copy replaces the system's, and a user's
//! `Hidden=true` copy removes the app.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::Personal;

use crate::entry::{DesktopEntry, Meaning, interpret};
use crate::keyfile::{locale_keys, messages_locale, parse};

/// How deep the index looks below an `applications/` folder (`kde4/kate.desktop` is one deep).
const MAX_DEPTH: usize = 8;

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
/// the unmerged base crates); this module's type gives way to it when that merges.
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
            path: Some(Personal::new(entry.path.to_string_lossy().into_owned())),
        }
    }
}

/// What the index reads from the session's environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
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

    /// The session the variables describe: `XDG_DATA_HOME`, `XDG_DATA_DIRS`, `HOME`, `LC_ALL`,
    /// `LC_MESSAGES`, `LANG`, `XDG_CURRENT_DESKTOP` and `PATH`.
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
            .into_iter()
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
        Self {
            data_dirs,
            locales: locale.map(|locale| locale_keys(&locale)).unwrap_or_default(),
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
#[must_use]
pub fn index(session: &Session) -> Vec<DesktopEntry> {
    let mut decided: BTreeSet<String> = BTreeSet::new();
    let mut apps: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for dir in &session.data_dirs {
        let packaged = is_export(dir);
        let mut files = Vec::new();
        collect(&dir.join("applications"), "", 0, &mut files);
        for (id, path) in files {
            if decided.contains(&id) {
                continue;
            }
            let Some(file) = std::fs::read_to_string(&path).ok().and_then(|text| parse(&text))
            else {
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

/// Whether a data folder is one of Flatpak's or Snap's export folders, whose apps are packaged.
fn is_export(dir: &Path) -> bool {
    dir.ends_with("flatpak/exports/share") || dir.ends_with("snapd/desktop")
}

/// The `.desktop` files under `dir`, with their desktop file IDs (`prefix` is the ID's start
/// for a subfolder, such as `kde4-`), sorted by path so a folder's files keep one order.
fn collect(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<(String, PathBuf)>) {
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
                collect(&path, &format!("{prefix}{name}-"), depth + 1, out);
            }
        } else if name.ends_with(".desktop") {
            out.push((format!("{prefix}{name}"), path));
        }
    }
}

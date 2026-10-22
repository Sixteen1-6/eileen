//! What one desktop entry file means for the app index: a visible app, an app this desktop does
//! not show, or a deletion that hides the same app in lower-precedence folders.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::Personal;

use crate::keyfile::{KeyFile, Line, list, localized, unescape};
use crate::{AppKind, Session};

/// An application's desktop entry, as the index keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// The desktop file ID: the file's path under `applications/` with `/` as `-`, such as
    /// `org.gnome.Calculator.desktop` or `kde4-kate.desktop`.
    pub id: String,
    /// `Name`, localized.
    pub name: String,
    /// `GenericName`, localized: what kind of app it is, such as "Web Browser".
    pub generic_name: Option<String>,
    /// `Keywords`, localized.
    pub keywords: Vec<String>,
    /// `Exec` as written, field codes (`%U`) included; `None` for an app started over D-Bus.
    /// A user's own entry can name their files, so it is personal.
    pub exec: Option<Personal<String>>,
    /// `Terminal`: the app runs in a terminal.
    pub terminal: bool,
    /// `DBusActivatable`: the app is started over D-Bus (`org.freedesktop.Application`).
    pub dbus_activatable: bool,
    /// `StartupWMClass`: the class of the app's windows, which ties a window to its entry.
    pub startup_wm_class: Option<String>,
    /// Packaged (Flatpak, Snap) or a classic desktop app.
    pub kind: AppKind,
    /// The file, which may be under the user's home folder.
    pub path: Personal<PathBuf>,
}

/// What a file means for its desktop file ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Meaning {
    /// An app to list.
    App(DesktopEntry),
    /// The ID exists here, so it hides the same ID further down, but nothing is listed: a
    /// `Hidden` entry, one this desktop does not show, or one whose `TryExec` is missing.
    Masked,
}

/// Interprets a parsed file found at `path` with the desktop file ID `id`; `None` for a file
/// that is not a valid application entry, which leaves lower folders' entries visible.
pub(crate) fn interpret(
    file: &KeyFile,
    id: &str,
    path: &Path,
    packaged: bool,
    session: &Session,
) -> Option<Meaning> {
    let lines = file.group("Desktop Entry")?;
    let raw = |key: &str| lines.iter().find(|line| line.key == key && line.locale.is_none());
    let flag = |key: &str| raw(key).is_some_and(|line| line.raw.trim() == "true");
    if flag("Hidden") {
        return Some(Meaning::Masked);
    }
    if raw("Type").map(|line| line.raw.trim()) != Some("Application") {
        return None;
    }
    let name = localized(lines, "Name", &session.locales).map(unescape)?;
    let exec = raw("Exec").map(|line| unescape(&line.raw)).filter(|exec| !exec.is_empty());
    let dbus_activatable = flag("DBusActivatable");
    if exec.is_none() && !dbus_activatable {
        return None;
    }
    if flag("NoDisplay") || !shown_here(lines, session) || !try_exec_found(raw("TryExec"), session)
    {
        return Some(Meaning::Masked);
    }
    let packaged = packaged || raw("X-Flatpak").is_some() || raw("X-SnapInstanceName").is_some();
    Some(Meaning::App(DesktopEntry {
        id: id.to_owned(),
        name,
        generic_name: localized(lines, "GenericName", &session.locales).map(unescape),
        keywords: localized(lines, "Keywords", &session.locales).map(list).unwrap_or_default(),
        exec: exec.map(Personal::new),
        terminal: flag("Terminal"),
        dbus_activatable,
        startup_wm_class: raw("StartupWMClass").map(|line| unescape(&line.raw)),
        kind: if packaged { AppKind::Packaged } else { AppKind::Desktop },
        path: Personal::new(path.to_path_buf()),
    }))
}

/// `OnlyShowIn` and `NotShowIn` against the session's desktops (`XDG_CURRENT_DESKTOP`).
fn shown_here(lines: &[Line], session: &Session) -> bool {
    let names = |key: &str| {
        lines
            .iter()
            .find(|line| line.key == key && line.locale.is_none())
            .map(|line| list(&line.raw))
    };
    let current = |names: &[String]| names.iter().any(|name| session.desktops.contains(name));
    names("OnlyShowIn").is_none_or(|names| current(&names))
        && !names("NotShowIn").is_some_and(|names| current(&names))
}

/// Whether `TryExec`, when the entry has a non-empty one, names an executable file: a path, or
/// a name looked up in the session's `PATH`.
fn try_exec_found(line: Option<&Line>, session: &Session) -> bool {
    let program = line.map(|line| unescape(line.raw.trim())).unwrap_or_default();
    if program.is_empty() {
        return true;
    }
    let program = Path::new(&program);
    if program.is_absolute() {
        return is_executable(program);
    }
    session.path.iter().any(|dir| is_executable(&dir.join(program)))
}

/// A regular file with an execute bit.
fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

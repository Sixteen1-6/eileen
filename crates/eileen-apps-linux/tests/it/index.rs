//! The app index over the fixture session: precedence, masking, the desktop's filters,
//! `TryExec`, locales and packaged apps.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use eileen_apps_linux::{AppEntry, AppKind, DesktopEntry, Session, index};

/// The fixture session's root.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("session")
}

/// The fixture session as `from_vars` reads it, held to the fixture's folders: the export
/// folders `from_vars` adds are the real machine's, which a test must not read.
fn session(lang: &str, desktop: &str, path: Option<&Path>) -> Session {
    session_with(lang, None, desktop, path)
}

fn session_with(lang: &str, language: Option<&str>, desktop: &str, path: Option<&Path>) -> Session {
    let root = fixture();
    let dirs = ["usr/share", "var/lib/flatpak/exports/share", "var/lib/snapd/desktop"]
        .map(|dir| root.join(dir).to_string_lossy().into_owned())
        .join(":");
    let vars = |name: &str| -> Option<OsString> {
        match name {
            "HOME" => Some(root.join("home/sam").into()),
            "XDG_DATA_DIRS" => Some(dirs.clone().into()),
            "LANG" => Some(lang.into()),
            "LANGUAGE" => language.map(Into::into),
            "XDG_CURRENT_DESKTOP" => Some(desktop.into()),
            "PATH" => path.map(|path| path.as_os_str().to_owned()),
            _ => None,
        }
    };
    let mut session = Session::from_vars(vars);
    session.data_dirs.retain(|dir| dir.starts_with(&root));
    session
}

fn names(apps: &[DesktopEntry]) -> Vec<&str> {
    apps.iter().map(|app| app.name.as_str()).collect()
}

fn find<'a>(apps: &'a [DesktopEntry], id: &str) -> &'a DesktopEntry {
    apps.iter().find(|app| app.id == id).unwrap_or_else(|| panic!("{id} is not listed"))
}

#[test]
fn the_menu_lists_what_gnome_shows_with_the_users_copies_first() {
    let apps = index(&session("en_US.UTF-8", "ubuntu:GNOME", None));
    assert_eq!(
        names(&apps),
        [
            "Back up photos",
            "Firefox (work profile)",
            "Htop",
            "Kate",
            "Spotify",
            "Text Editor",
            "VLC media player"
        ],
        "hidden: the user's removed calculator, Konsole (KDE only), the app not on GNOME, \
         nm-applet (NoDisplay), the missing TryExec, a link, a broken file and one with no Exec"
    );
    let firefox = find(&apps, "firefox.desktop");
    let exec = firefox.exec.as_ref().map(|exec| exec.get().as_str());
    assert_eq!(exec, Some("firefox -P work %u"), "the user's copy");
    assert!(firefox.path.get().starts_with(fixture().join("home")), "from the home folder");
    assert_eq!(find(&apps, "kde4-kate.desktop").name, "Kate", "a subfolder's ID");
    let editor = find(&apps, "org.gnome.TextEditor.desktop");
    assert!(editor.dbus_activatable, "started over D-Bus");
    assert_eq!(editor.keywords, ["text", "editor", "notes;plain"], "an escaped semicolon");
    assert!(find(&apps, "htop.desktop").terminal, "a terminal app");
}

#[test]
fn flatpak_and_snap_apps_are_packaged() {
    let apps = index(&session("en_US.UTF-8", "GNOME", None));
    assert_eq!(find(&apps, "org.videolan.VLC.desktop").kind, AppKind::Packaged, "Flatpak");
    assert_eq!(find(&apps, "spotify_spotify.desktop").kind, AppKind::Packaged, "Snap");
    assert_eq!(find(&apps, "htop.desktop").kind, AppKind::Desktop, "a classic app");
    let entry = AppEntry::from(find(&apps, "org.videolan.VLC.desktop"));
    assert_eq!(entry.id, "org.videolan.VLC.desktop", "the desktop file ID");
    assert_eq!(entry.name, "VLC media player", "the menu's name");
    assert!(entry.path.is_some(), "the file, kept as personal");
}

#[test]
fn names_and_keywords_follow_the_locale() {
    let apps = index(&session("de_DE.UTF-8", "GNOME", None));
    let editor = find(&apps, "org.gnome.TextEditor.desktop");
    assert_eq!(editor.name, "Texteditor", "Name[de]");
    assert_eq!(editor.keywords, ["Text", "Editor", "Notizen"], "Keywords[de]");
    assert_eq!(
        find(&apps, "firefox.desktop").name,
        "Firefox (work profile)",
        "the user's copy has no German name, and the system's is hidden by it"
    );
    let pt = index(&session("pt_BR.UTF-8", "GNOME", None));
    assert_eq!(find(&pt, "org.gnome.TextEditor.desktop").name, "Editor de texto", "pt_BR");
}

#[test]
fn another_desktop_sees_its_own_apps() {
    let apps = index(&session("en_US.UTF-8", "KDE", None));
    let names = names(&apps);
    assert!(names.contains(&"Konsole"), "OnlyShowIn=KDE");
    assert!(names.contains(&"Not On GNOME"), "NotShowIn=GNOME");
}

#[test]
fn try_exec_needs_an_executable_on_the_path() {
    let bin = Path::new(env!("CARGO_TARGET_TMPDIR")).join("apps-linux-path");
    fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("eileen-fixture-tool");
    fs::write(&tool, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
    let apps = index(&session("en_US.UTF-8", "GNOME", Some(&bin)));
    assert!(!names(&apps).contains(&"Needs a tool"), "not executable");
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let apps = index(&session("en_US.UTF-8", "GNOME", Some(&bin)));
    assert!(names(&apps).contains(&"Needs a tool"), "executable on the PATH");
}

#[test]
fn data_folders_follow_the_xdg_rules_and_add_the_missing_exports() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| -> Option<OsString> {
            pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).into())
        }
    };
    let dirs = |session: Session| -> Vec<String> {
        session.data_dirs.iter().map(|dir| dir.to_string_lossy().into_owned()).collect()
    };
    assert_eq!(
        dirs(Session::from_vars(vars(&[("HOME", "/home/sam")]))),
        [
            "/home/sam/.local/share",
            "/usr/local/share",
            "/usr/share",
            "/home/sam/.local/share/flatpak/exports/share",
            "/var/lib/flatpak/exports/share",
            "/var/lib/snapd/desktop"
        ],
        "the defaults, then the exports"
    );
    assert_eq!(
        dirs(Session::from_vars(vars(&[
            ("HOME", "/home/sam"),
            ("XDG_DATA_HOME", "relative/ignored"),
            ("XDG_DATA_DIRS", "/var/lib/snapd/desktop:relative:/usr/share"),
        ]))),
        [
            "/home/sam/.local/share",
            "/var/lib/snapd/desktop",
            "/usr/share",
            "/home/sam/.local/share/flatpak/exports/share",
            "/var/lib/flatpak/exports/share"
        ],
        "relative entries are dropped and a listed export is not added twice"
    );
}

#[test]
fn the_session_the_tests_run_in_indexes_cleanly() {
    for app in index(&Session::from_env()) {
        assert!(!app.name.is_empty(), "{} has a name", app.id);
        assert!(app.id.ends_with(".desktop"), "{} is a desktop file ID", app.id);
        assert!(app.exec.is_some() || app.dbus_activatable, "{} can be started", app.id);
    }
}

#[test]
fn language_comes_before_the_locale_except_under_c() {
    let apps = index(&session_with("en_US.UTF-8", Some("de:en"), "GNOME", None));
    assert_eq!(find(&apps, "org.gnome.TextEditor.desktop").name, "Texteditor", "LANGUAGE first");
    let apps = index(&session_with("C.UTF-8", Some("de"), "GNOME", None));
    assert_eq!(find(&apps, "org.gnome.TextEditor.desktop").name, "Text Editor", "ignored under C");
}

#[test]
fn an_empty_try_exec_is_no_condition() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("apps-linux-empty-tryexec");
    let applications = root.join("share").join("applications");
    fs::create_dir_all(&applications).unwrap();
    fs::write(
        applications.join("plain.desktop"),
        "[Desktop Entry]\nType=Application\nName=Plain\nExec=plain\nTryExec=\n",
    )
    .unwrap();
    let mut session = session("en_US.UTF-8", "GNOME", None);
    session.data_dirs = vec![root.join("share")];
    assert_eq!(names(&index(&session)), ["Plain"], "listed");
}

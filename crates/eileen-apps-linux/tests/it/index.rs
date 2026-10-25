//! The app index over the fixture session: precedence, masking, the desktop's filters,
//! `TryExec`, locales and packaged apps, and finding the entry of a trait's `AppEntry`; and over
//! a scratch folder, what it skips (a FIFO, a device and an entry over a mebibyte) and how it
//! walks links to folders.

use std::ffi::OsString;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use eileen_apps_linux::{AppEntry, AppKind, DesktopEntry, FileError, Session, entry_for, index};

use crate::open::Scratch;

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

fn ids(apps: &[DesktopEntry]) -> Vec<&str> {
    apps.iter().map(|app| app.id.as_str()).collect()
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
fn the_data_home_leads_the_data_folders() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| -> Option<OsString> {
            pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).into())
        }
    };
    let both = Session::from_vars(vars(&[("HOME", "/home/sam"), ("XDG_DATA_HOME", "/data/sam")]));
    assert_eq!(both.data_home.as_deref(), Some(Path::new("/data/sam")), "XDG_DATA_HOME first");
    assert_eq!(both.data_dirs.first(), both.data_home.as_ref(), "the index searches it first");
    let home = Session::from_vars(vars(&[("HOME", "/home/sam"), ("XDG_DATA_HOME", "")]));
    assert_eq!(
        home.data_home.as_deref(),
        Some(Path::new("/home/sam/.local/share")),
        "an empty XDG_DATA_HOME is unset"
    );
    let neither = Session::from_vars(vars(&[("HOME", "home/sam")]));
    assert_eq!(neither.data_home, None, "a relative HOME and no XDG_DATA_HOME");
    assert_eq!(
        neither.data_dirs.first().map(PathBuf::as_path),
        Some(Path::new("/usr/local/share")),
        "only the system's folders"
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

#[test]
fn an_app_entry_finds_its_indexed_entry_by_its_desktop_file_id() {
    let apps = index(&session("en_US.UTF-8", "ubuntu:GNOME", None));
    let firefox = AppEntry::from(find(&apps, "firefox.desktop"));
    let found = entry_for(&apps, &firefox).map(|entry| entry.path.get().clone());
    assert_eq!(
        found,
        Ok(fixture().join("home/sam/.local/share/applications/firefox.desktop")),
        "the entry that decides the ID: the user's copy"
    );
    let removed = AppEntry { id: "org.gnome.Calculator.desktop".to_owned(), ..firefox };
    assert_eq!(
        entry_for(&apps, &removed),
        Err(FileError::NotFound { what: "desktop entry" }),
        "an app the user removed has no entry, as one uninstalled since the caller listed it"
    );
}

/// The fixture session with one data folder, `scratch`, whose `applications/` holds one entry,
/// "Plain"; and that `applications/` folder, for the test's other files.
fn beside_a_plain_entry(scratch: &Scratch) -> (Session, PathBuf) {
    let applications = scratch.path().join("applications");
    fs::create_dir_all(&applications).unwrap();
    let plain = "[Desktop Entry]\nType=Application\nName=Plain\nExec=plain\n";
    fs::write(applications.join("plain.desktop"), plain).unwrap();
    let mut session = session("en_US.UTF-8", "GNOME", None);
    session.data_dirs = vec![scratch.path().to_owned()];
    (session, applications)
}

#[test]
fn a_fifo_or_a_device_named_as_an_entry_is_skipped_at_once() {
    // Nothing writes to the FIFO, so an open that waited for a writer would never return, and
    // /dev/zero never ends. The FIFO sorts first, so an index that waits hangs before it reads.
    // Both would be skipped even if they were read, the FIFO as empty and /dev/zero as over the
    // size limit, so the next test checks that a FIFO is not read.
    let scratch = Scratch::new();
    let (session, applications) = beside_a_plain_entry(&scratch);
    scratch.fifo("applications/pipe.desktop");
    symlink("/dev/zero", applications.join("zero.desktop")).unwrap();
    assert_eq!(names(&index(&session)), ["Plain"], "only the entry beside them, without waiting");
}

#[test]
fn a_fifo_that_holds_an_entry_is_skipped_unread() {
    // On Linux a read-write open of a FIFO never waits, and the read-only open after it finds
    // that writer. The reader held here keeps what was written once the writer closes, and with
    // no writer a read returns the entry and then the end: only the check that the descriptor
    // is a regular file keeps it out.
    let scratch = Scratch::new();
    let (session, _) = beside_a_plain_entry(&scratch);
    let fifo = scratch.fifo("applications/piped.desktop");
    let mut writer = fs::OpenOptions::new().read(true).write(true).open(&fifo).unwrap();
    let _reader = fs::File::open(&fifo).unwrap();
    writer.write_all(b"[Desktop Entry]\nType=Application\nName=Piped\nExec=piped\n").unwrap();
    drop(writer);
    assert_eq!(names(&index(&session)), ["Plain"], "what the FIFO holds is not read");
}

#[test]
fn an_entry_over_a_mebibyte_is_skipped() {
    // The index reads at most 1 MiB of an entry, and skips a longer one whole.
    const CAP: usize = 1 << 20;
    let entry = |name: &str, size: usize| {
        let mut text = format!("[Desktop Entry]\nType=Application\nName={name}\nExec=big\n#");
        text.push_str(&"x".repeat(size - text.len() - 1));
        text + "\n"
    };
    let scratch = Scratch::new();
    let (session, applications) = beside_a_plain_entry(&scratch);
    fs::write(applications.join("at.desktop"), entry("At the cap", CAP)).unwrap();
    fs::write(applications.join("over.desktop"), entry("Over the cap", CAP + 1)).unwrap();
    assert_eq!(names(&index(&session)), ["At the cap", "Plain"], "one byte over is skipped");
}

#[test]
fn a_folder_that_links_lead_back_to_is_walked_once() {
    // Ten links to `.`, and one to the data folder above, which holds only `applications/`. A
    // walk that followed each of them again at every level, down to its depth limit of 8, would
    // read the folder about 1.3e8 times and not return for hours.
    let scratch = Scratch::new();
    let (session, applications) = beside_a_plain_entry(&scratch);
    for n in 0..10 {
        symlink(".", applications.join(format!("here-{n}"))).unwrap();
    }
    symlink("..", applications.join("up")).unwrap();
    assert_eq!(ids(&index(&session)), ["plain.desktop"], "once, under its own path");
}

#[test]
fn a_folder_two_links_lead_to_is_walked_once_under_the_first() {
    // Some installs link their own folder into `applications/`. Here two links name one folder:
    // the walk follows `tools`, the first in its sorted order, and `tools-old` leads nowhere new,
    // so the folder's entry is listed once, with the first path's desktop file ID.
    let scratch = Scratch::new();
    let (session, applications) = beside_a_plain_entry(&scratch);
    let install = scratch.path().join("install");
    fs::create_dir_all(&install).unwrap();
    let tool = "[Desktop Entry]\nType=Application\nName=Tool\nExec=tool\n";
    fs::write(install.join("tool.desktop"), tool).unwrap();
    symlink(&install, applications.join("tools")).unwrap();
    symlink(&install, applications.join("tools-old")).unwrap();
    assert_eq!(
        ids(&index(&session)),
        ["plain.desktop", "tools-tool.desktop"],
        "a link to a new folder is followed, and a second path to it is not"
    );
}

#[test]
fn a_link_into_a_lower_data_folder_leaves_it_its_ids() {
    // The user's folder links to the system's `applications/`, so the user's walk reads it
    // first. Each data folder's walk keeps its own record of what it read, so the system's walk
    // reads it too, and its entry keeps the ID it has there.
    let scratch = Scratch::new();
    let (mut session, applications) = beside_a_plain_entry(&scratch);
    let system = scratch.path().join("system");
    fs::create_dir_all(system.join("applications")).unwrap();
    let tool = "[Desktop Entry]\nType=Application\nName=Tool\nExec=tool\n";
    fs::write(system.join("applications").join("tool.desktop"), tool).unwrap();
    symlink(system.join("applications"), applications.join("system")).unwrap();
    session.data_dirs.push(system);
    assert_eq!(
        ids(&index(&session)),
        ["plain.desktop", "system-tool.desktop", "tool.desktop"],
        "the link's path in the user's folder, and the system's own"
    );
}

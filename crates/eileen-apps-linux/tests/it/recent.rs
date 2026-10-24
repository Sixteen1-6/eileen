//! The recent files over fixture files in `tests/fixtures/recent/`: one in the layout `GLib`
//! writes, one in KIO's, and hand-written ones for the rules GTK never exercises, the namespaces,
//! and the missing, empty and broken files; over a FIFO and a device in the file's place; and
//! over sparse files at the read limit and one byte past it. The bookmarks come in the file's
//! order, the list newest first.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use eileen_apps_linux::{FileError, RecentItem, RecentlyUsed, Session};
use eileen_contracts::ids::{Date, WallTime};
use eileen_contracts::tools::apps::FileKind;

use crate::open::Scratch;

/// One item as the tests compare it: path, app, kind and time.
type Row<'a> = (&'a str, Option<&'a str>, Option<FileKind>, WallTime);

/// The fixtures' folder.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("recent")
}

/// The list in the fixture file `name`.
pub(crate) fn list(name: &str) -> RecentlyUsed {
    RecentlyUsed::at(fixtures().join(name))
}

/// The fixture home folder's file, which `GLib` wrote.
pub(crate) fn gtk() -> RecentlyUsed {
    list("home/sam/.local/share/recently-used.xbel")
}

/// Every bookmark in a list, in the file's order.
fn all(list: &RecentlyUsed) -> Vec<RecentItem> {
    list.bookmarks().unwrap()
}

/// Every recent file in a list, newest first.
fn newest(list: &RecentlyUsed) -> Vec<RecentItem> {
    list.recent(usize::MAX).unwrap()
}

/// A UTC time from its date, its `hh:mm:ss` and its microseconds, without the module's parser.
fn utc(date: &str, clock: &str, micros: i64) -> WallTime {
    let days = date.parse::<Date>().unwrap().days_since_epoch();
    let seconds = clock.split(':').fold(0, |total, part| total * 60 + part.parse::<i64>().unwrap());
    WallTime((days * 86_400 + seconds) * 1_000_000 + micros)
}

fn rows(items: &[RecentItem]) -> Vec<Row<'_>> {
    items
        .iter()
        .map(|item| {
            let app = item.app.as_ref().map(|app| app.get().as_str());
            (item.path.get().as_str(), app, item.kind, item.opened_at)
        })
        .collect()
}

fn paths(items: &[RecentItem]) -> Vec<&str> {
    items.iter().map(|item| item.path.get().as_str()).collect()
}

fn find<'a>(items: &'a [RecentItem], path: &str) -> &'a RecentItem {
    items
        .iter()
        .find(|item| item.path.get() == path)
        .unwrap_or_else(|| panic!("{path} is not listed"))
}

fn app_of(item: &RecentItem) -> Option<&str> {
    item.app.as_ref().map(|app| app.get().as_str())
}

#[test]
fn glibs_file_lists_its_local_public_files_in_the_files_order() {
    assert_eq!(
        rows(&all(&gtk())),
        [
            (
                "/home/sam/Documents/Quarterly Report.odt",
                Some("LibreOffice 24.2"),
                Some(FileKind::Document),
                utc("2026-10-20", "15:31:40", 551_201),
            ),
            (
                "/home/sam/Downloads/fonts.zip",
                Some("Archive Manager"),
                None,
                utc("2026-10-12", "13:45:51", 663_009),
            ),
            (
                "/home/sam/Music/Rainier Timelapse Score.flac",
                Some("Music"),
                Some(FileKind::Audio),
                utc("2026-10-16", "07:22:10", 81_736),
            ),
            (
                "/home/sam/Pictures/写真",
                Some("Files"),
                Some(FileKind::Folder),
                utc("2026-10-17", "11:03:27", 440_192),
            ),
            (
                "/home/sam/Videos/Mount Rainier Timelapse.mp4",
                Some("Videos"),
                Some(FileKind::Video),
                utc("2026-10-18", "20:41:55", 902_114),
            ),
            (
                "/home/sam/Documents/Résumé 2026.pdf",
                Some("org.kde.okular"),
                Some(FileKind::Document),
                utc("2026-10-19", "08:15:00", 250_000),
            ),
            (
                "/home/sam/Documents/Sales & Costs 2026.ods",
                Some("LibreOffice 24.2"),
                Some(FileKind::Spreadsheet),
                utc("2026-10-21", "09:58:14", 672_310),
            ),
            (
                "/home/sam/Pictures/Beach Day/IMG_4412.jpg",
                Some("Image Viewer"),
                Some(FileKind::Image),
                utc("2026-10-21", "18:44:09", 310_460),
            ),
            (
                "/home/sam/Notes/Sam's notes.md",
                Some("Text Editor"),
                Some(FileKind::Document),
                utc("2026-10-21", "12:30:02", 417),
            ),
            (
                "/home/sam/src/eileen/crates/eileen-apps-linux/src/recent.rs",
                Some("Text Editor"),
                Some(FileKind::Code),
                utc("2026-10-21", "21:07:33", 118_204),
            ),
        ],
        "every local, public file, in the file's order, with its app, kind and time"
    );
}

#[test]
fn kios_file_reads_as_glibs_does() {
    assert_eq!(
        rows(&all(&list("xdg-data/recently-used.xbel"))),
        [(
            "/home/sam/Pictures/Screenshots/Screenshot_20261021_075830.png",
            Some("org.kde.gwenview"),
            Some(FileKind::Image),
            utc("2026-10-21", "07:58:30", 512_000),
        )],
        "KIO's layout, with its milliseconds"
    );
}

#[test]
fn entities_and_percent_escapes_decode() {
    let items = all(&gtk());
    let paths = paths(&items);
    assert!(paths.contains(&"/home/sam/Documents/Sales & Costs 2026.ods"), "&amp; and %20");
    assert!(paths.contains(&"/home/sam/Notes/Sam's notes.md"), "&apos;");
    assert!(paths.contains(&"/home/sam/Documents/Résumé 2026.pdf"), "escaped UTF-8");
    assert!(paths.contains(&"/home/sam/Pictures/写真"), "escaped UTF-8 in three bytes a letter");
}

#[test]
fn the_newest_application_names_the_app() {
    let items = all(&gtk());
    assert_eq!(
        app_of(find(&items, "/home/sam/Pictures/Beach Day/IMG_4412.jpg")),
        Some("Image Viewer"),
        "listed first, opened last; GIMP is listed after it but opened it earlier"
    );
    let odd = all(&list("odd.xbel"));
    assert_eq!(
        app_of(find(&odd, "/home/sam/odd/app-newest.txt")),
        Some("Newest App"),
        "the later of two"
    );
    assert_eq!(
        app_of(find(&odd, "/home/sam/odd/app-tie.txt")),
        Some("Listed First"),
        "the first listed of two with the same time"
    );
    let nameless = find(&odd, "/home/sam/odd/nameless.txt");
    assert_eq!(app_of(nameless), Some("Named"), "an application without a name is ignored");
    assert_eq!(
        nameless.opened_at,
        utc("2026-10-09", "11:00:00", 0),
        "and so is its time, later than the named one's"
    );
}

#[test]
fn opened_at_is_the_newest_time_the_bookmark_and_its_applications_give() {
    let odd = all(&list("odd.xbel"));
    assert_eq!(
        find(&odd, "/home/sam/odd/app-newest.txt").opened_at,
        utc("2026-10-02", "12:00:00", 500_000),
        "an application's modified, newer than the bookmark's"
    );
    assert_eq!(
        find(&odd, "/home/sam/odd/visited-newest.txt").opened_at,
        utc("2026-10-04", "16:30:00", 0),
        "visited, given at +02:00"
    );
    assert_eq!(
        find(&odd, "/home/sam/odd/added-only.txt").opened_at,
        utc("2026-09-30", "23:59:59", 0),
        "added, when nothing else is there"
    );
}

#[test]
fn only_local_public_files_are_listed() {
    let paths_of = |list: &RecentlyUsed| -> Vec<String> {
        all(list).iter().map(|item| item.path.get().clone()).collect()
    };
    let gtk = paths_of(&gtk());
    assert!(!gtk.iter().any(|path| path.contains("budget-2027")), "an sftp: entry is skipped");
    assert!(!gtk.iter().any(|path| path.contains("minutes")), "an smb: entry is skipped");
    assert!(
        !gtk.iter().any(|path| path.contains("statement")),
        "a private entry is skipped, though it is the newest"
    );
    let odd = paths_of(&list("odd.xbel"));
    for skipped in ["remote", "escape", "latin1"] {
        assert!(!odd.iter().any(|path| path.contains(skipped)), "{skipped} is not a local path");
    }
    let namespaces = paths_of(&list("namespaces.xbel"));
    assert!(!namespaces.iter().any(|path| path.ends_with("private.txt")), "private, under bm:");
}

#[test]
fn unusable_entries_are_skipped_and_the_rest_read() {
    let odd = all(&list("odd.xbel"));
    assert_eq!(
        rows(&odd),
        [
            (
                "/home/sam/odd/app-newest.txt",
                Some("Newest App"),
                Some(FileKind::Document),
                utc("2026-10-02", "12:00:00", 500_000)
            ),
            (
                "/home/sam/odd/visited-newest.txt",
                Some("Reader"),
                Some(FileKind::Document),
                utc("2026-10-04", "16:30:00", 0)
            ),
            ("/home/sam/odd/added-only.txt", None, None, utc("2026-09-30", "23:59:59", 0)),
            ("/home/sam/odd/twice.txt", Some("Zeroth"), None, utc("2026-10-05", "14:00:00", 0)),
            ("/home/sam/odd/twice.txt", Some("First"), None, utc("2026-10-06", "10:00:00", 0)),
            ("/home/sam/odd/foreign.txt", None, None, utc("2026-10-08", "10:00:00", 0)),
            ("/home/sam/odd/twice.txt", Some("Second"), None, utc("2026-10-07", "10:00:00", 0)),
            ("/home/sam/odd/nameless.txt", Some("Named"), None, utc("2026-10-09", "11:00:00", 0)),
            (
                "/home/sam/odd/app-tie.txt",
                Some("Listed First"),
                None,
                utc("2026-10-10", "10:00:00", 0)
            ),
            ("/home/sam/odd/no-info.txt", None, None, utc("2026-10-10", "08:00:00", 0)),
            ("/home/sam/odd/tie-b.txt", None, None, utc("2026-10-11", "10:00:00", 0)),
            ("/home/sam/odd/tie-a.txt", None, None, utc("2026-10-11", "10:00:00", 0)),
            ("/home/sam/odd/empty-element.txt", None, None, utc("2026-10-12", "10:00:00", 0)),
        ],
        "skipped: no time, a bad time, a bad application time, no href, another host, an \
         escaped slash, a path that is not UTF-8 and a bookmark inside a folder; a path the \
         file lists three times is there three times, in the file's order"
    );
}

#[test]
fn metadata_counts_only_from_freedesktop_and_by_namespace() {
    let odd = all(&list("odd.xbel"));
    let foreign = find(&odd, "/home/sam/odd/foreign.txt");
    assert_eq!(
        (app_of(foreign), foreign.kind, foreign.opened_at),
        (None, None, utc("2026-10-08", "10:00:00", 0)),
        "another owner's metadata gives no app, kind or time"
    );
    let outside = find(&odd, "/home/sam/odd/no-info.txt");
    assert_eq!(
        (app_of(outside), outside.kind, outside.opened_at),
        (None, None, utc("2026-10-10", "08:00:00", 0)),
        "nor does metadata outside info"
    );
    assert_eq!(
        rows(&all(&list("namespaces.xbel"))),
        [
            (
                "/home/sam/ns/other-prefixes.odp",
                Some("Impress"),
                Some(FileKind::Presentation),
                utc("2026-10-20", "10:00:00", 0)
            ),
            (
                "/home/sam/ns/default-namespace.xlsx",
                Some("Calc"),
                Some(FileKind::Spreadsheet),
                utc("2026-10-19", "10:00:00", 0)
            ),
            ("/home/sam/ns/other-namespaces.pptx", None, None, utc("2026-10-18", "10:00:00", 0)),
        ],
        "bm: and mt: prefixes and default namespaces are read; bookmark: and mime: bound to \
         other namespaces are not, so that entry has no app or kind, is not private, and its \
         later application time does not count"
    );
}

#[test]
fn a_missing_empty_or_cleared_file_has_no_recent_files() {
    assert_eq!(list("no-such-folder/recently-used.xbel").bookmarks(), Ok(Vec::new()), "missing");
    assert_eq!(list("empty.xbel").bookmarks(), Ok(Vec::new()), "empty");
    assert_eq!(list("cleared.xbel").bookmarks(), Ok(Vec::new()), "GLib's file with no bookmarks");
}

#[test]
fn a_fifo_or_a_device_in_the_files_place_is_refused_unread() {
    // Nothing writes to the FIFO, so an open that waited for a writer would never return.
    let scratch = Scratch::new();
    let fifo = RecentlyUsed::at(scratch.fifo("recently-used.xbel"));
    let not_a_file =
        Err(FileError::Invalid { what: "recently-used.xbel", detail: "not a file".to_owned() });
    assert_eq!(fifo.bookmarks(), not_a_file, "a FIFO, opened without waiting");
    assert_eq!(fifo.recent(10), not_a_file, "and the list of it");
    assert_eq!(
        RecentlyUsed::at("/dev/null").bookmarks(),
        not_a_file,
        "a device, which would read as an empty file"
    );
}

/// A file in `scratch`, named `name`, that holds `text` and then zeros to `len` bytes.
/// `File::set_len` adds the zeros without writing them, so they take no disk space.
fn sparse(scratch: &Scratch, name: &str, text: &[u8], len: u64) -> RecentlyUsed {
    let path = scratch.path().join(name);
    fs::write(&path, text).unwrap();
    fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(len).unwrap();
    RecentlyUsed::at(path)
}

#[test]
fn a_file_over_16_mib_is_refused_whole() {
    // The reader reads at most 16 MiB and one byte of the file, and a file that reaches that
    // byte is refused rather than listed from its first 16 MiB. The files hold the GLib
    // fixture and then zeros, which read as text after the root.
    const LIMIT: u64 = 16 << 20;
    let gtk_file = fs::read(fixtures().join("home/sam/.local/share/recently-used.xbel")).unwrap();
    let scratch = Scratch::new();
    let at = sparse(&scratch, "at-the-limit.xbel", &gtk_file, LIMIT);
    assert_eq!(at.bookmarks(), gtk().bookmarks(), "a file of exactly 16 MiB is read whole");
    let over = sparse(&scratch, "over-the-limit.xbel", &gtk_file, LIMIT + 1);
    let too_large = Err(FileError::Invalid {
        what: "recently-used.xbel",
        detail: "larger than 16 MiB".to_owned(),
    });
    assert_eq!(over.bookmarks(), too_large, "one byte more is refused");
    assert_eq!(over.recent(10), too_large, "and so is the list of it");
    // Without its end tag, what is read ends inside the root: broken XML on its own.
    let open_root = gtk_file.strip_suffix(b"</xbel>").unwrap();
    let cut = sparse(&scratch, "cut-inside-the-root.xbel", open_root, LIMIT + 1);
    assert_eq!(cut.bookmarks(), too_large, "refused for its size, not for the cut");
}

#[test]
fn a_broken_file_is_an_error() {
    let problem = |name: &str| match list(name).bookmarks() {
        Err(FileError::Invalid { what: "recently-used.xbel", detail }) => {
            let (problem, offset) = detail.rsplit_once(" near byte ").unwrap();
            assert!(offset.parse::<u64>().unwrap() > 0, "{name}: an offset into the file");
            problem.to_owned()
        }
        other => panic!("{name} read as {other:?}"),
    };
    assert_eq!(problem("truncated.xbel"), "not well-formed XML", "cut off in an attribute");
    assert_eq!(problem("mismatched.xbel"), "not well-formed XML", "an end tag that does not match");
    assert_eq!(problem("not-xbel.xbel"), "not an XBEL bookmark file", "another root");
    assert_eq!(
        RecentlyUsed::at(fixtures()).bookmarks(),
        Err(FileError::Io { what: "recently-used.xbel", detail: "is a directory".to_owned() }),
        "a folder where the file should be is unreadable"
    );
    let message = list("truncated.xbel").bookmarks().unwrap_err().to_string();
    assert!(!message.contains("/home/sam") && !message.contains("soffice"), "{message}");
}

/// What the `GLib` fixture lists, newest first.
fn gtk_rows() -> Vec<Row<'static>> {
    vec![
        (
            "/home/sam/src/eileen/crates/eileen-apps-linux/src/recent.rs",
            Some("Text Editor"),
            Some(FileKind::Code),
            utc("2026-10-21", "21:07:33", 118_204),
        ),
        (
            "/home/sam/Pictures/Beach Day/IMG_4412.jpg",
            Some("Image Viewer"),
            Some(FileKind::Image),
            utc("2026-10-21", "18:44:09", 310_460),
        ),
        (
            "/home/sam/Notes/Sam's notes.md",
            Some("Text Editor"),
            Some(FileKind::Document),
            utc("2026-10-21", "12:30:02", 417),
        ),
        (
            "/home/sam/Documents/Sales & Costs 2026.ods",
            Some("LibreOffice 24.2"),
            Some(FileKind::Spreadsheet),
            utc("2026-10-21", "09:58:14", 672_310),
        ),
        (
            "/home/sam/Documents/Quarterly Report.odt",
            Some("LibreOffice 24.2"),
            Some(FileKind::Document),
            utc("2026-10-20", "15:31:40", 551_201),
        ),
        (
            "/home/sam/Documents/Résumé 2026.pdf",
            Some("org.kde.okular"),
            Some(FileKind::Document),
            utc("2026-10-19", "08:15:00", 250_000),
        ),
        (
            "/home/sam/Videos/Mount Rainier Timelapse.mp4",
            Some("Videos"),
            Some(FileKind::Video),
            utc("2026-10-18", "20:41:55", 902_114),
        ),
        (
            "/home/sam/Pictures/写真",
            Some("Files"),
            Some(FileKind::Folder),
            utc("2026-10-17", "11:03:27", 440_192),
        ),
        (
            "/home/sam/Music/Rainier Timelapse Score.flac",
            Some("Music"),
            Some(FileKind::Audio),
            utc("2026-10-16", "07:22:10", 81_736),
        ),
        (
            "/home/sam/Downloads/fonts.zip",
            Some("Archive Manager"),
            None,
            utc("2026-10-12", "13:45:51", 663_009),
        ),
    ]
}

#[test]
fn glibs_file_lists_its_local_files_newest_first() {
    assert_eq!(
        rows(&newest(&gtk())),
        gtk_rows(),
        "every local, public file, newest first, with its app, kind and time"
    );
}

#[test]
fn the_limit_keeps_the_newest() {
    let list = gtk();
    let expected = gtk_rows();
    assert_eq!(rows(&list.recent(3).unwrap()), expected[..3], "the three newest");
    assert_eq!(rows(&list.recent(1).unwrap()), expected[..1], "the newest");
    assert!(list.recent(0).unwrap().is_empty(), "none");
    assert_eq!(list.recent(100).unwrap().len(), expected.len(), "all, when the limit is higher");
}

#[test]
fn the_usable_entries_list_newest_first_each_path_once() {
    assert_eq!(
        rows(&newest(&list("odd.xbel"))),
        [
            ("/home/sam/odd/empty-element.txt", None, None, utc("2026-10-12", "10:00:00", 0)),
            ("/home/sam/odd/tie-b.txt", None, None, utc("2026-10-11", "10:00:00", 0)),
            ("/home/sam/odd/tie-a.txt", None, None, utc("2026-10-11", "10:00:00", 0)),
            (
                "/home/sam/odd/app-tie.txt",
                Some("Listed First"),
                None,
                utc("2026-10-10", "10:00:00", 0)
            ),
            ("/home/sam/odd/no-info.txt", None, None, utc("2026-10-10", "08:00:00", 0)),
            ("/home/sam/odd/nameless.txt", Some("Named"), None, utc("2026-10-09", "11:00:00", 0)),
            ("/home/sam/odd/foreign.txt", None, None, utc("2026-10-08", "10:00:00", 0)),
            ("/home/sam/odd/twice.txt", Some("Second"), None, utc("2026-10-07", "10:00:00", 0)),
            (
                "/home/sam/odd/visited-newest.txt",
                Some("Reader"),
                Some(FileKind::Document),
                utc("2026-10-04", "16:30:00", 0)
            ),
            (
                "/home/sam/odd/app-newest.txt",
                Some("Newest App"),
                Some(FileKind::Document),
                utc("2026-10-02", "12:00:00", 500_000)
            ),
            ("/home/sam/odd/added-only.txt", None, None, utc("2026-09-30", "23:59:59", 0)),
        ],
        "the usable entries newest first, the path the file lists three times once"
    );
}

#[test]
fn a_path_listed_twice_appears_once_at_its_newest() {
    let odd = newest(&list("odd.xbel"));
    let twice: Vec<&RecentItem> =
        odd.iter().filter(|item| item.path.get() == "/home/sam/odd/twice.txt").collect();
    assert_eq!(twice.len(), 1, "once, though file:/// and file://localhost/ name it three times");
    assert_eq!(twice[0].opened_at, utc("2026-10-07", "10:00:00", 0), "at its newest");
    assert_eq!(app_of(twice[0]), Some("Second"), "with that entry's app");
}

#[test]
fn equal_times_keep_the_files_order() {
    let odd = newest(&list("odd.xbel"));
    let ties: Vec<&str> = paths(&odd).into_iter().filter(|path| path.contains("/tie-")).collect();
    assert_eq!(ties, ["/home/sam/odd/tie-b.txt", "/home/sam/odd/tie-a.txt"], "b is written first");
}

#[test]
fn the_list_of_a_missing_or_broken_file_is_its_bookmarks() {
    assert_eq!(list("no-such-folder/recently-used.xbel").recent(10), Ok(Vec::new()), "missing");
    assert_eq!(list("empty.xbel").recent(10), Ok(Vec::new()), "empty");
    assert_eq!(list("cleared.xbel").recent(10), Ok(Vec::new()), "no bookmarks");
    assert_eq!(
        list("truncated.xbel").recent(10),
        Err(list("truncated.xbel").bookmarks().unwrap_err()),
        "the same error for a broken file"
    );
}

#[test]
fn the_file_is_xdg_data_homes_else_homes() {
    let root = fixtures();
    let home = root.join("home").join("sam");
    let xdg = root.join("xdg-data");
    let with = |pairs: &[(&str, &Path)]| {
        let pairs: Vec<(String, OsString)> =
            pairs.iter().map(|(name, value)| ((*name).to_owned(), value.into())).collect();
        let session = Session::from_vars(move |name| {
            pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
        });
        RecentlyUsed::for_session(&session)
    };
    let first_path = |list: Option<RecentlyUsed>| {
        list.expect("a data folder").recent(1).unwrap().first().map(|item| item.path.get().clone())
    };
    let screenshot =
        Some("/home/sam/Pictures/Screenshots/Screenshot_20261021_075830.png".to_owned());
    let newest_in_home =
        Some("/home/sam/src/eileen/crates/eileen-apps-linux/src/recent.rs".to_owned());
    assert_eq!(first_path(with(&[("HOME", &home)])), newest_in_home, "HOME's .local/share");
    assert_eq!(
        first_path(with(&[("XDG_DATA_HOME", &xdg), ("HOME", &home)])),
        screenshot,
        "XDG_DATA_HOME first"
    );
    assert_eq!(first_path(with(&[("XDG_DATA_HOME", &xdg)])), screenshot, "XDG_DATA_HOME alone");
    assert_eq!(
        first_path(with(&[("XDG_DATA_HOME", Path::new("")), ("HOME", &home)])),
        newest_in_home,
        "an empty XDG_DATA_HOME is unset"
    );
    assert_eq!(
        first_path(with(&[("XDG_DATA_HOME", Path::new("relative/share")), ("HOME", &home)])),
        newest_in_home,
        "a relative XDG_DATA_HOME is ignored"
    );
    assert!(with(&[]).is_none(), "neither");
    assert!(with(&[("HOME", Path::new("home/sam"))]).is_none(), "a relative HOME");
}

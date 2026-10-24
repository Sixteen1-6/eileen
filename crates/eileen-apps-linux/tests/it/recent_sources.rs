//! `LinuxRecent`, the `RecentSources` trait over `recently-used.xbel`: the fixture files of
//! `tests/fixtures/recent/` through the trait, and the app's name as it leaves `Untrusted`. What
//! the list holds is pinned by the `recent` tests: these pin the conversions.

use std::fmt::Write as _;

use eileen_apps_linux::{LinuxRecent, RecentlyUsed};
use eileen_contracts::ids::{Personal, Untrusted};
use eileen_contracts::tools::apps::FileKind;
use eileen_platform::{PlatformError, RecentItem, RecentSources};

use crate::open::Scratch;
use crate::recent::{fixtures, gtk, list};

/// A bookmark file with a file for each of `names`, opened by an app of that name as XML writes
/// it, each a day older than the one before.
fn opened_by(names: &[&str]) -> String {
    let mut file = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks">
"#,
    );
    for (n, name) in names.iter().enumerate() {
        let at = format!("2026-10-{:02}T07:58:30Z", 21_usize.saturating_sub(n));
        write!(
            file,
            r#"<bookmark href="file:///home/sam/{n}.txt" added="{at}" modified="{at}">
  <info><metadata owner="http://freedesktop.org"><bookmark:applications>
    <bookmark:application name="{name}" exec="app %u" modified="{at}" count="1"/>
  </bookmark:applications></metadata></info>
</bookmark>
"#
        )
        .unwrap();
    }
    file.push_str("</xbel>\n");
    file
}

#[test]
fn recent_lists_the_files_newest_first_as_the_traits_items() {
    let items = LinuxRecent::new(gtk()).recent(usize::MAX).unwrap();
    let theirs = gtk().recent(usize::MAX).unwrap();
    assert_eq!(items.len(), theirs.len(), "one item for each");
    for (item, their) in items.iter().zip(&theirs) {
        assert_eq!(item.path, their.path, "the path, still personal");
        assert_eq!(item.app.as_ref(), their.app.as_ref().map(Untrusted::get), "the app's name");
        assert_eq!((item.kind, item.opened_at), (their.kind, their.opened_at), "kind and time");
    }
    let newest = RecentItem {
        path: Personal::new("/home/sam/src/eileen/crates/eileen-apps-linux/src/recent.rs".into()),
        app: Some("Text Editor".to_owned()),
        kind: Some(FileKind::Code),
        opened_at: theirs[0].opened_at,
    };
    assert_eq!(items.first(), Some(&newest), "the newest first");
    assert_eq!(
        LinuxRecent::new(gtk()).recent(3).unwrap(),
        items[..3],
        "the limit keeps the newest"
    );
}

#[test]
fn an_apps_name_leaves_untrusted_sanitized() {
    let scratch = Scratch::new();
    let long = "a".repeat(600);
    let names = ["Text&#x202E; Editor&#x200F;", "&#x202E;&#x2066;&#xFEFF;", long.as_str()];
    let file = scratch.write("recently-used.xbel", &opened_by(&names));
    let items = LinuxRecent::new(RecentlyUsed::at(file)).recent(usize::MAX).unwrap();
    let apps: Vec<Option<&str>> = items.iter().map(|item| item.app.as_deref()).collect();
    let kept = "a".repeat(512);
    assert_eq!(
        apps,
        [Some("Text Editor"), None, Some(kept.as_str())],
        "no bidirectional or invisible characters, a name they made empty is none, at most 512"
    );
}

#[test]
fn the_lists_errors_are_platform_errors() {
    let what = "recently-used.xbel";
    let recent = |list: RecentlyUsed| LinuxRecent::new(list).recent(10);
    assert_eq!(recent(list("no-such-folder/recently-used.xbel")), Ok(Vec::new()), "missing");
    assert!(
        matches!(
            recent(list("not-xbel.xbel")),
            Err(PlatformError::Invalid { what: "recently-used.xbel", detail })
                if detail.starts_with("not an XBEL bookmark file near byte ")
        ),
        "another root"
    );
    assert_eq!(
        recent(RecentlyUsed::at(fixtures())),
        Err(PlatformError::Io { what, detail: "is a directory".to_owned() }),
        "a folder in the file's place"
    );
    let scratch = Scratch::new();
    assert_eq!(
        recent(RecentlyUsed::at(scratch.fifo("recently-used.xbel"))),
        Err(PlatformError::Invalid { what, detail: "not a file".to_owned() }),
        "a FIFO, refused unread"
    );
}

#[test]
fn watching_and_thumbnails_are_unavailable() {
    let recent = LinuxRecent::new(gtk());
    assert_eq!(
        recent.watch(&["/home/sam".to_owned()], Box::new(|_item| {})).err(),
        Some(PlatformError::Unavailable { what: "watching recent files" }),
        "until phase L2 brings inotify"
    );
    let item = recent.recent(1).unwrap().remove(0);
    assert_eq!(
        recent.thumbnail(&item, 256).err(),
        Some(PlatformError::Unavailable { what: "a recent file's thumbnail" }),
        "until phase L2 reads the thumbnail cache"
    );
}

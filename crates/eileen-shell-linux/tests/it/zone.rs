//! The local time zone read from `etc/localtime` under a root of each test's own, as a link into
//! its `usr/share/zoneinfo` or a copy, and what is refused there; and the zones of the machine the
//! tests run on.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::WallTime;
use eileen_shell_linux::SysRoot;
use eileen_shell_linux::zone::{
    MAX_TZIF_BYTES, ZoneError, ZoneOffsets, local_offsets, local_offsets_under,
};
use rustix::fs::{CWD, Mode, mkfifoat};

/// A root of one test's own under Cargo's temporary folder, with an empty `etc`, removed with
/// everything in it when the guard drops, a failing test included.
struct Root(PathBuf);

impl Root {
    fn new(test: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("zone").join(test);
        let _ignored = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("etc")).unwrap();
        Self(dir)
    }

    fn sys(&self) -> SysRoot {
        SysRoot::at(&self.0)
    }

    fn localtime(&self) -> PathBuf {
        self.0.join("etc").join("localtime")
    }

    /// Copies the fixture `zone` into the root's `usr/share/zoneinfo` and links `etc/localtime`
    /// to it, relatively, as an installer would with the root at `/`.
    fn link(&self, zone: &str) {
        let target = self.0.join("usr/share/zoneinfo").join(zone);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(fixture(zone), &target).unwrap();
        symlink(Path::new("../usr/share/zoneinfo").join(zone), self.localtime()).unwrap();
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        // Best effort: a leftover folder under the target folder is harmless.
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn fixture(zone: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/zoneinfo").join(zone)
}

fn offsets(initial_min: i32, changes: &[(i64, i32)]) -> ZoneOffsets {
    let changes = changes.iter().map(|(at, offset)| (WallTime(at * 1_000_000), *offset)).collect();
    ZoneOffsets { initial_min, changes }
}

#[test]
fn the_zone_etc_localtime_links_to_is_read() {
    let root = Root::new("link");
    root.link("Pacific/Chatham");
    // glibc 2.39 for Pacific/Chatham: +13:45 in the southern summer, +12:45 from April.
    let chatham = offsets(
        825,
        &[(1_775_311_200, 765), (1_790_431_200, 825), (1_806_760_800, 765), (1_821_880_800, 825)],
    );
    assert_eq!(local_offsets_under(&root.sys(), 2026), Ok(chatham), "2026 and 2027");
}

#[test]
fn a_copy_at_etc_localtime_is_read_as_a_link_is() {
    let root = Root::new("copy");
    fs::copy(fixture("Asia/Kolkata"), root.localtime()).unwrap();
    assert_eq!(local_offsets_under(&root.sys(), 2026), Ok(offsets(330, &[])), "+5:30 always");
    fs::copy(fixture("slim/America/Santiago"), root.localtime()).unwrap();
    // glibc 2.39 for America/Santiago, whose slim file has its rule alone after 2023.
    let santiago = offsets(
        -180,
        &[
            (1_775_358_000, -240),
            (1_788_667_200, -180),
            (1_806_807_600, -240),
            (1_820_116_800, -180),
        ],
    );
    assert_eq!(local_offsets_under(&root.sys(), 2026), Ok(santiago), "a slim file");
}

#[test]
fn no_file_or_a_link_to_nothing_is_missing() {
    let root = Root::new("missing");
    assert_eq!(local_offsets_under(&root.sys(), 2026), Err(ZoneError::Missing), "no file");
    symlink("../usr/share/zoneinfo/Mars/Olympus_Mons", root.localtime()).unwrap();
    assert_eq!(local_offsets_under(&root.sys(), 2026), Err(ZoneError::Missing), "a dangling link");
}

#[test]
fn a_folder_or_a_fifo_is_refused_without_waiting() {
    let root = Root::new("not-a-file");
    let not_a_file = Err(ZoneError::Unreadable { detail: "not a regular file".to_owned() });
    fs::create_dir_all(root.localtime()).unwrap();
    assert_eq!(local_offsets_under(&root.sys(), 2026), not_a_file, "a folder");
    fs::remove_dir(root.localtime()).unwrap();
    // No process writes to the FIFO: an open that waited for a writer would never return.
    mkfifoat(CWD, root.localtime(), Mode::RUSR | Mode::WUSR).unwrap();
    assert_eq!(local_offsets_under(&root.sys(), 2026), not_a_file, "a FIFO");
}

#[test]
fn a_file_over_the_limit_or_not_a_time_zone_file_is_malformed() {
    let root = Root::new("malformed");
    let mut file = fs::read(fixture("America/New_York")).unwrap();
    // Bytes after the footer are not read, so a file at the limit reads and one past it is refused.
    file.resize(usize::try_from(MAX_TZIF_BYTES).unwrap(), b'x');
    fs::write(root.localtime(), &file).unwrap();
    let at_the_limit = local_offsets_under(&root.sys(), 2026).unwrap();
    assert_eq!(at_the_limit.initial_min, -300, "a file at the limit reads");
    file.push(b'x');
    fs::write(root.localtime(), &file).unwrap();
    let over = Err(ZoneError::Malformed { reason: "it is over 64 KiB" });
    assert_eq!(local_offsets_under(&root.sys(), 2026), over, "one byte over");
    fs::write(root.localtime(), "America/New_York\n").unwrap();
    let error = local_offsets_under(&root.sys(), 2026).unwrap_err();
    assert!(matches!(error, ZoneError::Malformed { .. }), "a zone's name: {error:?}");
}

#[test]
fn no_error_names_the_root() {
    let root = Root::new("no-path");
    let mut errors = vec![local_offsets_under(&root.sys(), 2026).unwrap_err()];
    fs::create_dir_all(root.localtime()).unwrap();
    errors.push(local_offsets_under(&root.sys(), 2026).unwrap_err());
    fs::remove_dir(root.localtime()).unwrap();
    fs::write(root.localtime(), "TZif").unwrap();
    errors.push(local_offsets_under(&root.sys(), 2026).unwrap_err());
    let root_text = root.0.to_str().unwrap();
    for error in errors {
        let text = error.to_string();
        assert!(text.starts_with("/etc/localtime "), "names the fixed file: {text}");
        assert!(!text.contains(root_text) && !text.contains("zone/no-path"), "no root: {text}");
    }
}

#[test]
fn the_zones_of_this_machine_read() {
    // The machine's own zone: read, or missing where a container has none, never refused.
    match local_offsets(2026) {
        Ok(_) | Err(ZoneError::Missing) => {}
        Err(error) => panic!("this machine's /etc/localtime: {error}"),
    }
    // Every time zone file the machine has, where it has tzdata.
    let root = Root::new("installed");
    let mut folders = vec![PathBuf::from("/usr/share/zoneinfo")];
    let mut read = 0;
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for path in entries.map(|entry| entry.unwrap().path()) {
            let meta = fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                folders.push(path);
            } else if meta.is_file() && fs::read(&path).unwrap().starts_with(b"TZif") {
                fs::copy(&path, root.localtime()).unwrap();
                for year in [1970, 2026, 2037, 2040] {
                    let read_back = local_offsets_under(&root.sys(), year);
                    assert!(read_back.is_ok(), "{}: {read_back:?}", path.display());
                }
                read += 1;
            }
        }
    }
    assert!(read == 0 || read > 300, "a tzdata install has hundreds of zones: {read}");
}

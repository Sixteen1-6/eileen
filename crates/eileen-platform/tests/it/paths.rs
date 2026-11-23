//! The two rules of form every path probe applies before it reads anything (BUILD.md section
//! 2.12, decision D86): which paths `PathProbe::full_path` takes, and which paths a probe opens.

use eileen_platform::{PlatformError, path_allow_listed, path_fully_qualified};

const NOT_QUALIFIED: PlatformError =
    PlatformError::Refused { what: "path", reason: "not fully qualified" };

const OFF_ALLOW_LIST: PlatformError =
    PlatformError::Refused { what: "path", reason: "not a drive or volume path" };

const VOLUME: &str = r"\\?\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e6963}";

#[test]
fn a_drive_and_root_a_unc_share_and_a_prefixed_path_are_fully_qualified() {
    let qualified = [
        r"C:\",
        r"C:\job\src\main.rs",
        "c:/job/src",
        r"z:\..\x",
        r"\\fileserver\team",
        r"\\fileserver\team\plan.docx",
        "//fileserver/team/plan.docx",
        r"\\localhost\c$\x",
        r"\\?\C:\job",
        r"\\?\UNC\fileserver\team",
        r"\\?\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e6963}\x",
        r"\??\C:\job",
        r"\??\pipe\x",
        r"\\.\pipe\x",
        r"\\.\NUL",
        "//./C:/job",
    ];
    for path in qualified {
        assert_eq!(path_fully_qualified(path), Ok(()), "{path}");
    }
}

#[test]
fn a_path_that_needs_a_current_directory_is_not_fully_qualified() {
    let refused = [
        "",
        "main.rs",
        r"src\main.rs",
        r".\src",
        r"..\other",
        r"\rooted\with\no\drive",
        "/rooted/with/no/drive",
        "C:",
        "C:drive-relative",
        r"C:drive\relative",
        r"1:\not\a\letter",
        r"\\",
        r"\\host",
        r"\\host\",
        r"\\\share",
        r"\??\",
        r"\?\C:\x",
        "CONIN$",
        "NUL",
    ];
    for path in refused {
        assert_eq!(path_fully_qualified(path), Err(NOT_QUALIFIED), "{path}");
    }
}

#[test]
fn each_drive_and_volume_spelling_is_on_the_allow_list() {
    let volume_root = format!(r"{VOLUME}\");
    let volume_file = format!(r"{VOLUME}\job\a.txt");
    let upper = r"\\?\VOLUME{4C1B02C1-D990-11DC-99AE-806E6F6E6963}\x";
    let allowed = [
        r"C:\",
        r"C:\job\a.txt",
        r"z:\job",
        r"\\?\C:\job\a.txt",
        r"\??\C:\job\a.txt",
        r"\\.\C:\job\a.txt",
        r"\\?\d:\",
        // A stream of a file on a drive is still that drive's file.
        r"C:\job\a.txt:stream",
        volume_root.as_str(),
        volume_file.as_str(),
        upper,
    ];
    for path in allowed {
        assert_eq!(path_allow_listed(path), Ok(()), "{path}");
    }
}

#[test]
fn a_pipe_a_device_and_every_unc_spelling_are_off_the_allow_list() {
    let bare_volume = VOLUME;
    let short_guid = r"\\?\Volume{4c1b02c1-d990-11dc-99ae}\x";
    let not_hex = r"\\?\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e696g}\x";
    let refused = [
        // A pipe under each of the three prefixes, and the other devices.
        r"\\.\pipe\eileen-dispatch",
        r"\\?\pipe\eileen-dispatch",
        r"\??\pipe\eileen-dispatch",
        r"\\?\GLOBALROOT\Device\NamedPipe\x",
        r"\\.\GLOBALROOT\Device\HarddiskVolume3\x",
        r"\\.\CONIN$",
        "CONIN$",
        r"\\.\NUL",
        r"\\.\PhysicalDrive0",
        // Every UNC spelling.
        r"\\fileserver\team\plan.docx",
        "//fileserver/team/plan.docx",
        r"\\localhost\c$\x",
        r"\\?\UNC\fileserver\team\x",
        r"\\.\UNC\fileserver\team\x",
        r"\??\UNC\fileserver\team\x",
        // A drive or volume with no root is the raw volume, not a folder.
        r"\\.\C:",
        r"\\?\C:",
        "C:",
        bare_volume,
        // A volume is spelled only under `\\?\`, with a whole GUID.
        r"\\.\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e6963}\x",
        r"\??\Volume{4c1b02c1-d990-11dc-99ae-806e6f6e6963}\x",
        short_guid,
        not_hex,
        // Forms `full_path` has not yet made into one of the above.
        "C:/job/a.txt",
        "//?/C:/job",
        r"C:drive-relative",
        r"\rooted",
        r"src\main.rs",
        r"1:\x",
        r"\\?\\C:\x",
        "",
    ];
    for path in refused {
        assert_eq!(path_allow_listed(path), Err(OFF_ALLOW_LIST), "{path}");
    }
}

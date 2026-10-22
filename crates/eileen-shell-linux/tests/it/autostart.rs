//! Eileen's autostart entry, written and removed in a folder of each test's own.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use eileen_shell_linux::{AutostartError, set_run_at_sign_in};

/// A folder of one test's own under Cargo's temporary folder, removed with everything in it when
/// the guard drops, a failing test included.
struct Scratch(PathBuf);

impl Scratch {
    /// A fresh, empty folder for the test `test`.
    fn new(test: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("autostart").join(test);
        let _ignored = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The autostart folder the test uses, inside the scratch folder and not yet made.
    fn autostart(&self) -> PathBuf {
        self.0.join("autostart")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a leftover folder under the target folder is harmless.
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

/// Reads an `Exec` value back as the Desktop Entry Specification says: the string escapes
/// first, then the quoting.
fn unquote(value: &str) -> String {
    let mut unescaped = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            unescaped.push(match chars.next().unwrap() {
                's' => ' ',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                other => panic!("an undefined string escape: \\{other}"),
            });
        } else {
            unescaped.push(c);
        }
    }
    let inner = unescaped.strip_prefix('"').unwrap().strip_suffix('"').unwrap();
    let mut argument = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let escaped = chars.next().unwrap();
                assert!(matches!(escaped, '"' | '`' | '$' | '\\'), "only these are escaped");
                argument.push(escaped);
            }
            '"' | '`' | '$' => panic!("{c} unescaped inside the quotes"),
            _ => argument.push(c),
        }
    }
    argument
}

#[test]
fn the_autostart_entry_is_written_and_removed() {
    let scratch = Scratch::new("written_and_removed");
    let dir = scratch.autostart();
    let program = Path::new("/opt/Eileen Apps/eileen");
    set_run_at_sign_in(&dir, "eileen", program, true).unwrap();
    let path = dir.join("eileen.desktop");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "[Desktop Entry]\nType=Application\nName=Eileen\nExec=\"/opt/Eileen Apps/eileen\"\n\
         X-GNOME-Autostart-enabled=true\n",
        "the entry, in a folder made for it"
    );
    set_run_at_sign_in(&dir, "eileen", Path::new("/usr/bin/eileen"), true).unwrap();
    assert!(
        fs::read_to_string(&path).unwrap().contains("\nExec=\"/usr/bin/eileen\"\n"),
        "rewritten in place"
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no partial file left beside it");
    set_run_at_sign_in(&dir, "eileen", program, false).unwrap();
    assert!(!path.exists(), "removed");
    set_run_at_sign_in(&dir, "eileen", Path::new("relative"), false).unwrap();
    assert!(!path.exists(), "already off, and the program is not checked to turn it off");
}

#[test]
fn the_program_is_quoted_by_the_desktop_entry_rules() {
    let scratch = Scratch::new("quoting");
    let dir = scratch.autostart();
    let program = "/opt/a $b \"c\" `d` 'e' ~f;g&h|i<j>k*l?m#n(o)/eileen";
    set_run_at_sign_in(&dir, "eileen", Path::new(program), true).unwrap();
    let text = fs::read_to_string(dir.join("eileen.desktop")).unwrap();
    let exec = text.lines().find_map(|line| line.strip_prefix("Exec=")).unwrap();
    assert_eq!(
        exec, r#""/opt/a \\$b \\"c\\" \\`d\\` 'e' ~f;g&h|i<j>k*l?m#n(o)/eileen""#,
        "in double quotes; the escaping backslashes written twice"
    );
    assert_eq!(unquote(exec), program, "it reads back as the path");
}

#[test]
fn programs_and_ids_autostart_cannot_carry_are_refused() {
    let scratch = Scratch::new("refused");
    let dir = scratch.autostart();
    let refused =
        |id: &str, program: &Path| set_run_at_sign_in(&dir, id, program, true).unwrap_err();
    let program_refused = |program: &Path| {
        matches!(refused("eileen", program), AutostartError::Invalid { what: "program path", .. })
    };
    assert!(program_refused(Path::new("eileen")), "a relative path");
    assert!(program_refused(Path::new("/opt/50%/eileen")), "a percent sign");
    assert!(program_refused(Path::new("/opt/a\\b/eileen")), "a backslash");
    assert!(program_refused(Path::new("/opt/a\nb/eileen")), "a newline");
    assert!(program_refused(Path::new(OsStr::from_bytes(b"/opt/\xff/eileen"))), "not UTF-8");
    for id in ["", ".eileen", "../eileen", "a/b", "ei leen"] {
        let error = refused(id, Path::new("/usr/bin/eileen"));
        assert!(matches!(error, AutostartError::Invalid { what: "autostart ID", .. }), "{id:?}");
    }
    assert_eq!(
        refused("eileen", Path::new("eileen")).to_string(),
        "invalid program path: not absolute",
        "the message names the argument as a bare noun"
    );
    assert!(!dir.exists(), "nothing was written");
}

#[test]
fn an_io_failure_names_the_entry_and_not_its_path() {
    let scratch = Scratch::new("io_failure");
    let dir = scratch.autostart();
    fs::write(&dir, "a file where the folder belongs").unwrap();
    let error = set_run_at_sign_in(&dir, "eileen", Path::new("/usr/bin/eileen"), true).unwrap_err();
    assert!(matches!(error, AutostartError::Io { what: "autostart entry", .. }), "{error}");
    assert!(!error.to_string().contains("io_failure"), "no path in the error: {error}");
}

#[test]
fn the_scratch_folder_goes_with_its_guard() {
    let scratch = Scratch::new("guard");
    let dir = scratch.autostart();
    set_run_at_sign_in(&dir, "eileen", Path::new("/usr/bin/eileen"), true).unwrap();
    let root = scratch.0.clone();
    drop(scratch);
    assert!(!root.exists(), "the folder and the entry in it are removed");
}

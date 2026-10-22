//! Starting Eileen at sign-in on Linux: [`set_run_at_sign_in`] writes or removes Eileen's XDG
//! autostart entry, as `ShellHost::set_run_at_sign_in` adds or removes the `HKCU` Run entry on
//! Windows (BUILD.md sections 2.1 and 3.4; docs/LINUX.md section 4).
//!
//! The entry is `<id>.desktop` in the user's autostart folder, `$XDG_CONFIG_HOME/autostart`
//! ([the autostart specification]), where `XDG_CONFIG_HOME` falls back to `~/.config` when it is
//! unset, empty or relative ([the base directory specification]); [`default_autostart_dir`]
//! finds it. The entry reads:
//!
//! ```text
//! [Desktop Entry]
//! Type=Application
//! Name=Eileen
//! Exec="/usr/bin/eileen"
//! X-GNOME-Autostart-enabled=true
//! ```
//!
//! `Exec` is the program's path quoted by the Desktop Entry Specification's rules ([the Exec
//! key]): in double quotes, with `"`, `` ` ``, `$` and `\` escaped by a backslash and `%` written
//! `%%`, and then the string escapes every value takes, so that a backslash in the path is four
//! in the file and a dollar sign `\\$`. Quoting always is allowed ("Arguments may be quoted in
//! whole") and needs no list of reserved characters.
//!
//! Turning it off removes the file, and an entry that is already absent is off. The program's
//! path is checked only when the entry is written, so any path can turn it off. A folder the user
//! may not change refuses the write and the removal alike ([`AutostartError::Refused`]), and the
//! entry stays as it was.
//!
//! # Traps
//!
//! - The program must be an absolute path in UTF-8 with no `%`, backslash or control character.
//!   systemd's autostart generator, which starts the entries of KDE Plasma's sessions
//!   (`plasma-workspace.target` wants `xdg-desktop-autostart.target`), reads those two characters
//!   differently from the specification: it keeps `%%` in the program's path, and unescapes each
//!   argument a second time as a C string ([xdg-autostart-service.c]). Such a path would be
//!   written correctly and then fail at the next sign-in, so it is refused when the entry is
//!   written.
//! - A sign-in must never read half an entry: the file is written beside its place, as a hidden
//!   `.partial` file no desktop reads, and renamed into it.
//!
//! [the autostart specification]: https://specifications.freedesktop.org/autostart/latest/
//! [the base directory specification]: https://specifications.freedesktop.org/basedir/latest/
//! [the Exec key]: https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
//! [xdg-autostart-service.c]: https://github.com/systemd/systemd/blob/main/src/xdg-autostart-generator/xdg-autostart-service.c

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::AutostartError;

/// What the errors name.
const ENTRY: &str = "autostart entry";
const ID: &str = "autostart ID";
const PROGRAM: &str = "program path";

/// Where the user's autostart entries live: `$XDG_CONFIG_HOME/autostart`, or
/// `~/.config/autostart` when `XDG_CONFIG_HOME` is unset, empty or relative (the XDG base
/// directory rules). `None` without a home folder.
#[must_use]
pub fn default_autostart_dir() -> Option<PathBuf> {
    autostart_dir_from(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))
}

/// [`default_autostart_dir`] from the two variables' values.
fn autostart_dir_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let absolute =
        |value: Option<OsString>| value.map(PathBuf::from).filter(|path| path.is_absolute());
    absolute(xdg_config_home)
        .or_else(|| absolute(home).map(|home| home.join(".config")))
        .map(|config| config.join("autostart"))
}

/// When `on`, writes `<dir>/<id>.desktop`, an entry that starts `program` when the user signs in,
/// creating `dir` if needed; otherwise removes it. `dir` is the autostart folder,
/// [`default_autostart_dir`] on a real session; `id` is the desktop file ID without `.desktop`;
/// `program` is checked only when the entry is written. The file is written beside its place and
/// renamed into it, so a sign-in never reads half an entry.
pub fn set_run_at_sign_in(
    dir: &Path,
    id: &str,
    program: &Path,
    on: bool,
) -> Result<(), AutostartError> {
    check_id(id)?;
    let path = dir.join(format!("{id}.desktop"));
    if !on {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AutostartError::io(ENTRY)(error)),
        };
    }
    let text = entry(program)?;
    std::fs::create_dir_all(dir).map_err(AutostartError::io(ENTRY))?;
    let partial = dir.join(format!(".{id}.desktop.partial"));
    std::fs::write(&partial, text).map_err(AutostartError::io(ENTRY))?;
    std::fs::rename(&partial, &path).map_err(|error| {
        // Best effort: a leftover partial file is not a `.desktop` file, so nothing reads it.
        let _ignored = std::fs::remove_file(&partial);
        AutostartError::io(ENTRY)(error)
    })
}

/// The entry's text for `program`.
fn entry(program: &Path) -> Result<String, AutostartError> {
    let invalid =
        |detail: &str| AutostartError::Invalid { what: PROGRAM, detail: detail.to_owned() };
    if !program.is_absolute() {
        return Err(invalid("not absolute"));
    }
    let program = program.to_str().ok_or_else(|| invalid("not UTF-8"))?;
    if program.contains(['%', '\\']) {
        return Err(invalid("holds % or a backslash, which systemd's autostart reads differently"));
    }
    if program.chars().any(char::is_control) {
        return Err(invalid("holds a control character"));
    }
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Eileen\nExec={}\nX-GNOME-Autostart-enabled=true\n",
        exec_value(program)
    ))
}

/// The `Exec` value that runs `program` with no arguments: the path quoted by the Desktop Entry
/// Specification's rules, then the string escapes applied over the quoting (`\\` for a backslash,
/// `\n`, `\t` and `\r` for the control characters that have one).
fn exec_value(program: &str) -> String {
    let mut quoted = String::with_capacity(program.len() + 2);
    quoted.push('"');
    for c in program.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    let mut value = String::with_capacity(quoted.len());
    for c in quoted.chars() {
        match c {
            '\\' => value.push_str("\\\\"),
            '\n' => value.push_str("\\n"),
            '\t' => value.push_str("\\t"),
            '\r' => value.push_str("\\r"),
            _ => value.push(c),
        }
    }
    value
}

/// Checks that `id` names a file in the autostart folder: ASCII letters, digits, `.`, `_` and
/// `-`, not starting with a dot (the shape of a desktop file ID).
fn check_id(id: &str) -> Result<(), AutostartError> {
    let invalid = |detail: &str| AutostartError::Invalid { what: ID, detail: detail.to_owned() };
    if id.is_empty() {
        return Err(invalid("empty"));
    }
    if id.starts_with('.') {
        return Err(invalid("starts with a dot"));
    }
    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
        return Err(invalid("not letters, digits, '.', '_' and '-'"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{autostart_dir_from, exec_value};

    #[test]
    fn the_autostart_folder_follows_the_xdg_rules() {
        let some = |text: &str| Some(text.into());
        assert_eq!(
            autostart_dir_from(some("/data/config"), some("/home/sam")),
            Some(PathBuf::from("/data/config/autostart")),
            "XDG_CONFIG_HOME first"
        );
        assert_eq!(
            autostart_dir_from(None, some("/home/sam")),
            Some(PathBuf::from("/home/sam/.config/autostart")),
            "the default under HOME"
        );
        assert_eq!(
            autostart_dir_from(some("relative/config"), some("/home/sam")),
            Some(PathBuf::from("/home/sam/.config/autostart")),
            "a relative XDG_CONFIG_HOME is ignored"
        );
        assert_eq!(
            autostart_dir_from(some(""), some("/home/sam")),
            Some(PathBuf::from("/home/sam/.config/autostart")),
            "an empty XDG_CONFIG_HOME is ignored"
        );
        assert_eq!(autostart_dir_from(None, some("relative")), None, "a relative HOME");
        assert_eq!(autostart_dir_from(None, None), None, "no home");
    }

    #[test]
    fn the_exec_value_follows_the_specifications_quoting() {
        assert_eq!(exec_value("/usr/bin/eileen"), "\"/usr/bin/eileen\"", "quoted in whole");
        assert_eq!(
            exec_value("/opt/My Apps/eileen"),
            "\"/opt/My Apps/eileen\"",
            "a space needs only the quotes"
        );
        // The specification's own examples: a literal backslash is four in the file, a literal
        // dollar sign `\\$`.
        assert_eq!(exec_value("/a\\b"), "\"/a\\\\\\\\b\"", "a backslash");
        assert_eq!(exec_value("/a$b"), "\"/a\\\\$b\"", "a dollar sign");
        assert_eq!(exec_value("/a\"b`c"), "\"/a\\\\\"b\\\\`c\"", "a double quote and a backtick");
        assert_eq!(exec_value("/a'b;c&d|e"), "\"/a'b;c&d|e\"", "reserved, but literal in quotes");
        assert_eq!(exec_value("/50%/eileen"), "\"/50%%/eileen\"", "a literal percent");
        assert_eq!(exec_value("/a\tb\nc"), "\"/a\\tb\\nc\"", "string escapes");
    }
}

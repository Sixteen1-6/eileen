//! Entry point for the fuzz target this crate owns (BUILD.md section 4.7): `desktop_entry`, a
//! desktop entry file through the parser and the index's reading of it, since apps and users
//! write these files. The seed corpus under `fuzz/corpus/desktop_entry/` replays through it as
//! an ordinary test (`it::fuzz_corpus`). It returns `Ok` for every input that keeps the
//! invariants it guards and `Err` naming the broken one; it never panics. The test and the
//! cargo-fuzz target turn an `Err` into a failure.
//!
//! Bytes are decoded lossily, as `uia_text` decodes them, so every input reaches the parser. An
//! absolute `TryExec` is looked up on the machine the target runs on, which can only move an
//! entry between listed and hidden. The cargo-fuzz target itself
//! (`fuzz/fuzz_targets/desktop_entry.rs`) waits for WP0's `fuzz/Cargo.toml`. This crate builds
//! only on Linux, so the corpus replays only there, which is where fuzzing runs.

use std::path::Path;

use crate::Session;
use crate::entry::{DesktopEntry, Meaning, interpret};
use crate::keyfile::{KeyFile, Line, list, locale_keys, parse, unescape};

/// The desktop file ID the target reads its file as.
const ID: &str = "org.example.Fuzzed.desktop";

/// Where the target says the file is.
const PATH: &str = "/usr/share/applications/org.example.Fuzzed.desktop";

/// The escapes of a string value, as written.
const STRING_ESCAPES: [&str; 4] = [r"\s", r"\n", r"\t", r"\r"];

/// A desktop entry file. Its text as a string value, and its lines as a list, written with the
/// format's escapes, read back as written. Parsed, it is bounded by the input, its keys and
/// group names stay in the format's alphabet, its values resolve their escapes without growing,
/// and written back it parses to the same file. Read by the index, an app it lists is what the
/// file says: the ID it was found at, the name of the `Name` line its locales pick,
/// `Type=Application`, not `Hidden`, and an `Exec` line or D-Bus activation to start it.
pub fn desktop_entry(data: &[u8]) -> Result<(), &'static str> {
    let text = String::from_utf8_lossy(data).into_owned();
    round_trips(&text)?;
    let Some(file) = parse(&text) else {
        return Ok(());
    };
    check_parsed(&file, &text)?;
    reads_back(&file)?;
    let session = Session {
        data_home: None,
        data_dirs: Vec::new(),
        locales: locale_keys("de_DE.UTF-8@euro"),
        desktops: vec!["GNOME".to_owned()],
        path: Vec::new(),
    };
    match interpret(&file, ID, Path::new(PATH), false, &session) {
        Some(Meaning::App(app)) => check_app(&file, &app, &session.locales),
        Some(Meaning::Masked) | None => Ok(()),
    }
}

/// `text` as a string value, and its lines as the items of a list, written with the format's
/// escapes by this module's own writer, resolve back to what was written: no escape is left
/// unresolved or resolved twice. A wrong `unescape` or `list` cannot agree with the writer the
/// way it agrees with itself.
fn round_trips(text: &str) -> Result<(), &'static str> {
    if unescape(&escape(text)) != text {
        return Err("a string written with its escapes did not resolve back to itself");
    }
    let items: Vec<&str> = text.lines().filter(|item| !item.is_empty()).collect();
    let written: Vec<String> = items.iter().map(|item| escape(item).replace(';', r"\;")).collect();
    if list(&written.join(";")) != items {
        return Err("a list written with its escapes did not split back into its items");
    }
    Ok(())
}

/// The parsed file holds no more groups and keys than the input has lines and no more text
/// than the input, and its keys, group names and values are well formed.
fn check_parsed(file: &KeyFile, text: &str) -> Result<(), &'static str> {
    let items: usize = file.groups.iter().map(|(_, lines)| 1 + lines.len()).sum();
    if items > text.lines().count() {
        return Err("a desktop entry gave more groups and keys than it has lines");
    }
    let kept: usize = file
        .groups
        .iter()
        .map(|(name, lines)| name.len() + lines.iter().map(line_len).sum::<usize>())
        .sum();
    if kept > text.len() {
        return Err("a desktop entry's groups and keys hold more text than the input");
    }
    for (name, lines) in &file.groups {
        if !is_group_name(name) {
            return Err("a group name is outside the format's alphabet");
        }
        for line in lines {
            if !is_key(&line.key) {
                return Err("a key is outside the format's alphabet");
            }
            check_value(&line.raw)?;
        }
    }
    Ok(())
}

/// The text a line keeps.
fn line_len(line: &Line) -> usize {
    line.key.len() + line.locale.as_ref().map_or(0, String::len) + line.raw.len()
}

/// A key as the specification allows it: `A-Za-z0-9-`, and not empty.
fn is_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A group name as the specification allows it, read as `GLib` reads it: not empty, and no
/// `[`, `]` or control character.
fn is_group_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| matches!(c, '[' | ']') || c.is_ascii_control())
}

/// A value's escapes resolve: as a string and as a list it is no longer than written, and when
/// it holds no escaped backslash, none of the escapes the format resolves is left in it. An
/// escaped backslash can rightly leave `\s` behind, so `round_trips` covers those values.
fn check_value(raw: &str) -> Result<(), &'static str> {
    let value = unescape(raw);
    let items = list(raw);
    if value.len() > raw.len() || items.iter().map(String::len).sum::<usize>() > raw.len() {
        return Err("resolving a value's escapes made it longer");
    }
    if raw.contains(r"\\") {
        return Ok(());
    }
    if STRING_ESCAPES.iter().any(|escape| value.contains(escape)) {
        return Err("a string value kept an escape the format resolves");
    }
    let kept = |item: &String| {
        item.contains(r"\;") || STRING_ESCAPES.iter().any(|escape| item.contains(escape))
    };
    if items.iter().any(kept) {
        return Err("a list item kept an escape the format resolves");
    }
    Ok(())
}

/// Writes `file` back, each value escaped as a writer escapes a string, and parses it again:
/// the groups, keys, locales and values must come back unchanged.
fn reads_back(file: &KeyFile) -> Result<(), &'static str> {
    let mut text = String::new();
    for (name, lines) in &file.groups {
        text.push('[');
        text.push_str(name);
        text.push_str("]\n");
        for line in lines {
            text.push_str(&write_line(line));
        }
    }
    let again = parse(&text).ok_or("a desktop entry written back does not parse")?;
    let same = again.groups.len() == file.groups.len()
        && again.groups.iter().zip(&file.groups).all(|((name, lines), (was_name, was_lines))| {
            name == was_name
                && lines.len() == was_lines.len()
                && lines.iter().zip(was_lines).all(|(line, was)| {
                    line.key == was.key
                        && line.locale == was.locale
                        && unescape(&line.raw) == unescape(&was.raw)
                })
        });
    if same { Ok(()) } else { Err("a desktop entry written back parses to a different file") }
}

/// One `Key[locale]=value` line, its value escaped.
fn write_line(line: &Line) -> String {
    let locale = line.locale.as_ref().map(|locale| format!("[{locale}]")).unwrap_or_default();
    format!("{}{locale}={}\n", line.key, escape(&unescape(&line.raw)))
}

/// `value` as a writer escapes a string: a backslash, space, tab, newline and carriage return
/// as `\\`, `\s`, `\t`, `\n` and `\r`.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            ' ' => out.push_str(r"\s"),
            '\t' => out.push_str(r"\t"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            _ => out.push(c),
        }
    }
    out
}

/// An app the index lists is what its file says.
fn check_app(file: &KeyFile, app: &DesktopEntry, locales: &[String]) -> Result<(), &'static str> {
    if app.id != ID || app.path.get() != Path::new(PATH) {
        return Err("an app does not carry the ID and path it was found at");
    }
    let lines =
        file.group("Desktop Entry").ok_or("an app came from a file with no Desktop Entry group")?;
    // The specification's order: the first `Name` line of the most specific locale that has
    // one, else the first unlocalized one.
    let name_line = |locale: Option<&String>| {
        lines.iter().find(|line| line.key == "Name" && line.locale.as_ref() == locale)
    };
    let picked =
        locales.iter().find_map(|locale| name_line(Some(locale))).or_else(|| name_line(None));
    if picked.is_none_or(|line| unescape(&line.raw) != app.name) {
        return Err("an app's name is not the Name line its locales pick");
    }
    // The first unlocalized line of a key decides, as the index reads every key.
    let says = |key: &str, value: &str| {
        lines
            .iter()
            .find(|line| line.key == key && line.locale.is_none())
            .is_some_and(|line| line.raw.trim() == value)
    };
    if !says("Type", "Application") {
        return Err("an app's file does not say Type=Application");
    }
    if says("Hidden", "true") {
        return Err("a Hidden entry is listed");
    }
    let startable = app.exec.as_ref().map_or(app.dbus_activatable, |exec| !exec.get().is_empty());
    if !startable {
        return Err("an app has no Exec line and is not started over D-Bus");
    }
    Ok(())
}

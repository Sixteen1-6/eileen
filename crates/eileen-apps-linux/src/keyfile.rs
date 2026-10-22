//! The desktop entry file format (the XDG Desktop Entry Specification, version 1.5): groups of
//! `Key=Value` lines, keys with an optional `[locale]`, and the escapes of string values.
//!
//! Eileen reads the format itself: the `freedesktop-desktop-entry` crate is MPL-2.0, which
//! `deny.toml` does not allow (docs/LINUX.md section 4).

/// One `Key[locale]=value` line of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    /// The key, such as `Name`.
    pub(crate) key: String,
    /// The locale in brackets, such as `de_DE@euro`, for a localized key.
    pub(crate) locale: Option<String>,
    /// The value as written, escapes unresolved.
    pub(crate) raw: String,
}

/// A parsed file: its groups in order, each with its lines in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyFile {
    /// `(group name, lines)`.
    pub(crate) groups: Vec<(String, Vec<Line>)>,
}

impl KeyFile {
    /// The lines of the first group called `name`.
    pub(crate) fn group(&self, name: &str) -> Option<&[Line]> {
        self.groups.iter().find(|(group, _)| group == name).map(|(_, lines)| lines.as_slice())
    }
}

/// Parses a desktop entry file. `None` when a line before the first group, a group name the
/// format does not allow, or a line that is neither a group header, a `Key=Value` line, a
/// comment nor blank, makes it malformed.
pub(crate) fn parse(text: &str) -> Option<KeyFile> {
    let mut file = KeyFile::default();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) =
            line.strip_prefix('[').and_then(|rest| rest.trim_end().strip_suffix(']'))
        {
            if !is_group_name(name) {
                return None;
            }
            file.groups.push((name.to_owned(), Vec::new()));
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let (_, lines) = file.groups.last_mut()?;
        let key = key.trim_end();
        let (key, locale) = match key.split_once('[') {
            Some((key, rest)) => (key, Some(rest.strip_suffix(']')?.to_owned())),
            None => (key, None),
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return None;
        }
        lines.push(Line { key: key.to_owned(), locale, raw: value.trim_start().to_owned() });
    }
    Some(file)
}

/// Whether `name` may name a group: not empty, and none of the `[`, `]` and control characters
/// the specification excludes. Characters beyond ASCII are allowed, as `GLib` allows them.
fn is_group_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| matches!(c, '[' | ']') || c.is_ascii_control())
}

/// Resolves a string value's escapes: `\s`, `\n`, `\t`, `\r` and `\\`. An unknown escape is
/// kept as written.
pub(crate) fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            // An escaped backslash, or a lone one at the end, is one backslash.
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Splits a list value at its unescaped semicolons (`\;` is a semicolon inside an item),
/// resolving each item's escapes and dropping empty items.
pub(crate) fn list(raw: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(';') => item.push(';'),
                Some(other) => {
                    item.push('\\');
                    item.push(other);
                }
                None => item.push('\\'),
            },
            ';' => items.push(std::mem::take(&mut item)),
            other => item.push(other),
        }
    }
    items.push(item);
    items.into_iter().map(|item| unescape(&item)).filter(|item| !item.is_empty()).collect()
}

/// The user's locale for messages, as the C library chooses it: `LC_ALL`, then `LC_MESSAGES`,
/// then `LANG`; `None` for `C`, `POSIX` or nothing set.
pub(crate) fn messages_locale(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> Option<String> {
    let value =
        [lc_all, lc_messages, lang].into_iter().flatten().find(|value| !value.is_empty())?;
    (value != "C" && value != "POSIX" && !value.starts_with("C.")).then(|| value.to_owned())
}

/// The keys to try for `locale` (`lang_COUNTRY.ENCODING@MODIFIER`), most specific first, as the
/// specification orders them: `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`,
/// `lang`. The encoding is ignored.
pub(crate) fn locale_keys(locale: &str) -> Vec<String> {
    let (rest, modifier) = match locale.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier)),
        None => (locale, None),
    };
    let rest = rest.split_once('.').map_or(rest, |(before, _)| before);
    let (lang, country) = match rest.split_once('_') {
        Some((lang, country)) => (lang, Some(country)),
        None => (rest, None),
    };
    let mut keys = Vec::new();
    if let (Some(country), Some(modifier)) = (country, modifier) {
        keys.push(format!("{lang}_{country}@{modifier}"));
    }
    if let Some(country) = country {
        keys.push(format!("{lang}_{country}"));
    }
    if let Some(modifier) = modifier {
        keys.push(format!("{lang}@{modifier}"));
    }
    keys.push(lang.to_owned());
    keys
}

/// The raw value of `key` in `lines` for the first matching locale key, else the unlocalized
/// one.
pub(crate) fn localized<'l>(lines: &'l [Line], key: &str, locales: &[String]) -> Option<&'l str> {
    locales
        .iter()
        .find_map(|locale| {
            lines.iter().find(|line| line.key == key && line.locale.as_deref() == Some(locale))
        })
        .or_else(|| lines.iter().find(|line| line.key == key && line.locale.is_none()))
        .map(|line| line.raw.as_str())
}

#[cfg(test)]
mod tests {
    use super::{list, locale_keys, localized, messages_locale, parse, unescape};

    const FILE: &str = "# A comment\n[Desktop Entry]\nType=Application\nName=Calculator\n\
                        Name[de]=Taschenrechner\nName[pt_BR]=Calculadora\n  Exec = gnome-calculator\n\
                        \n[Desktop Action new]\nName=New\n";

    #[test]
    fn groups_keys_and_locales_parse() {
        let file = parse(FILE).unwrap();
        assert_eq!(file.groups.len(), 2, "two groups");
        let entry = file.group("Desktop Entry").unwrap();
        assert_eq!(entry.len(), 5, "five lines");
        assert_eq!(entry[2].locale.as_deref(), Some("de"), "a localized key");
        assert_eq!(entry[4].key, "Exec", "spaces around = are ignored");
        assert_eq!(entry[4].raw, "gnome-calculator", "and so is the space after it");
        assert!(file.group("Desktop Action new").is_some(), "an action group");
    }

    #[test]
    fn malformed_files_are_refused() {
        assert!(parse("Name=Orphan\n[Desktop Entry]\n").is_none(), "a key before any group");
        assert!(parse("[Desktop Entry]\nnot a key line\n").is_none(), "no equals sign");
        assert!(parse("[Desktop Entry]\nName[de=Broken\n").is_none(), "an unclosed locale");
        assert!(parse("[Desktop Entry]\nBad Key=1\n").is_none(), "a space in a key");
        assert!(parse("[Desktop]Entry]\nName=x\n").is_none(), "a bracket in a group name");
        assert!(parse("[]\nName=x\n").is_none(), "an empty group name");
        assert!(parse("[Desktop\u{1}Entry]\n").is_none(), "a control character in a group name");
        assert!(parse("[Desktop Action \u{d6}ffnen]\n").is_some(), "beyond ASCII, as GLib allows");
    }

    #[test]
    fn escapes_and_lists_resolve() {
        assert_eq!(unescape(r"Foo\sBar\\Baz\tTab"), "Foo Bar\\Baz\tTab", "escapes");
        assert_eq!(unescape(r"keep\x"), r"keep\x", "an unknown escape is kept");
        assert_eq!(
            list(r"calc;math\;sums;;arithmetic;"),
            ["calc", "math;sums", "arithmetic"],
            "a list"
        );
        assert_eq!(list(""), Vec::<String>::new(), "an empty list");
    }

    #[test]
    fn locales_are_tried_most_specific_first() {
        assert_eq!(
            locale_keys("sr_YU.UTF-8@Latn"),
            ["sr_YU@Latn", "sr_YU", "sr@Latn", "sr"],
            "the specification's order, encoding dropped"
        );
        assert_eq!(locale_keys("de"), ["de"], "a bare language");
        let file = parse(FILE).unwrap();
        let entry = file.group("Desktop Entry").unwrap();
        assert_eq!(
            localized(entry, "Name", &locale_keys("de_AT.UTF-8")),
            Some("Taschenrechner"),
            "de"
        );
        assert_eq!(localized(entry, "Name", &locale_keys("pt_BR")), Some("Calculadora"), "pt_BR");
        assert_eq!(
            localized(entry, "Name", &locale_keys("fr_FR")),
            Some("Calculator"),
            "the default"
        );
    }

    #[test]
    fn the_messages_locale_follows_the_c_library() {
        assert_eq!(
            messages_locale(None, None, Some("de_DE.UTF-8")).as_deref(),
            Some("de_DE.UTF-8"),
            "LANG"
        );
        assert_eq!(
            messages_locale(Some("fr_FR"), None, Some("de_DE")).as_deref(),
            Some("fr_FR"),
            "LC_ALL wins"
        );
        assert_eq!(
            messages_locale(Some(""), Some("es_ES"), None).as_deref(),
            Some("es_ES"),
            "empty is unset"
        );
        assert_eq!(messages_locale(None, None, Some("C.UTF-8")), None, "C");
        assert_eq!(messages_locale(None, None, None), None, "nothing set");
    }
}

//! The scenario privacy rules and the PII scan (BUILD.md section 4.8 and check 18), shared by
//! `cargo xtask scenarios check` in `tools/repo-tools` and `eileen scenario check` and
//! `share` in `eileen-scenarios`, so a contributor's PC and CI apply one rule.
//!
//! A scenario folder passes when no file is binary (except consented audio), no file is over
//! its size cap, `scenario.toml` never says `origin = "user"`, no text line holds a secret the
//! [`Redactor`] finds or anything the PII scan finds (e-mail addresses, phone numbers but not
//! dates with a time, user folders such as `C:\Users\<name>`, `/home/<name>`, `/media/<name>`
//! and a WSL distro's `\\wsl.localhost\<distro>\home\<name>` but not the path of a web
//! address, machine names and token prefixes), and:
//!
//! - **Audio only with its consent record.** `scenario.toml` parses as a [`Scenario`] with
//!   `origin = "contributed-consented-audio"`, its `intent.audio.file` names the audio file,
//!   and the record at `intent.audio.consent` is in the folder. Any other audio file fails.
//! - **Only placeholders in personal fields.** Every value in a position the contracts mark
//!   `Personal` (the `x-eileen-text` schema extension), in `scenario.toml` and in every line of
//!   the replay inputs (`sim.inputs`, `replay/inputs.jsonl` by default), is built only from
//!   the [`PLACEHOLDERS`]: after removing them, no letter or digit is left. The rule holds for
//!   every origin, as section 4.8 states; a hand-written scenario drives the core through its
//!   steps and utterances, which are not `Personal` positions.
//! - **Readable.** A `scenario.toml` that does not parse as a scenario, or a replay line that
//!   does not parse as a `RecordLine`, fails, because these rules cannot vouch for it. The
//!   problem names the file and line, never the text.

mod personal;

use std::collections::BTreeSet;
use std::fmt;

use eileen_contracts::record::RecordLine;
use eileen_contracts::scenario::{Scenario, ScenarioOrigin};

use crate::redact::{Redactor, SecretKind};

/// What a shared scenario writes in place of personal values.
pub const PLACEHOLDERS: [&str; 5] = ["{HOME}", "{DOCS}", "{WORK}", "{NAME}", "{TITLE}"];

/// The largest text file in a scenario folder, bytes.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// The largest consented audio file, bytes.
pub const MAX_AUDIO_BYTES: usize = 5 * 1024 * 1024;
/// The largest scenario folder, bytes.
pub const MAX_FOLDER_BYTES: usize = 8 * 1024 * 1024;

/// What the PII scan found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PiiKind {
    /// An e-mail address outside the reserved example domains.
    Email,
    /// A phone number.
    Phone,
    /// A user's folder with a real name in it.
    UserPath,
    /// A Windows default machine name or a UNC host.
    MachineName,
    /// A credential's prefix followed by token text.
    TokenPrefix,
    /// A secret the [`Redactor`] found.
    Secret(SecretKind),
}

impl fmt::Display for PiiKind {
    /// Plain words for a contributor: "an e-mail address", "a user folder".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Email => f.write_str("an e-mail address"),
            Self::Phone => f.write_str("a phone number"),
            Self::UserPath => f.write_str("a user folder with a name in it"),
            Self::MachineName => f.write_str("a machine name"),
            Self::TokenPrefix => f.write_str("a credential's prefix with token text"),
            Self::Secret(kind) => write!(f, "a secret ({})", kind.as_str()),
        }
    }
}

/// One finding in a line, as a byte range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiiFinding {
    /// What it is.
    pub kind: PiiKind,
    /// Where it starts.
    pub start: usize,
    /// Where it ends.
    pub end: usize,
}

/// One file of a scenario folder.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioFile<'a> {
    /// Its path inside the folder, `/`-separated (`scenario.toml`, `replay/inputs.jsonl`).
    pub path: &'a str,
    /// Its bytes.
    pub bytes: &'a [u8],
}

/// One broken privacy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// `origin = "user"`: raw captures are never shared.
    OriginUser {
        /// The 1-based line.
        line: usize,
    },
    /// Audio without the consented origin and its consent record.
    AudioWithoutConsent {
        /// The audio file.
        path: String,
    },
    /// A binary file that is not consented audio.
    BinaryFile {
        /// The file.
        path: String,
    },
    /// A file or the folder over its cap.
    TooLarge {
        /// The file, or `.` for the folder.
        path: String,
        /// Its size.
        bytes: usize,
        /// The cap.
        cap: usize,
    },
    /// Personal data or a secret in a text line.
    Pii {
        /// The file.
        path: String,
        /// The 1-based line.
        line: usize,
        /// What was found.
        kind: PiiKind,
    },
    /// A value in a `Personal` position that is not built only from the placeholders.
    NotPlaceholder {
        /// The file.
        path: String,
        /// The 1-based line of a replay line; `None` in `scenario.toml`.
        line: Option<usize>,
        /// Where in the value, as a JSON pointer (`/input/input/user/cli/text`).
        pointer: String,
    },
    /// A file these rules must read does not parse, so they cannot vouch for it.
    Unreadable {
        /// The file.
        path: String,
        /// The 1-based line of a replay line; `None` for `scenario.toml`.
        line: Option<usize>,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OriginUser { line } => {
                write!(f, "scenario.toml:{line}: origin = \"user\" is never shared")
            }
            Self::AudioWithoutConsent { path } => {
                write!(
                    f,
                    "{path}: audio needs origin = \"contributed-consented-audio\" and its consent record"
                )
            }
            Self::BinaryFile { path } => write!(f, "{path}: binary files are not allowed"),
            Self::TooLarge { path, bytes, cap } => {
                write!(f, "{path}: {bytes} bytes, over the {cap}-byte cap")
            }
            Self::Pii { path, line, kind } => {
                write!(f, "{path}:{line}: {kind}; use a placeholder")
            }
            Self::NotPlaceholder { path, line, pointer } => {
                write!(
                    f,
                    "{}: {pointer} is a personal field; write only placeholders there",
                    at(path, *line)
                )
            }
            Self::Unreadable { path, line } => {
                write!(f, "{}: does not parse, so its privacy cannot be checked", at(path, *line))
            }
        }
    }
}

/// `path` or `path:line`.
fn at(path: &str, line: Option<usize>) -> String {
    line.map_or_else(|| path.to_owned(), |line| format!("{path}:{line}"))
}

/// Domains reserved for examples (RFC 2606 and 6761), which scenarios may use.
fn example_domain(domain: &str) -> bool {
    let domain = domain.to_ascii_lowercase();
    ["example.com", "example.org", "example.net"].contains(&domain.as_str())
        || [".example", ".test", ".invalid", ".localhost", ".example.com"]
            .iter()
            .any(|s| domain.ends_with(s))
}

const fn email_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+' | b'%')
}

fn emails(line: &str, out: &mut Vec<PiiFinding>) {
    let bytes = line.as_bytes();
    for (at, _) in line.match_indices('@') {
        let mut start = at;
        while start > 0 && bytes.get(start - 1).copied().is_some_and(email_byte) {
            start -= 1;
        }
        let mut end = at + 1;
        while bytes
            .get(end)
            .copied()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            end += 1;
        }
        let domain = line.get(at + 1..end).unwrap_or_default().trim_end_matches('.');
        if start < at && domain.contains('.') && !example_domain(domain) {
            out.push(PiiFinding { kind: PiiKind::Email, start, end });
        }
    }
}

/// The length of the date and time at the start of `bytes`, if one starts there: ISO 8601's and
/// TOML's `2026-10-21 15:30` or `2026-10-21T15:30`, with any seconds, fraction and offset
/// (`:00.123456-07:00`, `Z`). Its digits are no phone number, though the phone rule reads ten
/// in four groups up to a spaced hour, and ten in the seconds, fraction and offset hour.
fn date_time_len(bytes: &[u8]) -> Option<usize> {
    let digits = |at: usize, n: usize| {
        bytes.get(at..at + n).is_some_and(|run| run.iter().all(u8::is_ascii_digit))
    };
    let byte = |at: usize| bytes.get(at).copied();
    let date = digits(0, 4)
        && byte(4) == Some(b'-')
        && digits(5, 2)
        && byte(7) == Some(b'-')
        && digits(8, 2);
    let time = matches!(byte(10), Some(b' ' | b'T' | b't'))
        && digits(11, 2)
        && byte(13) == Some(b':')
        && digits(14, 2);
    if !(date && time) {
        return None;
    }
    let mut end = 16;
    if byte(end) == Some(b':') && digits(end + 1, 2) {
        end += 3;
        let fraction = bytes.get(end + 1..).unwrap_or_default();
        let fraction = fraction.iter().take_while(|b| b.is_ascii_digit()).count();
        if byte(end) == Some(b'.') && fraction > 0 {
            end += 1 + fraction;
        }
    }
    if matches!(byte(end), Some(b'Z' | b'z')) {
        end += 1;
    } else if matches!(byte(end), Some(b'+' | b'-'))
        && digits(end + 1, 2)
        && byte(end + 3) == Some(b':')
        && digits(end + 4, 2)
    {
        end += 6;
    }
    Some(end)
}

fn phones(line: &str, out: &mut Vec<PiiFinding>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let starts = bytes.get(i).is_some_and(|b| b.is_ascii_digit() || *b == b'+' || *b == b'(')
            && !bytes
                .get(i.wrapping_sub(1))
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'.');
        if !starts {
            i += 1;
            continue;
        }
        if let Some(len) = date_time_len(bytes.get(i..).unwrap_or_default()) {
            i += len;
            continue;
        }
        let plus = bytes.get(i) == Some(&b'+');
        let (mut j, mut digits, mut groups, mut dots, mut separators) =
            (i + usize::from(plus), 0, 1, 0, 0);
        while let Some(&b) = bytes.get(j) {
            if b.is_ascii_digit() {
                digits += 1;
                separators = 0;
            } else if matches!(b, b' ' | b'-' | b'.' | b'(' | b')')
                && separators < 2
                && (digits > 0 || b == b'(')
            {
                if separators == 0 && digits > 0 {
                    groups += 1;
                    dots += usize::from(b == b'.');
                }
                separators += 1;
            } else {
                break;
            }
            j += 1;
        }
        while j > i && !bytes.get(j - 1).is_some_and(u8::is_ascii_digit) {
            j -= 1;
        }
        let ipv4 = dots == 3 && groups == 4;
        if (10..=15).contains(&digits) && (plus || groups >= 2) && !ipv4 {
            out.push(PiiFinding { kind: PiiKind::Phone, start: i, end: j });
        }
        i = j.max(i + 1);
    }
}

/// True when `before`, the text in front of a folder marker, puts the marker in a web address
/// (`http://` or `https://` earlier in the same word): the path of a page, not a folder on a PC.
/// A `file://` address, or `sftp://` and the like, names a folder, so it is still scanned.
fn in_web_address(before: &str) -> bool {
    let word = before
        .rsplit(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>'))
        .next()
        .unwrap_or_default();
    word.contains("http://") || word.contains("https://")
}

/// The folders a user's name follows: Windows' and macOS's `Users` (plain, JSON-escaped and
/// with forward slashes), Linux's `/home/`, and `/media/`, where Ubuntu mounts a user's drives
/// (`/media/<name>/<label>`; Fedora's and Arch's `/run/media/<name>` too).
const USER_FOLDERS: [&str; 6] =
    [":\\users\\", ":\\\\users\\\\", ":/users/", "/home/", "/users/", "/media/"];

/// A Linux home as a Windows app shows a WSL distro's files, `\home\` plain and JSON-escaped:
/// it counts only under the distro's root ([`under_wsl_distro`]), so a Windows folder named
/// Home is not read as one.
const WSL_HOMES: [&str; 2] = ["\\home\\", "\\\\home\\\\"];

/// True when `before`, the text in front of a `\home\` marker, ends with a WSL distro's root,
/// `\\wsl.localhost\<distro>` or `\\wsl$\<distro>`, plain or JSON-escaped.
fn under_wsl_distro(before: &str) -> bool {
    let mut parts = before.rsplit('\\').filter(|part| !part.is_empty());
    parts.next().is_some() && matches!(parts.next(), Some("wsl.localhost" | "wsl$"))
}

fn user_paths(line: &str, out: &mut Vec<PiiFinding>) {
    let lower = line.to_ascii_lowercase();
    let markers =
        USER_FOLDERS.iter().map(|m| (m, false)).chain(WSL_HOMES.iter().map(|m| (m, true)));
    for (marker, wsl) in markers {
        for (at, _) in lower.match_indices(marker) {
            let before = lower.get(..at).unwrap_or_default();
            if (wsl && !under_wsl_distro(before)) || in_web_address(before) {
                continue;
            }
            let name_start = at + marker.len();
            let name: String = line
                .get(name_start..)
                .unwrap_or_default()
                .chars()
                .take_while(|c| !matches!(c, '\\' | '/' | '"' | '\'' | ' ' | '\t'))
                .collect();
            let generic = name.is_empty()
                || PLACEHOLDERS.contains(&name.as_str())
                || ["public", "default", "default user", "all users", "runneradmin", "runner"]
                    .contains(&name.to_ascii_lowercase().as_str());
            if !generic {
                out.push(PiiFinding {
                    kind: PiiKind::UserPath,
                    start: at.saturating_sub(1),
                    end: name_start + name.len(),
                });
            }
        }
    }
}

fn machine_names(line: &str, out: &mut Vec<PiiFinding>) {
    for prefix in ["DESKTOP-", "LAPTOP-"] {
        for (at, _) in line.match_indices(prefix) {
            let rest = line.get(at + prefix.len()..).unwrap_or_default();
            let len =
                rest.bytes().take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit()).count();
            if len == 7 {
                out.push(PiiFinding {
                    kind: PiiKind::MachineName,
                    start: at,
                    end: at + prefix.len() + len,
                });
            }
        }
    }
    // A UNC path at the start of a value, written plainly or JSON-escaped: \\HOST\share.
    for marker in ["\\\\\\\\", "\\\\"] {
        for (at, _) in line.match_indices(marker) {
            let value_start = at == 0
                || line
                    .as_bytes()
                    .get(at - 1)
                    .is_some_and(|b| matches!(b, b' ' | b'"' | b'\'' | b'\t' | b'=' | b'('));
            let rest = line.get(at + marker.len()..).unwrap_or_default();
            let host = rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'-').count();
            let after = rest.as_bytes().get(host).copied();
            if value_start && host >= 2 && after == Some(b'\\') {
                out.push(PiiFinding {
                    kind: PiiKind::MachineName,
                    start: at,
                    end: at + marker.len() + host,
                });
                break;
            }
        }
    }
}

fn token_prefixes(line: &str, out: &mut Vec<PiiFinding>) {
    for prefix in ["sk-", "ghp_", "github_pat_", "xoxb-", "xoxp-", "AKIA", "eyJ"] {
        for (at, _) in line.match_indices(prefix) {
            let word_start =
                at == 0 || !line.as_bytes().get(at - 1).is_some_and(u8::is_ascii_alphanumeric);
            let rest = line.get(at + prefix.len()..).unwrap_or_default();
            let token = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                .count();
            if word_start && token >= 8 {
                out.push(PiiFinding {
                    kind: PiiKind::TokenPrefix,
                    start: at,
                    end: at + prefix.len() + token,
                });
            }
        }
    }
}

/// Everything the PII scan and the redactor find in one line.
#[must_use]
pub fn scan_line(line: &str, redactor: &Redactor) -> Vec<PiiFinding> {
    let mut found = Vec::new();
    emails(line, &mut found);
    phones(line, &mut found);
    user_paths(line, &mut found);
    machine_names(line, &mut found);
    token_prefixes(line, &mut found);
    for f in redactor.find(line) {
        found.push(PiiFinding {
            kind: PiiKind::Secret(f.kind),
            start: f.range.start,
            end: f.range.end,
        });
    }
    found.sort_by_key(|f| (f.start, f.kind));
    found
}

fn is_audio(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav") || ext.eq_ignore_ascii_case("flac"))
}

/// A path inside the folder in one spelling: `/`-separated, without a leading `./`.
fn normalised(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").map_or_else(|| path.clone(), str::to_owned)
}

/// `scenario.toml`, parsed; `None` when it is missing, and a problem as well when it does not
/// parse.
fn parse_scenario(files: &[ScenarioFile<'_>], problems: &mut Vec<Problem>) -> Option<Scenario> {
    let file = files.iter().find(|f| f.path == "scenario.toml")?;
    let parsed = std::str::from_utf8(file.bytes).ok().and_then(|text| toml::from_str(text).ok());
    if parsed.is_none() {
        problems.push(Problem::Unreadable { path: file.path.to_owned(), line: None });
    }
    parsed
}

/// The one audio file the folder may hold: the scenario's consented clip, when its origin is
/// the consented one and its consent record is in the folder.
fn consented_audio(scenario: Option<&Scenario>, files: &[ScenarioFile<'_>]) -> Option<String> {
    let scenario = scenario?;
    let audio = scenario.intent.audio.as_ref()?;
    let consent = normalised(&audio.consent);
    let has_record = files
        .iter()
        .any(|f| normalised(f.path) == consent && !f.bytes.is_empty() && !is_audio(f.path));
    (scenario.origin == ScenarioOrigin::ContributedConsentedAudio && has_record)
        .then(|| normalised(&audio.file))
}

/// The replay inputs file of the folder: `sim.inputs`, else the default path.
fn replay_inputs(scenario: Option<&Scenario>) -> String {
    scenario
        .and_then(|s| s.sim.as_ref())
        .map_or_else(|| "replay/inputs.jsonl".to_owned(), |sim| normalised(&sim.inputs))
}

/// The placeholder rule over one replay line, which must be a `RecordLine`.
fn check_replay_line(path: &str, line_no: usize, line: &str, problems: &mut Vec<Problem>) {
    if line.trim().is_empty() {
        return;
    }
    let value = serde_json::from_str::<serde_json::Value>(line).ok();
    let typed =
        value.as_ref().is_some_and(|v| serde_json::from_value::<RecordLine>(v.clone()).is_ok());
    let Some(value) = value.filter(|_| typed) else {
        problems.push(Problem::Unreadable { path: path.to_owned(), line: Some(line_no) });
        return;
    };
    for pointer in personal::offending(&value, "RecordLine") {
        problems.push(Problem::NotPlaceholder {
            path: path.to_owned(),
            line: Some(line_no),
            pointer,
        });
    }
}

/// Every problem in one scenario folder.
#[must_use]
pub fn check_files(files: &[ScenarioFile<'_>], redactor: &Redactor) -> Vec<Problem> {
    let mut problems = Vec::new();
    let scenario = parse_scenario(files, &mut problems);
    let allowed_audio = consented_audio(scenario.as_ref(), files);
    let inputs = replay_inputs(scenario.as_ref());
    if let Some(value) = scenario.as_ref().and_then(|s| serde_json::to_value(s).ok()) {
        for pointer in personal::offending(&value, "Scenario") {
            problems.push(Problem::NotPlaceholder {
                path: "scenario.toml".to_owned(),
                line: None,
                pointer,
            });
        }
    }
    let total: usize = files.iter().map(|f| f.bytes.len()).sum();
    if total > MAX_FOLDER_BYTES {
        problems.push(Problem::TooLarge {
            path: ".".to_owned(),
            bytes: total,
            cap: MAX_FOLDER_BYTES,
        });
    }
    for file in files {
        if is_audio(file.path) {
            if allowed_audio.as_deref() != Some(normalised(file.path).as_str()) {
                problems.push(Problem::AudioWithoutConsent { path: file.path.to_owned() });
            }
            if file.bytes.len() > MAX_AUDIO_BYTES {
                problems.push(Problem::TooLarge {
                    path: file.path.to_owned(),
                    bytes: file.bytes.len(),
                    cap: MAX_AUDIO_BYTES,
                });
            }
            continue;
        }
        let Some(text) = std::str::from_utf8(file.bytes).ok().filter(|t| !t.contains('\0')) else {
            problems.push(Problem::BinaryFile { path: file.path.to_owned() });
            continue;
        };
        if file.bytes.len() > MAX_TEXT_BYTES {
            problems.push(Problem::TooLarge {
                path: file.path.to_owned(),
                bytes: file.bytes.len(),
                cap: MAX_TEXT_BYTES,
            });
        }
        let is_inputs = normalised(file.path) == inputs;
        for (n, line) in text.lines().enumerate() {
            if file.path == "scenario.toml" && origin_value(line) == Some("user") {
                problems.push(Problem::OriginUser { line: n + 1 });
            }
            if is_inputs {
                check_replay_line(file.path, n + 1, line, &mut problems);
            }
            // One problem per kind on a line, however many times and wherever it appears.
            let kinds: BTreeSet<PiiKind> =
                scan_line(line, redactor).into_iter().map(|f| f.kind).collect();
            for kind in kinds {
                problems.push(Problem::Pii { path: file.path.to_owned(), line: n + 1, kind });
            }
        }
    }
    problems
}

/// The value of an `origin = "..."` line, if the line is one.
fn origin_value(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("origin")?.trim_start().strip_prefix('=')?.trim();
    let rest = rest.strip_prefix('"')?;
    rest.split('"').next()
}

#[cfg(test)]
mod tests {
    use super::{PiiKind, Problem, ScenarioFile, check_files, scan_line};
    use crate::redact::{Redactor, SecretKind};

    fn kinds(line: &str) -> Vec<PiiKind> {
        scan_line(line, &Redactor::new()).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn the_scan_finds_personal_data() {
        assert_eq!(kinds("mail sam.lee@quillpost.net now"), [PiiKind::Email], "e-mail");
        assert_eq!(kinds("call +1 (555) 123-4567"), [PiiKind::Phone], "phone");
        assert_eq!(
            kinds(r"C:\Users\samlee\Documents\cv.docx"),
            [PiiKind::UserPath],
            "Windows user folder"
        );
        assert_eq!(
            kinds(r#"{"path":"C:\\Users\\samlee\\x"}"#),
            [PiiKind::UserPath],
            "JSON-escaped"
        );
        assert_eq!(kinds("/home/samlee/notes"), [PiiKind::UserPath], "Linux home");
        for (line, what) in [
            (r"\\wsl.localhost\Ubuntu\home\samlee\notes.txt", "a WSL home as Windows shows it"),
            (r"\\wsl$\Ubuntu-24.04\home\samlee\notes.txt", "the older wsl$ form"),
            (r#"{"path":"\\\\wsl.localhost\\Ubuntu\\home\\samlee\\notes.txt"}"#, "JSON-escaped"),
            (r#"{"path":"\\\\wsl$\\Ubuntu\\home\\samlee\\notes.txt"}"#, "wsl$ JSON-escaped"),
            ("/media/samlee/USB/cv.pdf", "Ubuntu's drive mounts"),
            ("/run/media/samlee/USB/cv.pdf", "Fedora's and Arch's"),
        ] {
            assert_eq!(kinds(line), [PiiKind::UserPath], "{what}: {line}");
        }
        assert_eq!(
            kinds("file:///home/samlee/notes.txt"),
            [PiiKind::UserPath],
            "a file address names a folder on a PC"
        );
        assert_eq!(
            kinds("see https://site.test/docs or /home/samlee/notes"),
            [PiiKind::UserPath],
            "a web address ends with its word"
        );
        assert_eq!(kinds("host DESKTOP-4F7Q2ZK"), [PiiKind::MachineName], "default machine name");
        assert_eq!(kinds(r"\\NAS01\photos"), [PiiKind::MachineName], "UNC host");
        assert!(
            kinds("key sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv")
                .contains(&PiiKind::Secret(SecretKind::ApiKey)),
            "secret"
        );
    }

    #[test]
    fn a_kind_found_twice_on_a_line_is_one_problem_in_plain_words() {
        let line = b"write to sam.lee@quillpost.net or +1 (555) 123-4567 or jo.vale@quillpost.net";
        let problems = check(&[("notes.txt", line)]);
        let pii = |kind| Problem::Pii { path: "notes.txt".to_owned(), line: 1, kind };
        assert_eq!(
            problems,
            [pii(PiiKind::Email), pii(PiiKind::Phone)],
            "two addresses with a phone number between them are one e-mail problem"
        );
        assert_eq!(
            problems[0].to_string(),
            "notes.txt:1: an e-mail address; use a placeholder",
            "the kind reads as plain words"
        );
        assert_eq!(
            PiiKind::Secret(SecretKind::ApiKey).to_string(),
            "a secret (api_key)",
            "a secret names its kind as the redactor's replacement does"
        );
    }

    #[test]
    fn placeholders_and_ordinary_values_pass() {
        for line in [
            r"C:\Users\{NAME}\Documents\{TITLE}",
            "{HOME}/projects",
            "test@example.com",
            "\"at_us\": 1761134400000000, \"mono\": 12345",
            "go to 15:30 on 2026-10-21",
            "2026-10-21 15:30:00",
            "created = 2026-10-21 15:30:00",
            "at = 1979-05-27T07:32:00.999999-07:00",
            "{\"at\": \"2026-10-21 15:30:00.5Z\"}",
            "https://api.example.com/users/42",
            "https://site.test/home/index.html",
            "[the guide](HTTPS://site.test/users/guide)",
            "server 192.168.100.200",
            r"C:\Users\Public\Desktop",
            r"\\wsl.localhost\Ubuntu\home\{NAME}\notes.txt",
            "/media/{NAME}/USB/{TITLE}",
            r"{DOCS}\Home\Recipes.docx",
            "https://cdn.example.com/static/media/logo.png",
            "version 1.13.8",
        ] {
            assert!(kinds(line).is_empty(), "{line}: {:?}", kinds(line));
        }
    }

    /// A valid `scenario.toml` with this origin, and `extra` appended (an `[intent.audio]`
    /// table or a step).
    fn scenario(origin: &str, extra: &str) -> String {
        format!(
            "schema = 1\nid = \"open-notepad\"\ntitle = \"Open Notepad\"\narea = \"core\"\n\
             origin = \"{origin}\"\noutcome = \"pass\"\n\n[environment]\nlanes = [\"sim\"]\n\n\
             [intent]\nutterances = [\"open notepad\"]\n{extra}"
        )
    }

    const CONSENTED_AUDIO: &str =
        "\n[intent.audio]\nfile = \"audio/take.wav\"\nconsent = \"audio/consent.txt\"\n";

    fn check(files: &[(&str, &[u8])]) -> Vec<Problem> {
        let files: Vec<ScenarioFile<'_>> =
            files.iter().map(|(path, bytes)| ScenarioFile { path, bytes }).collect();
        check_files(&files, &Redactor::new())
    }

    #[test]
    fn folder_rules_hold() {
        let user = b"schema = 1\norigin = \"user\"\n";
        let wav = [0_u8, 1, 2];
        let png = [0x89, 0x50, 0x4e, 0x47, 0];
        let problems =
            check(&[("scenario.toml", user), ("audio/take.wav", &wav), ("shot.png", &png)]);
        assert!(problems.contains(&Problem::OriginUser { line: 2 }), "{problems:?}");
        assert!(
            problems.contains(&Problem::AudioWithoutConsent { path: "audio/take.wav".to_owned() }),
            "{problems:?}"
        );
        assert!(
            problems.contains(&Problem::BinaryFile { path: "shot.png".to_owned() }),
            "{problems:?}"
        );
        assert!(
            problems
                .contains(&Problem::Unreadable { path: "scenario.toml".to_owned(), line: None }),
            "not a scenario: {problems:?}"
        );
        let clean = scenario("synthetic", "");
        assert_eq!(check(&[("scenario.toml", clean.as_bytes())]), [], "a clean folder passes");
    }

    #[test]
    fn audio_needs_its_named_consent_record() {
        let wav = [0_u8, 1, 2];
        let toml = scenario("contributed-consented-audio", CONSENTED_AUDIO);
        // The consented origin and the table, but no consent record in the folder.
        let missing = check(&[("scenario.toml", toml.as_bytes()), ("audio/take.wav", &wav)]);
        assert_eq!(
            missing,
            [Problem::AudioWithoutConsent { path: "audio/take.wav".to_owned() }],
            "no record"
        );
        let record = b"I agree to share this recording under CC-BY-4.0.\n";
        let consented = check(&[
            ("scenario.toml", toml.as_bytes()),
            ("audio/take.wav", &wav),
            ("audio/consent.txt", record),
        ]);
        assert_eq!(consented, [], "the named clip with its record passes");
        let other = check(&[
            ("scenario.toml", toml.as_bytes()),
            ("audio/other.wav", &wav),
            ("audio/consent.txt", record),
        ]);
        assert_eq!(
            other,
            [Problem::AudioWithoutConsent { path: "audio/other.wav".to_owned() }],
            "only the clip the scenario names"
        );
        let synthetic = scenario("synthetic", CONSENTED_AUDIO);
        let unconsented = check(&[
            ("scenario.toml", synthetic.as_bytes()),
            ("audio/take.wav", &wav),
            ("audio/consent.txt", record),
        ]);
        assert_eq!(
            unconsented,
            [Problem::AudioWithoutConsent { path: "audio/take.wav".to_owned() }],
            "a record without the consented origin"
        );
    }

    /// One replay line: the user typed `text` on the command line.
    fn typed_line(text: &str) -> String {
        use eileen_contracts::core_io::{Input, UserInput};
        use eileen_contracts::ids::{MonoTime, Personal, TraceCtx, WallTime};
        use eileen_contracts::record::{InputRecord, RecordLine};

        let line = RecordLine::Input(Box::new(InputRecord {
            seq: 1,
            mono: MonoTime(0),
            at: WallTime(0),
            ctx: TraceCtx::default(),
            input: Input::User(UserInput::Cli { text: Personal::new(text.to_owned()) }),
        }));
        serde_json::to_string(&line).unwrap()
    }

    #[test]
    fn personal_fields_hold_only_placeholders() {
        let toml = scenario(
            "synthetic-from-user",
            "\n[sim]\ninputs = \"replay/inputs.jsonl\"\nexpected = \"replay/expected.trace\"\n",
        );
        let inputs = format!(
            "{}\n{}\n",
            typed_line("{HOME}\\{DOCS}\\{TITLE}"),
            typed_line("open my tax return")
        );
        let problems = check(&[
            ("scenario.toml", toml.as_bytes()),
            ("replay/inputs.jsonl", inputs.as_bytes()),
            ("replay/expected.trace", b"turn.ended\n"),
        ]);
        assert_eq!(
            problems,
            [Problem::NotPlaceholder {
                path: "replay/inputs.jsonl".to_owned(),
                line: Some(2),
                pointer: "/input/input/user/cli/text".to_owned(),
            }],
            "the second line's typed text is real text"
        );
        let broken = check(&[
            ("scenario.toml", toml.as_bytes()),
            ("replay/inputs.jsonl", b"{\"input\": 3}\n"),
        ]);
        assert_eq!(
            broken,
            [Problem::Unreadable { path: "replay/inputs.jsonl".to_owned(), line: Some(1) }],
            "a line that is not a RecordLine"
        );
    }

    #[test]
    fn a_hud_setting_value_in_a_step_is_a_personal_field() {
        // `HudInput::SetSetting`'s value is `Personal`: a step that types a real value fails.
        let step = |value: &str| {
            format!(
                "\n[[steps]]\nhud = {{ input = {{ set_setting = {{ key = \"voice.name\", value = \"{value}\" }} }} }}\n"
            )
        };
        let real = scenario("synthetic-from-user", &step("Jordan Vale"));
        let problems = check(&[("scenario.toml", real.as_bytes())]);
        assert!(
            matches!(problems.as_slice(), [Problem::NotPlaceholder { line: None, .. }]),
            "{problems:?}"
        );
        let placeholder = scenario("synthetic-from-user", &step("{NAME}"));
        assert_eq!(check(&[("scenario.toml", placeholder.as_bytes())]), [], "a placeholder passes");
    }
}

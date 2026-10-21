//! The scenario privacy rules and the PII scan (BUILD.md section 4.8 and check 18), shared by
//! `cargo xtask scenarios check` in `tools/repo-tools` and `eileen scenario check` and
//! `share` in `eileen-scenarios`, so a contributor's PC and CI apply one rule.
//!
//! A scenario folder passes when no file is binary (except consented audio), no file is over
//! its size cap, `scenario.toml` never says `origin = "user"`, audio comes only with the
//! consented origin, and no text line holds a secret the [`Redactor`] finds or anything the
//! PII scan finds: e-mail addresses, phone numbers, user folders (`C:\Users\<name>`,
//! `/home/<name>`), machine names and token prefixes. Personal values are written as the
//! [`PLACEHOLDERS`] instead.

use std::fmt;

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
                write!(f, "{path}:{line}: {kind:?}; use a placeholder")
            }
        }
    }
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

fn user_paths(line: &str, out: &mut Vec<PiiFinding>) {
    let lower = line.to_ascii_lowercase();
    for marker in [":\\users\\", ":\\\\users\\\\", ":/users/", "/home/", "/users/"] {
        for (at, _) in lower.match_indices(marker) {
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

/// Every problem in one scenario folder.
#[must_use]
pub fn check_files(files: &[ScenarioFile<'_>], redactor: &Redactor) -> Vec<Problem> {
    let mut problems = Vec::new();
    let scenario_toml = files
        .iter()
        .find(|f| f.path == "scenario.toml")
        .and_then(|f| std::str::from_utf8(f.bytes).ok());
    let consented = scenario_toml.is_some_and(|text| {
        text.lines().any(|l| origin_value(l) == Some("contributed-consented-audio"))
            && text.contains("consent")
    });
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
            if !consented {
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
        for (n, line) in text.lines().enumerate() {
            if file.path == "scenario.toml" && origin_value(line) == Some("user") {
                problems.push(Problem::OriginUser { line: n + 1 });
            }
            let mut kinds: Vec<PiiKind> =
                scan_line(line, redactor).into_iter().map(|f| f.kind).collect();
            kinds.dedup();
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
        assert_eq!(kinds("mail sam.lee@gmail.com now"), [PiiKind::Email], "e-mail");
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
        assert_eq!(kinds("host DESKTOP-4F7Q2ZK"), [PiiKind::MachineName], "default machine name");
        assert_eq!(kinds(r"\\NAS01\photos"), [PiiKind::MachineName], "UNC host");
        assert!(
            kinds("key sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv")
                .contains(&PiiKind::Secret(SecretKind::ApiKey)),
            "secret"
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
            "server 192.168.100.200",
            r"C:\Users\Public\Desktop",
            "version 1.13.8",
        ] {
            assert!(kinds(line).is_empty(), "{line}: {:?}", kinds(line));
        }
    }

    #[test]
    fn folder_rules_hold() {
        let toml = b"schema = 1\norigin = \"user\"\n";
        let wav = [0_u8, 1, 2];
        let files = [
            ScenarioFile { path: "scenario.toml", bytes: toml },
            ScenarioFile { path: "audio/take.wav", bytes: &wav },
            ScenarioFile { path: "shot.png", bytes: &[0x89, 0x50, 0x4e, 0x47, 0] },
        ];
        let problems = check_files(&files, &Redactor::new());
        assert!(problems.contains(&Problem::OriginUser { line: 2 }), "{problems:?}");
        assert!(
            problems.contains(&Problem::AudioWithoutConsent { path: "audio/take.wav".to_owned() }),
            "{problems:?}"
        );
        assert!(
            problems.contains(&Problem::BinaryFile { path: "shot.png".to_owned() }),
            "{problems:?}"
        );
        let clean = [ScenarioFile {
            path: "scenario.toml",
            bytes: b"schema = 1\norigin = \"synthetic\"\n",
        }];
        assert!(check_files(&clean, &Redactor::new()).is_empty(), "a clean folder passes");
    }
}

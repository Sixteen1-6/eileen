//! The `Redactor`: the one scanner that keeps secrets out of every sink.
//!
//! It finds API keys (`sk-...`), GitHub and Slack tokens, AWS access key ids, JWTs, private
//! key blocks, card numbers that pass the Luhn check, and any literal secret registered at run
//! time (the session's hook token, a password field's value the desktop sidecar saw). A cheap
//! literal prefilter keeps the cost per line low: most lines contain none of the prefixes and
//! no long digit run, and skip the scan. A match becomes `[redacted:<kind>]`; a card number
//! standing alone as a JSON number becomes `0`, so the line stays valid JSON (a run of zeros
//! would not be: JSON forbids leading zeros).
//!
//! A literal is found in the forms a sink writes it: raw, JSON-escaped (a field value inside a
//! log line), `Debug`-escaped (a `?value` field, or the console's strings), and `Debug` then
//! JSON-escaped, so a password holding a quote or a backslash is caught after serialization
//! too. The file layer also redacts each field value before it serializes the line, and the
//! line pass stays as the backstop. A ring dump redacts only the values under each record's
//! `fields`, so the record's own ids and times never meet the card rule.
//!
//! A literal stays registered while its [`LiteralGuard`] lives, so a session's hook token
//! leaves the scan when the session ends, and at most [`MAX_LITERALS`] are held at once: each
//! costs a substring search on every line a sink writes. The `Debug` of a [`Redactor`] shows
//! how many literals it holds, never the literals.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

/// What a match was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecretKind {
    /// `sk-...` keys, the form several model vendors issue.
    ApiKey,
    /// GitHub personal access, OAuth, app and refresh tokens.
    GithubToken,
    /// Slack tokens.
    SlackToken,
    /// AWS access key ids.
    AwsKey,
    /// JSON Web Tokens.
    Jwt,
    /// A PEM private key block.
    PrivateKey,
    /// A card number that passes the Luhn check.
    CardNumber,
    /// A literal secret registered with [`Redactor::add_literal`].
    Literal,
}

impl SecretKind {
    /// The name in the replacement text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::GithubToken => "github_token",
            Self::SlackToken => "slack_token",
            Self::AwsKey => "aws_key",
            Self::Jwt => "jwt",
            Self::PrivateKey => "private_key",
            Self::CardNumber => "card",
            Self::Literal => "secret",
        }
    }
}

/// One match, as a byte range of the scanned text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What it is.
    pub kind: SecretKind,
    /// Where it is.
    pub range: Range<usize>,
}

/// The literal prefixes the prefilter looks for.
const PREFIXES: &[&str] = &[
    "sk-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "xox",
    "AKIA",
    "eyJ",
    "-----BEGIN",
];

/// Literal secrets shorter than this are not registered (they would match ordinary text).
pub const MIN_LITERAL_LEN: usize = 6;
/// The most literal secrets registered at once; a registration past it is refused and
/// counted ([`Redactor::refused_literals`]).
pub const MAX_LITERALS: usize = 64;

/// Why a literal secret was not registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralError {
    /// Shorter than [`MIN_LITERAL_LEN`] bytes, so it would match ordinary text.
    TooShort,
    /// [`MAX_LITERALS`] secrets are registered already.
    Full,
}

impl fmt::Display for LiteralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => {
                write!(f, "a literal secret under {MIN_LITERAL_LEN} bytes is not registered")
            }
            Self::Full => write!(f, "{MAX_LITERALS} literal secrets are registered already"),
        }
    }
}

impl std::error::Error for LiteralError {}

/// The registered literal secrets.
#[derive(Default)]
struct Literals {
    /// Each registration's forms, by its id.
    registered: BTreeMap<u64, Vec<String>>,
    /// Every form, with how many registrations hold it, so dropping one of two registrations of
    /// the same secret keeps it redacted.
    forms: BTreeMap<String, usize>,
    /// The next registration's id.
    next: u64,
}

/// The scanner. Share one per process (it counts its redactions).
#[derive(Default)]
pub struct Redactor {
    literals: RwLock<Literals>,
    redactions: AtomicU64,
    refused: AtomicU64,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field("literals", &self.literal_count())
            .field("redactions", &self.redactions())
            .finish_non_exhaustive()
    }
}

/// A registered literal secret. Dropping it removes the secret from the scan; it holds its
/// redactor weakly, so it never keeps one alive.
#[must_use = "dropping the guard unregisters the secret at once"]
#[derive(Debug)]
pub struct LiteralGuard {
    redactor: Weak<Redactor>,
    id: u64,
}

impl Drop for LiteralGuard {
    fn drop(&mut self) {
        if let Some(redactor) = self.redactor.upgrade() {
            redactor.remove_literal(self.id);
        }
    }
}

const fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The end of a run of bytes from `start` that satisfy `ok`.
fn run_end(bytes: &[u8], start: usize, ok: impl Fn(u8) -> bool) -> usize {
    let mut end = start;
    while bytes.get(end).copied().is_some_and(&ok) {
        end += 1;
    }
    end
}

/// True when the byte before `at` cannot continue a token, so a prefix starts a new word.
fn word_start(bytes: &[u8], at: usize) -> bool {
    at == 0 || !bytes.get(at - 1).copied().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0_u32;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut v = u32::from(d - b'0');
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum.is_multiple_of(10)
}

impl Redactor {
    /// A redactor with no literal secrets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a literal secret with its escaped forms (see the module doc) for as long as
    /// the returned guard lives. Refuses a secret shorter than [`MIN_LITERAL_LEN`] bytes, and
    /// any registration past [`MAX_LITERALS`], which it counts.
    pub fn add_literal(self: &Arc<Self>, secret: &str) -> Result<LiteralGuard, LiteralError> {
        if secret.len() < MIN_LITERAL_LEN {
            return Err(LiteralError::TooShort);
        }
        let forms = literal_forms(secret);
        let mut literals = self.write_literals();
        if literals.registered.len() >= MAX_LITERALS {
            drop(literals);
            self.refused.fetch_add(1, Ordering::Relaxed);
            return Err(LiteralError::Full);
        }
        for form in &forms {
            *literals.forms.entry(form.clone()).or_insert(0) += 1;
        }
        let id = literals.next;
        literals.next += 1;
        literals.registered.insert(id, forms);
        drop(literals);
        Ok(LiteralGuard { redactor: Arc::downgrade(self), id })
    }

    /// Removes one registration, keeping the forms another registration still holds.
    fn remove_literal(&self, id: u64) {
        let mut literals = self.write_literals();
        let Some(forms) = literals.registered.remove(&id) else { return };
        for form in forms {
            let left = literals.forms.get_mut(&form).map(|count| {
                *count = count.saturating_sub(1);
                *count
            });
            if left == Some(0) {
                literals.forms.remove(&form);
            }
        }
    }

    /// How many literal secrets are registered now.
    #[must_use]
    pub fn literal_count(&self) -> usize {
        self.read_literals().registered.len()
    }

    /// How many registrations were refused because [`MAX_LITERALS`] were held.
    #[must_use]
    pub fn refused_literals(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    fn read_literals(&self) -> RwLockReadGuard<'_, Literals> {
        self.literals.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_literals(&self) -> RwLockWriteGuard<'_, Literals> {
        self.literals.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many matches this redactor has replaced (`redactions.total`).
    #[must_use]
    pub fn redactions(&self) -> u64 {
        self.redactions.load(Ordering::Relaxed)
    }

    /// The cheap check: false means the text holds no secret this redactor finds.
    #[must_use]
    pub fn prefilter(&self, text: &str) -> bool {
        PREFIXES.iter().any(|p| text.contains(p))
            || has_long_digit_run(text.as_bytes())
            || self.read_literals().forms.keys().any(|l| text.contains(l.as_str()))
    }

    /// Every match in `text`, in order, without overlaps.
    #[must_use]
    pub fn find(&self, text: &str) -> Vec<Finding> {
        if !self.prefilter(text) {
            return Vec::new();
        }
        let bytes = text.as_bytes();
        let mut found = Vec::new();
        {
            let literals = self.read_literals();
            for literal in literals.forms.keys() {
                let mut from = 0;
                while let Some(at) = text.get(from..).and_then(|rest| rest.find(literal.as_str())) {
                    let start = from + at;
                    found.push(Finding {
                        kind: SecretKind::Literal,
                        range: start..start + literal.len(),
                    });
                    from = start + literal.len();
                }
            }
        }
        let mut i = 0;
        while i < bytes.len() {
            if let Some(finding) = match_at(bytes, i) {
                i = finding.range.end;
                found.push(finding);
            } else {
                i += 1;
            }
        }
        found.sort_by_key(|f| (f.range.start, std::cmp::Reverse(f.range.end)));
        let mut merged: Vec<Finding> = Vec::with_capacity(found.len());
        for f in found {
            match merged.last_mut() {
                Some(last) if f.range.start < last.range.end => {
                    last.range.end = last.range.end.max(f.range.end);
                }
                _ => merged.push(f),
            }
        }
        merged
    }

    /// `text` with every match replaced; borrowed when nothing matched.
    #[must_use]
    pub fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let found = self.find(text);
        if found.is_empty() {
            return Cow::Borrowed(text);
        }
        self.redactions
            .fetch_add(u64::try_from(found.len()).unwrap_or(u64::MAX), Ordering::Relaxed);
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for f in &found {
            out.push_str(text.get(at..f.range.start).unwrap_or_default());
            if f.kind == SecretKind::CardNumber && bare_json_number(bytes, &f.range) {
                out.push('0');
            } else {
                out.push_str("[redacted:");
                out.push_str(f.kind.as_str());
                out.push(']');
            }
            at = f.range.end;
        }
        out.push_str(text.get(at..).unwrap_or_default());
        Cow::Owned(out)
    }
}

/// A secret in each form a sink writes it (see the module doc), each once.
fn literal_forms(secret: &str) -> Vec<String> {
    let debug = format!("{secret:?}");
    let debug = unquote(&debug);
    BTreeSet::from([secret.to_owned(), json_escaped(secret), debug.to_owned(), json_escaped(debug)])
        .into_iter()
        .collect()
}

/// `text` without the double quotes around it, as `Debug` and JSON write a string.
fn unquote(text: &str) -> &str {
    text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text)
}

/// `text` as it stands inside a JSON string.
fn json_escaped(text: &str) -> String {
    let quoted = serde_json::Value::from(text).to_string();
    unquote(&quoted).to_owned()
}

/// A run of at least 15 digits, allowing single spaces or hyphens between them.
fn has_long_digit_run(bytes: &[u8]) -> bool {
    let mut digits = 0;
    let mut previous_separator = false;
    for &b in bytes {
        if b.is_ascii_digit() {
            digits += 1;
            previous_separator = false;
            if digits >= 15 {
                return true;
            }
        } else if (b == b' ' || b == b'-') && digits > 0 && !previous_separator {
            previous_separator = true;
        } else {
            digits = 0;
            previous_separator = false;
        }
    }
    false
}

/// True when the range stands where a JSON number would: after `:`, `,` or `[` and before
/// `,`, `}` or `]`, ignoring spaces.
fn bare_json_number(bytes: &[u8], range: &Range<usize>) -> bool {
    let before =
        bytes.get(..range.start).and_then(|b| b.iter().rev().find(|c| !c.is_ascii_whitespace()));
    let after = bytes.get(range.end..).and_then(|b| b.iter().find(|c| !c.is_ascii_whitespace()));
    matches!(before, Some(b':' | b',' | b'[')) && matches!(after, Some(b',' | b'}' | b']'))
}

/// A match starting exactly at `i`, if any.
fn match_at(bytes: &[u8], i: usize) -> Option<Finding> {
    let rest = bytes.get(i..)?;
    let token = |prefix_len: usize, min: usize, kind: SecretKind| {
        let end = run_end(bytes, i + prefix_len, is_token_byte);
        (end - (i + prefix_len) >= min && word_start(bytes, i))
            .then_some(Finding { kind, range: i..end })
    };
    if rest.starts_with(b"sk-") {
        return token(3, 20, SecretKind::ApiKey);
    }
    for prefix in [&b"ghp_"[..], b"gho_", b"ghu_", b"ghs_", b"ghr_", b"github_pat_"] {
        if rest.starts_with(prefix) {
            return token(prefix.len(), 20, SecretKind::GithubToken);
        }
    }
    if rest.starts_with(b"xox")
        && rest.get(3).is_some_and(|b| b"abprs".contains(b))
        && rest.get(4) == Some(&b'-')
    {
        return token(5, 10, SecretKind::SlackToken);
    }
    if rest.starts_with(b"AKIA") && word_start(bytes, i) {
        let end = run_end(bytes, i + 4, |b| b.is_ascii_uppercase() || b.is_ascii_digit());
        return (end - (i + 4) == 16)
            .then_some(Finding { kind: SecretKind::AwsKey, range: i..end });
    }
    if rest.starts_with(b"eyJ") && word_start(bytes, i) {
        return jwt(bytes, i);
    }
    if rest.starts_with(b"-----BEGIN") {
        return private_key(bytes, i);
    }
    if rest.first().is_some_and(|b| (b'2'..=b'6').contains(b))
        && !bytes.get(i.wrapping_sub(1)).is_some_and(u8::is_ascii_digit)
    {
        return card(bytes, i);
    }
    None
}

fn jwt(bytes: &[u8], i: usize) -> Option<Finding> {
    let b64 = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    let first = run_end(bytes, i, b64);
    if first - i < 8 || bytes.get(first) != Some(&b'.') {
        return None;
    }
    let second = run_end(bytes, first + 1, b64);
    if second - (first + 1) < 4 || bytes.get(second) != Some(&b'.') {
        return None;
    }
    let third = run_end(bytes, second + 1, b64);
    Some(Finding { kind: SecretKind::Jwt, range: i..third })
}

fn private_key(bytes: &[u8], i: usize) -> Option<Finding> {
    let rest = bytes.get(i..)?;
    let header_end = rest.get(10..)?.windows(5).position(|w| w == b"-----")? + 10;
    if !rest.get(..header_end)?.windows(11).any(|w| w == b"PRIVATE KEY") {
        return None;
    }
    // To the matching END line, or else to the end of the JSON string or the text: before the
    // first quote and the backslashes in front of it, so a quote escaped once (`\"`) or twice
    // (`\\\"`) keeps its escape and a JSON line stays JSON.
    let end = rest
        .windows(8)
        .position(|w| w == b"-----END")
        .and_then(|at| {
            let after = at + 8;
            rest.get(after..)?.windows(5).position(|w| w == b"-----").map(|p| after + p + 5)
        })
        .or_else(|| {
            let quote = rest.iter().position(|b| *b == b'"')?;
            let escapes = rest.get(..quote)?.iter().rev().take_while(|b| **b == b'\\').count();
            Some(quote - escapes)
        })
        .unwrap_or(rest.len());
    Some(Finding { kind: SecretKind::PrivateKey, range: i..i + end })
}

fn card(bytes: &[u8], i: usize) -> Option<Finding> {
    let mut digits = Vec::with_capacity(19);
    let mut end = i;
    let mut j = i;
    while let Some(&b) = bytes.get(j) {
        if b.is_ascii_digit() {
            digits.push(b);
            j += 1;
            end = j;
            if digits.len() > 19 {
                return None;
            }
        } else if (b == b' ' || b == b'-')
            && bytes.get(j + 1).is_some_and(u8::is_ascii_digit)
            && !digits.is_empty()
        {
            j += 1;
        } else {
            break;
        }
    }
    let followed_by_digit = bytes.get(end).is_some_and(u8::is_ascii_digit);
    let card_length = matches!(digits.len(), 15 | 16 | 19);
    (card_length && !followed_by_digit && luhn(&digits))
        .then_some(Finding { kind: SecretKind::CardNumber, range: i..end })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{LiteralError, MAX_LITERALS, Redactor, SecretKind};

    #[test]
    fn finds_each_kind() {
        let r = Redactor::new();
        let cases = [
            ("key sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv rest", SecretKind::ApiKey),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", SecretKind::GithubToken),
            ("token xoxb-1234567890-abcdef", SecretKind::SlackToken),
            ("AKIAABCDEFGHIJKLMNOP", SecretKind::AwsKey),
            ("auth eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl ok", SecretKind::Jwt),
            (
                "-----BEGIN RSA PRIVATE KEY-----\\nMIIEow\\n-----END RSA PRIVATE KEY-----",
                SecretKind::PrivateKey,
            ),
            ("card 4111 1111 1111 1111 thanks", SecretKind::CardNumber),
        ];
        for (text, kind) in cases {
            let found = r.find(text);
            assert_eq!(found.len(), 1, "{text}");
            assert_eq!(found[0].kind, kind, "{text}");
        }
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let r = Redactor::new();
        for text in [
            "risk-free sk-short",
            "the time is 15:30, at_us 1761134400000000",
            "{\"mono\":1234567890123456}",
            "4111 1111 1111 1112 fails Luhn",
            "-----BEGIN CERTIFICATE-----",
            "version 2026-10-21",
        ] {
            assert!(r.find(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn redacts_and_counts_and_keeps_json_numbers_numbers() {
        let r = Arc::new(Redactor::new());
        let _password = r.add_literal("hunter2-password").unwrap();
        assert_eq!(r.add_literal("short").unwrap_err(), LiteralError::TooShort, "too short");
        let line = r#"{"pin":4111111111111111,"pw":"hunter2-password","short":"short"}"#;
        let out = r.redact(line);
        assert_eq!(out, r#"{"pin":0,"pw":"[redacted:secret]","short":"short"}"#, "redacted");
        assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok(), "still JSON: {out}");
        assert_eq!(r.redactions(), 2, "counted");
        assert!(
            matches!(r.redact("nothing here"), std::borrow::Cow::Borrowed(_)),
            "borrowed when clean"
        );
    }

    #[test]
    fn a_key_header_without_its_end_line_stops_before_an_escaped_quote() {
        let r = Redactor::new();
        let quoted = "could not parse \"-----BEGIN RSA PRIVATE KEY-----\"";
        // Serialized once (a field in a log line), and `Debug` then serialized (a `?value`).
        let once = serde_json::json!({ "line": quoted }).to_string();
        let twice = serde_json::json!({ "line": format!("{quoted:?}") }).to_string();
        for line in [once, twice] {
            let out = r.redact(&line);
            assert!(!out.contains("-----BEGIN"), "{line} came out as {out}");
            assert!(
                serde_json::from_str::<serde_json::Value>(&out).is_ok(),
                "{line} came out as {out}, which is not JSON"
            );
        }
        assert_eq!(
            r.redact(&format!("line={quoted:?}")),
            r#"line="could not parse \"[redacted:private_key]\"""#,
            "the console's form keeps its escaped quote"
        );
    }

    #[test]
    fn a_literal_with_quotes_and_backslashes_is_found_escaped() {
        let r = Arc::new(Redactor::new());
        let secret = "pa\"ss\\word\tcanary";
        let _secret = r.add_literal(secret).unwrap();
        let json = serde_json::json!({ "pw": secret, "dbg": format!("{secret:?}") }).to_string();
        for text in [secret.to_owned(), json, format!("pw={secret:?}")] {
            let out = r.redact(&text);
            assert!(!out.contains("canary"), "{text} came out as {out}");
        }
    }

    #[test]
    fn a_literal_leaves_the_scan_once_its_guards_drop() {
        let r = Arc::new(Redactor::new());
        let first = r.add_literal("hook-token-canary").unwrap();
        let second = r.add_literal("hook-token-canary").unwrap();
        assert_eq!(r.literal_count(), 2, "two registrations of one secret");
        drop(first);
        assert_eq!(
            r.redact("token hook-token-canary"),
            "token [redacted:secret]",
            "the other registration still holds it"
        );
        drop(second);
        assert_eq!(r.literal_count(), 0, "none left");
        assert_eq!(
            r.redact("token hook-token-canary"),
            "token hook-token-canary",
            "unregistered once both guards are gone"
        );
    }

    #[test]
    fn the_literals_have_a_maximum_and_refusals_are_counted() {
        let r = Arc::new(Redactor::new());
        let held: Vec<_> = (0..MAX_LITERALS)
            .map(|i| r.add_literal(&format!("session-token-{i:04}")).unwrap())
            .collect();
        assert_eq!(r.add_literal("one-too-many").unwrap_err(), LiteralError::Full, "refused");
        assert_eq!(r.refused_literals(), 1, "and counted");
        drop(held);
        assert!(r.add_literal("room-again-now").is_ok(), "room again once the guards drop");
    }

    #[test]
    fn debug_shows_how_many_literals_never_which() {
        let r = Arc::new(Redactor::new());
        let _secret = r.add_literal("correct-horse-canary").unwrap();
        let shown = format!("{r:?}");
        assert!(!shown.contains("canary"), "{shown}");
        assert!(shown.contains("literals: 1"), "{shown}");
    }
}

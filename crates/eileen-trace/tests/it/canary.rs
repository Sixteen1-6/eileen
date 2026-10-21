//! The redaction canary: fake secrets written through the file and ring layers never appear
//! raw on disk or in a ring dump (acceptance 6), and the console layer writes none of them
//! either (BUILD.md section 2.13: no sink writes them where policy forbids).

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use eileen_trace::{Console, ProcessKind, Redactor, TraceConfig, names};
use serde_json::Value;
use tracing_subscriber::Registry;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;

/// A fake key shaped like a real one.
const API_KEY: &str = "sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKEFAKE-canary";
/// A fake JWT.
const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYW5hcnkifQ.ZmFrZXNpZ25hdHVyZQ";
/// A Luhn-valid test card, spoken with spaces.
const CARD: &str = "4111 1111 1111 1111";
/// The same card standing as a bare JSON number.
const CARD_NUMBER: u64 = 4_111_111_111_111_111;
/// What a password manager put on the clipboard (a UIA password field's value).
const PASSWORD: &str = "correct-horse-battery-canary";
/// A password with a quote, a backslash and a tab, which serialization escapes.
const QUOTED_PASSWORD: &str = "pa\"ss\\word\tcanary";
/// A private key block's body.
const PEM_BODY: &str = "MIIBVQIBADANBgkqhkiG9w0BAQEFAASCAT8wggE7AgEAAkEAcanary";
/// A marked transcript.
const TRANSCRIPT: &str = "my bank pin is in the blue notebook";

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn read_all(dir: &PathBuf) -> String {
    let mut text = String::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        text.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
    }
    text
}

/// Every form a secret takes in a sink: raw, JSON-escaped, `Debug`-escaped, and `Debug` then
/// JSON-escaped.
fn forms(secret: &str) -> Vec<String> {
    let unquote =
        |text: String| text.strip_prefix('"').unwrap().strip_suffix('"').unwrap().to_owned();
    let json = |text: &str| unquote(Value::from(text).to_string());
    let debug = unquote(format!("{secret:?}"));
    vec![secret.to_owned(), json(secret), json(&debug), debug]
}

/// Asserts that `text` holds no form of any secret and that every line is valid JSON.
fn assert_clean_json_lines(what: &str, text: &str) {
    for secret in [API_KEY, JWT, CARD, PASSWORD, QUOTED_PASSWORD, PEM_BODY] {
        for form in forms(secret) {
            assert!(!text.contains(&form), "{form} reached {what}:\n{text}");
        }
    }
    assert!(!text.contains(&CARD_NUMBER.to_string()), "the bare card reached {what}:\n{text}");
    for line in text.lines() {
        assert!(serde_json::from_str::<Value>(line).is_ok(), "{what} holds a line that is not JSON: {line}");
    }
}

/// Emits every canary secret in every way a field or message can carry it.
fn emit_secrets(pem: &str) {
    let turn = tracing::info_span!(target: "eileen_core", names::TURN, turn_id = "turn_canary", "pii.transcript" = TRANSCRIPT);
    let _entered = turn.enter();
    tracing::info!(target: "eileen_core", key = API_KEY, "the worker printed {JWT}");
    tracing::info!(target: "eileen_core", card = CARD, amount = CARD_NUMBER, "pii.title" = "Bank statement", "spoken digits");
    tracing::warn!(target: "eileen_desktop", password = PASSWORD, pem = %pem, "clipboard");
    tracing::warn!(target: "eileen_desktop", plain = QUOTED_PASSWORD, debug = ?QUOTED_PASSWORD, shown = %QUOTED_PASSWORD, "password field");
    tracing::warn!(target: "eileen_desktop", "typed {QUOTED_PASSWORD:?} into the field");
    tracing::debug!(target: "eileen_core", key = API_KEY, "ring only");
}

#[test]
fn fake_secrets_never_reach_disk_or_a_dump() {
    let dir =
        TempDir(std::env::temp_dir().join(format!("eileen-trace-canary-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    guard.redactor().add_literal(PASSWORD);
    guard.redactor().add_literal(QUOTED_PASSWORD);
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{PEM_BODY}\n-----END PRIVATE KEY-----");
    tracing::subscriber::with_default(subscriber, || emit_secrets(&pem));
    let dump = String::from_utf8(guard.ring().dump(guard.redactor())).unwrap();
    let parsed = guard.ring().records(guard.redactor());
    drop(guard);
    let files = read_all(&dir.0);
    assert!(files.lines().count() >= 5, "the events reached the file:\n{files}");
    assert_clean_json_lines("a log file", &files);
    assert_clean_json_lines("a ring dump", &dump);
    assert_eq!(parsed.skipped, 0, "every dump line read back as a record");
    assert!(
        files.contains("[redacted:api_key]") && files.contains("[redacted:card]"),
        "redacted in place"
    );
    assert!(
        !files.contains(TRANSCRIPT) && !files.contains("Bank statement"),
        "pii fields are reduced in files"
    );
    assert!(dump.contains(TRANSCRIPT), "the ring keeps pii fields in memory, as the policy says");
    assert!(dump.contains("ring only"), "DEBUG for Eileen's crates reaches the ring");
}

/// An in-memory console for the test.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()).unwrap()
    }
}

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn fake_secrets_never_reach_the_console() {
    // The console layer `build` installs with `Console::Always`, over a captured writer.
    let redactor = Arc::new(Redactor::new());
    redactor.add_literal(PASSWORD);
    redactor.add_literal(QUOTED_PASSWORD);
    let console = Captured::default();
    let layer = eileen_trace::init::console_layer::<Registry, _>(
        console.clone(),
        Arc::clone(&redactor),
        "trace",
    )
    .unwrap();
    let subscriber = tracing_subscriber::registry().with(layer);
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{PEM_BODY}\n-----END PRIVATE KEY-----");
    tracing::subscriber::with_default(subscriber, || emit_secrets(&pem));
    let text = console.text();
    assert!(text.lines().count() >= 6, "the events reached the console:\n{text}");
    for secret in [API_KEY, JWT, CARD, PASSWORD, QUOTED_PASSWORD, PEM_BODY] {
        for form in forms(secret) {
            assert!(!text.contains(&form), "{form} reached the console:\n{text}");
        }
    }
    assert!(text.contains("[redacted:"), "redacted in place:\n{text}");
    assert!(text.contains(TRANSCRIPT), "the console keeps pii fields, as the policy says");
}

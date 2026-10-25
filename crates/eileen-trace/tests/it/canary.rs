//! The redaction canary: fake secrets written through the file and ring layers never appear
//! raw on disk or in a ring dump (acceptance 6), and the console layer writes none of them
//! either (BUILD.md section 2.13: no sink writes them where policy forbids). Nor does the home
//! folder of the PC that built a dependency, which its events carry in their source path and
//! its `log` records in a field. And values that only look like a card number reach every
//! sink whole.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use eileen_trace::{Console, ProcessKind, Redactor, TraceConfig, names, test_support};
use serde_json::{Value, json};
use tracing::callsite::{DefaultCallsite, Identifier};
use tracing::field::{FieldSet, Value as FieldValue};
use tracing::metadata::Kind;
use tracing::{Level, Metadata};
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
/// A message quoting a private key header with no END line after it.
const KEY_HEADER_QUOTED: &str = "could not parse \"-----BEGIN RSA PRIVATE KEY-----\"";
/// The card as a negative JSON number.
const NEGATIVE_CARD: i64 = -4_111_111_111_111_111;
/// The card on a line of its own, which `Debug` writes after an escaped newline (`\n`).
const CARD_ON_ITS_OWN_LINE: &str = "Card number:\n4111 1111 1111 1111";
/// 6/11, whose 16 digits after the point pass the card rule's Luhn check.
const RATIO: f64 = 6.0 / 11.0;
/// A `traceparent` whose trace id's last digit, parent id and flags make 19 digits that do too.
const TRACEPARENT: &str = "00-0123456789abcdef0123456789abcde2-0000000000000007-01";

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

/// What a home folder looks like in a line: a Windows user folder, raw or JSON-escaped, and a
/// Linux one.
const HOME_FOLDERS: [&str; 3] = [":\\Users\\", ":\\\\Users\\\\", "/home/"];

/// Asserts that `text` holds no form of any secret and no home folder, and that every line is
/// valid JSON.
fn assert_clean_json_lines(what: &str, text: &str) {
    for secret in [API_KEY, JWT, CARD, PASSWORD, QUOTED_PASSWORD, PEM_BODY] {
        for form in forms(secret) {
            assert!(!text.contains(&form), "{form} reached {what}:\n{text}");
        }
    }
    for home in HOME_FOLDERS {
        assert!(!text.contains(home), "a home folder ({home}) reached {what}:\n{text}");
    }
    assert!(!text.contains(&CARD_NUMBER.to_string()), "the bare card reached {what}:\n{text}");
    for line in text.lines() {
        assert!(
            serde_json::from_str::<Value>(line).is_ok(),
            "{what} holds a line that is not JSON: {line}"
        );
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
    tracing::warn!(target: "eileen_desktop", line = KEY_HEADER_QUOTED, "a key header alone");
    tracing::info!(target: "eileen_core", balance = NEGATIVE_CARD, "a negative card number");
    tracing::info!(target: "eileen_core", note = CARD_ON_ITS_OWN_LINE, shown = ?CARD_ON_ITS_OWN_LINE, "a card on its own line");
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
    let _password = guard.redactor().add_literal(PASSWORD).unwrap();
    let _quoted = guard.redactor().add_literal(QUOTED_PASSWORD).unwrap();
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

/// A registry crate's event on a Windows build PC: its file, and its default name, hold the
/// absolute path under the user's folder.
static WINDOWS_DEPENDENCY: Metadata<'static> = Metadata::new(
    "event C:\\Users\\someone\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\somecrate-1.2.3\\src\\client.rs:7",
    "somecrate::client",
    Level::WARN,
    Some(
        "C:\\Users\\someone\\.cargo\\registry\\src\\index.crates.io-1949cf8c6b5b557f\\somecrate-1.2.3\\src\\client.rs",
    ),
    Some(7),
    Some("somecrate::client"),
    FieldSet::new(&["message"], Identifier(&WINDOWS_CALLSITE)),
    Kind::EVENT,
);
static WINDOWS_CALLSITE: DefaultCallsite = DefaultCallsite::new(&WINDOWS_DEPENDENCY);

/// The same on a Linux build PC.
static LINUX_DEPENDENCY: Metadata<'static> = Metadata::new(
    "event /home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/somecrate-1.2.3/src/lib.rs:9",
    "somecrate",
    Level::WARN,
    Some(
        "/home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/somecrate-1.2.3/src/lib.rs",
    ),
    Some(9),
    Some("somecrate"),
    FieldSet::new(&["message"], Identifier(&LINUX_CALLSITE)),
    Kind::EVENT,
);
static LINUX_CALLSITE: DefaultCallsite = DefaultCallsite::new(&LINUX_DEPENDENCY);

/// Emits one event through `meta`'s callsite, as `tracing`'s macros do in a dependency.
fn emit_from(meta: &'static Metadata<'static>) {
    let fields = meta.fields();
    let message = fields.field("message").unwrap();
    let text: &dyn FieldValue = &"a dependency logged this";
    let values = [(&message, Some(text))];
    let value_set = fields.value_set(&values);
    tracing::dispatcher::get_default(|dispatch| {
        if dispatch.enabled(meta) {
            dispatch.event(&tracing::Event::new(meta, &value_set));
        }
    });
}

#[test]
fn no_home_folder_reaches_disk_or_a_dump() {
    let dir =
        TempDir(std::env::temp_dir().join(format!("eileen-trace-home-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        emit_from(&WINDOWS_DEPENDENCY);
        emit_from(&LINUX_DEPENDENCY);
    });
    let dump = String::from_utf8(guard.ring().dump(guard.redactor())).unwrap();
    drop(guard);
    let files = read_all(&dir.0);
    assert!(
        files.contains("somecrate-1.2.3/src/client.rs")
            && files.contains("somecrate-1.2.3/src/lib.rs"),
        "the files keep each event's path from its crate's folder:\n{files}"
    );
    assert!(
        dump.contains("event somecrate-1.2.3/src/client.rs:7")
            && dump.contains("event somecrate-1.2.3/src/lib.rs:9"),
        "the dump keeps each event's location:\n{dump}"
    );
    assert_clean_json_lines("a log file", &files);
    assert_clean_json_lines("a ring dump", &dump);
    assert!(!files.contains("someone") && !dump.contains("someone"), "no user name anywhere");
}

/// The fields of tracing-log 0.2.0's callsites, which carry a bridged record's own target,
/// module, file and line.
const LOG_FIELDS: &[&str] = &["message", "log.target", "log.module_path", "log.file", "log.line"];

/// tracing-log 0.2.0's callsite for `log` records at WARN, which `init` installs: one per
/// level, shared by every record, named `log event` with target `log` and no location.
static LOG_WARN: Metadata<'static> = Metadata::new(
    "log event",
    "log",
    Level::WARN,
    None,
    None,
    None,
    FieldSet::new(LOG_FIELDS, Identifier(&LOG_WARN_CALLSITE)),
    Kind::EVENT,
);
static LOG_WARN_CALLSITE: DefaultCallsite = DefaultCallsite::new(&LOG_WARN);

/// Where a dependency's `log::warn!` was, on a Linux build PC.
const LOG_FILE: &str = "/home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/somecrate-1.2.3/src/conn.rs";

/// `value` as a field value.
fn as_value<T: FieldValue>(value: &T) -> &dyn FieldValue {
    value
}

/// A dependency's `log::warn!(target: target, "{message}")` at `location`, bridged as
/// tracing-log's `dispatch_record` does it: the dispatcher is asked with the record's own target
/// and location, and the event goes through the shared callsite, with them in its fields.
fn log_warn(target: &str, location: Option<(&str, u32)>, message: &str) {
    let (file, line) = (location.map(|(file, _)| file), location.map(|(_, line)| line));
    let fieldset = FieldSet::new(LOG_FIELDS, Identifier(&LOG_WARN_CALLSITE));
    let asked = Metadata::new(
        "log record",
        target,
        Level::WARN,
        file,
        line,
        Some(target),
        fieldset,
        Kind::EVENT,
    );
    let fields = LOG_WARN.fields();
    let field = |name: &str| fields.field(name).unwrap();
    let shown = tracing::field::display(message);
    tracing::dispatcher::get_default(|dispatch| {
        if dispatch.enabled(&asked) {
            dispatch.event(&tracing::Event::new(
                &LOG_WARN,
                &fields.value_set(&[
                    (&field("message"), Some(as_value(&shown))),
                    (&field("log.target"), Some(as_value(&target))),
                    (&field("log.module_path"), Some(as_value(&target))),
                    (&field("log.file"), file.as_ref().map(as_value)),
                    (&field("log.line"), line.as_ref().map(as_value)),
                ]),
            ));
        }
    });
}

/// Every key of a line's or a record's fields.
fn keys<'a>(fields: impl IntoIterator<Item = (&'a String, &'a Value)>) -> Vec<&'a str> {
    fields.into_iter().map(|(key, _)| key.as_str()).collect()
}

#[test]
fn a_log_crate_record_keeps_its_own_target_and_location() {
    let dir =
        TempDir(std::env::temp_dir().join(format!("eileen-trace-log-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        log_warn("somecrate::conn", Some((LOG_FILE, 42)), "a log record");
        log_warn("somecrate::bare", None, "a log record without a location");
        tracing::warn!(target: "eileen_core", "a tracing event");
    });
    let parsed = guard.ring().records(guard.redactor());
    let dump = String::from_utf8(guard.ring().dump(guard.redactor())).unwrap();
    drop(guard);
    let files = read_all(&dir.0);
    let lines: Vec<Value> = files.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let line = |message: &str| lines.iter().find(|l| l["fields"]["message"] == message).unwrap();
    let origin = |line: &Value| {
        (line["target"].clone(), line["filename"].clone(), line["line_number"].clone())
    };
    let record = line("a log record");
    assert_eq!(
        origin(record),
        (json!("somecrate::conn"), json!("somecrate-1.2.3/src/conn.rs"), json!(42)),
        "the file line has the record's own target and location: {record}"
    );
    assert_eq!(
        keys(record["fields"].as_object().unwrap()),
        ["message"],
        "no log.* field: {record}"
    );
    let bare = line("a log record without a location");
    assert_eq!(
        origin(bare),
        (json!("somecrate::bare"), Value::Null, Value::Null),
        "a record without a location keeps its own target: {bare}"
    );
    let event = line("a tracing event");
    assert_eq!(event["target"], "eileen_core", "a tracing event keeps its own: {event}");
    assert!(event["filename"].as_str().unwrap().ends_with("canary.rs"), "its file: {event}");
    let ring = |message: &str| {
        let message = Value::from(message);
        parsed.events.iter().find(|e| e.fields.get("message") == Some(&message)).unwrap()
    };
    let (record, bare) = (ring("a log record"), ring("a log record without a location"));
    assert_eq!(
        (record.target.as_str(), record.name.as_str(), keys(&record.fields)),
        ("somecrate::conn", "event somecrate-1.2.3/src/conn.rs:42", vec!["message"]),
        "the ring names the record from its own location"
    );
    assert_eq!(
        (bare.target.as_str(), bare.name.as_str()),
        ("somecrate::bare", "log event"),
        "a record without a location keeps the callsite's name"
    );
    let ((), captured) =
        test_support::capture(|| log_warn("somecrate::conn", Some((LOG_FILE, 42)), "captured"));
    let captured = &captured.events[0];
    assert_eq!(
        (captured.target.as_str(), captured.name.as_str(), keys(&captured.fields)),
        ("somecrate::conn", "event somecrate-1.2.3/src/conn.rs:42", vec!["message"]),
        "the test capture keeps the record as the ring does"
    );
    assert_clean_json_lines("a log file", &files);
    assert_clean_json_lines("a ring dump", &dump);
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
    let _password = redactor.add_literal(PASSWORD).unwrap();
    let _quoted = redactor.add_literal(QUOTED_PASSWORD).unwrap();
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
    // The card as a number too (`amount`, `balance`). The console redacts the text `fmt` wrote,
    // so the card rule finds a value's first digit only while no colour code stands before it.
    assert!(
        !text.contains(&CARD_NUMBER.to_string()),
        "the bare card reached the console:\n{text:?}"
    );
    assert!(!text.contains('\u{1b}'), "the console wrote a colour code:\n{text:?}");
    assert!(text.contains("[redacted:"), "redacted in place:\n{text}");
    assert!(text.contains(TRANSCRIPT), "the console keeps pii fields, as the policy says");
}

/// Emits values that only look like card numbers: a fraction and a `traceparent`.
fn emit_lookalikes() {
    let action = tracing::info_span!(target: "eileen_desktop", names::DESKTOP_ACTION, traceparent = TRACEPARENT);
    let _entered = action.enter();
    tracing::warn!(target: "eileen_desktop", ratio = RATIO, "scored");
}

#[test]
fn values_that_only_look_like_cards_reach_every_sink() {
    let dir = TempDir(
        std::env::temp_dir().join(format!("eileen-trace-lookalike-{}", std::process::id())),
    );
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Desktop, &config).unwrap();
    tracing::subscriber::with_default(subscriber, emit_lookalikes);
    let dump = String::from_utf8(guard.ring().dump(guard.redactor())).unwrap();
    let parsed = guard.ring().records(guard.redactor());
    drop(guard);
    let console = Captured::default();
    let layer = eileen_trace::init::console_layer::<Registry, _>(
        console.clone(),
        Arc::new(Redactor::new()),
        "trace",
    )
    .unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), emit_lookalikes);
    for (what, text) in
        [("a log file", read_all(&dir.0)), ("a ring dump", dump), ("the console", console.text())]
    {
        assert!(text.contains("0.5454545454545454"), "the ratio reached {what} whole:\n{text}");
        assert!(text.contains(TRACEPARENT), "the traceparent reached {what} whole:\n{text}");
        assert!(!text.contains("[redacted:"), "{what} redacted neither:\n{text}");
    }
    assert!(
        parsed.spans.iter().any(|span| span.remote_parent.is_some()),
        "the dumped span keeps its parent in the core"
    );
}

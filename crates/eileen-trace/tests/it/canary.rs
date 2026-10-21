//! The redaction canary: fake secrets written through the file and ring layers never appear
//! raw on disk or in a ring dump (acceptance 6).

use std::path::PathBuf;

use eileen_trace::{Console, ProcessKind, TraceConfig, names};

/// A fake key shaped like a real one.
const API_KEY: &str = "sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKEFAKE-canary";
/// A fake JWT.
const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYW5hcnkifQ.ZmFrZXNpZ25hdHVyZQ";
/// A Luhn-valid test card, spoken with spaces.
const CARD: &str = "4111 1111 1111 1111";
/// What a password manager put on the clipboard (a UIA password field's value).
const PASSWORD: &str = "correct-horse-battery-canary";
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

#[test]
fn fake_secrets_never_reach_disk_or_a_dump() {
    let dir =
        TempDir(std::env::temp_dir().join(format!("eileen-trace-canary-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    guard.redactor().add_literal(PASSWORD);
    let pem = format!("-----BEGIN PRIVATE KEY-----\n{PEM_BODY}\n-----END PRIVATE KEY-----");
    tracing::subscriber::with_default(subscriber, || {
        let turn = tracing::info_span!(target: "eileen_core", names::TURN, turn_id = "turn_canary", "pii.transcript" = TRANSCRIPT);
        let _entered = turn.enter();
        tracing::info!(target: "eileen_core", key = API_KEY, "the worker printed {JWT}");
        tracing::info!(target: "eileen_core", card = CARD, "pii.title" = "Bank statement", "spoken digits");
        tracing::warn!(target: "eileen_desktop", password = PASSWORD, pem = %pem, "clipboard");
        tracing::debug!(target: "eileen_core", key = API_KEY, "ring only");
    });
    let dump = String::from_utf8(guard.ring().dump(guard.redactor())).unwrap();
    drop(guard);
    let files = read_all(&dir.0);
    assert!(files.lines().count() >= 3, "the events reached the file:\n{files}");
    for secret in [API_KEY, JWT, CARD, PASSWORD, PEM_BODY] {
        assert!(!files.contains(secret), "{secret} reached a log file:\n{files}");
        assert!(!dump.contains(secret), "{secret} reached a ring dump:\n{dump}");
    }
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

//! Span and event records: what the flight recorder keeps, what the schema check and the
//! Chrome converter read, and the field visitor every layer uses.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};

use crate::names::REMOTE_PARENT_FIELD;
use crate::redact::Redactor;
use crate::traceparent::{self, TraceParent};

/// One span, from its open to its close.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanRecord {
    /// Its record id, unique within its process for the whole recording.
    pub id: u64,
    /// Its `tracing` span id, the one a `traceparent` names (reused after the span closes, and
    /// handed out by every process from the same small range).
    pub span_id: u64,
    /// Its parent's record id, in the same process.
    pub parent: Option<u64>,
    /// Its parent in another process, from its `traceparent` field: the trace (the turn's ULID,
    /// or a job's with no turn) and the parent's span id in that process.
    pub remote_parent: Option<TraceParent>,
    /// Its name, from [`crate::names`].
    pub name: String,
    /// The process that recorded it (`eileen`, `eileen-speech`, ...).
    pub process: String,
    /// When it opened, microseconds since the Unix epoch.
    pub start_us: i64,
    /// When it closed; `None` while it is open.
    pub end_us: Option<i64>,
    /// Its fields, sorted by name.
    pub fields: BTreeMap<String, Value>,
}

/// One event.
#[derive(Debug, Clone, PartialEq)]
pub struct EventRecord {
    /// Its name (`span.slow`, or tracing's default `event file:line`).
    pub name: String,
    /// Its level.
    pub level: String,
    /// Its target.
    pub target: String,
    /// The record id of the span it happened in.
    pub span: Option<u64>,
    /// When, microseconds since the Unix epoch.
    pub at_us: i64,
    /// Its fields, sorted by name (the message is `message`).
    pub fields: BTreeMap<String, Value>,
}

/// Collects a span's or an event's fields as JSON values.
#[derive(Default)]
pub(crate) struct JsonVisitor(pub(crate) Map<String, Value>);

impl Visit for JsonVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), json!(value));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.0.insert(field.name().to_owned(), json!(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().to_owned(), json!(format!("{value:?}")));
    }
}

/// The parent a `traceparent` field names, when it holds a valid one.
pub(crate) fn remote_parent(fields: &Map<String, Value>) -> Option<TraceParent> {
    fields.get(REMOTE_PARENT_FIELD).and_then(Value::as_str).and_then(traceparent::parse)
}

/// Runs every value of `fields` (nested ones included) through `redactor`, before the map is
/// serialized, so a literal secret is matched raw rather than JSON-escaped. A string is
/// redacted in place; a number whose digits hold a match (a card number, which the digits
/// after a decimal point never are) becomes `0`, so a negative card or one written `.0` leaves
/// valid JSON too. True when anything was replaced.
pub(crate) fn redact_values(fields: &mut Map<String, Value>, redactor: &Redactor) -> bool {
    let mut replaced = false;
    for value in fields.values_mut() {
        replaced |= redact_value(value, redactor);
    }
    replaced
}

fn redact_value(value: &mut Value, redactor: &Redactor) -> bool {
    match value {
        Value::String(text) => match redactor.redact(text) {
            Cow::Owned(clean) => {
                *text = clean;
                true
            }
            Cow::Borrowed(_) => false,
        },
        Value::Number(number) => {
            let found = matches!(redactor.redact(&number.to_string()), Cow::Owned(_));
            if found {
                *value = Value::from(0);
            }
            found
        }
        Value::Array(items) => {
            items.iter_mut().fold(false, |replaced, item| redact_value(item, redactor) | replaced)
        }
        Value::Object(map) => redact_values(map, redactor),
        Value::Null | Value::Bool(_) => false,
    }
}

/// Sorted copy of a field map.
pub(crate) fn sorted(fields: &Map<String, Value>) -> BTreeMap<String, Value> {
    fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// A source path as the logs keep it, so no line carries the home folder of the PC that built
/// the binary. A path relative to the workspace (every Eileen crate's `file!()`) and one under
/// `/rustc/` (the standard library's) stay as they are. An absolute path, which a registry
/// crate's events and a dependency's `log` records carry, is cut to the part from the crate's
/// `<name>-<version>` folder when it lies in Cargo's registry, and to its file name otherwise.
#[must_use]
pub fn source_file(path: &str) -> Cow<'_, str> {
    let bytes = path.as_bytes();
    let absolute = matches!(bytes.first(), Some(b'/' | b'\\')) || bytes.get(1) == Some(&b':');
    if !absolute || path.starts_with("/rustc/") {
        return Cow::Borrowed(path);
    }
    let path = path.replace('\\', "/");
    let registry = "/registry/src/";
    let in_registry = path
        .find(registry)
        .and_then(|at| path.get(at + registry.len()..))
        .and_then(|rest| rest.split_once('/'))
        .map(|(_index, from_crate)| from_crate.to_owned());
    Cow::Owned(
        in_registry.unwrap_or_else(|| path.rsplit('/').next().unwrap_or_default().to_owned()),
    )
}

/// An event's name as the ring and the test capture keep it: a name given with `name:` as it
/// is, and `tracing`'s default, `event <file>:<line>`, with the file cut by [`source_file`].
pub(crate) fn event_name(name: &'static str) -> Cow<'static, str> {
    let Some(location) = name.strip_prefix("event ") else {
        return Cow::Borrowed(name);
    };
    let (file, line) = location.rsplit_once(':').unwrap_or((location, ""));
    match source_file(file) {
        Cow::Borrowed(_) => Cow::Borrowed(name),
        Cow::Owned(file) => Cow::Owned(format!("event {file}:{line}")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{event_name, redact_values, source_file};
    use crate::redact::Redactor;

    #[test]
    fn a_card_in_any_field_number_becomes_zero() {
        let r = Redactor::new();
        let mut fields = json!({
            "amount": 4_111_111_111_111_111_u64,
            "balance": -4_111_111_111_111_111_i64,
            "ratio": 4_111_111_111_111_111.0_f64,
            "nested": {"cards": [4_111_111_111_111_111_u64, 7]},
            "count": 7,
        });
        assert!(redact_values(fields.as_object_mut().unwrap(), &r), "something was replaced");
        assert_eq!(
            fields,
            json!({"amount": 0, "balance": 0, "ratio": 0, "nested": {"cards": [0, 7]}, "count": 7}),
            "every card became 0, other numbers stay"
        );
        let mut clean = json!({"count": 7, "turn_id": "turn_1"});
        assert!(!redact_values(clean.as_object_mut().unwrap(), &r), "nothing to replace");
    }

    #[test]
    fn a_fraction_keeps_its_digits() {
        let r = Redactor::new();
        // The 16 digits after each point pass the card rule's Luhn check.
        let mut fields = json!({"ratio": 6.0_f64 / 11.0, "score": 0.590_089_667_194_192_1_f64});
        let before = fields.clone();
        assert!(!redact_values(fields.as_object_mut().unwrap(), &r), "nothing to replace");
        assert_eq!(fields, before, "both numbers stay as they are");
    }

    #[test]
    fn workspace_and_standard_library_paths_stay() {
        for path in [
            "crates/eileen-trace/src/file.rs",
            r"crates\eileen-trace\src\file.rs",
            "/rustc/1a2b3c/library/std/src/panicking.rs",
        ] {
            assert_eq!(source_file(path), path, "{path}");
        }
    }

    #[test]
    fn a_registry_path_loses_the_home_folder() {
        let windows = r"C:\Users\someone\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\rmcp-3.6.0\src\service.rs";
        assert_eq!(source_file(windows), "rmcp-3.6.0/src/service.rs", "Windows");
        let linux = "/home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/zbus-5.19.0/src/lib.rs";
        assert_eq!(source_file(linux), "zbus-5.19.0/src/lib.rs", "Linux");
        assert_eq!(
            source_file(r"D:\work\vendored\thing\src\lib.rs"),
            "lib.rs",
            "an absolute path outside the registry keeps its file name only"
        );
        assert_eq!(source_file(r"\\server\share\src\lib.rs"), "lib.rs", "a UNC path too");
    }

    #[test]
    fn a_default_event_name_is_cut_the_same_way() {
        let name = r"event C:\Users\someone\.cargo\registry\src\index.crates.io-1\tokio-1.53.3\src\lib.rs:42";
        assert_eq!(event_name(name), "event tokio-1.53.3/src/lib.rs:42", "cut, line kept");
        assert_eq!(
            event_name("event crates/eileen-rt/src/task.rs:9"),
            "event crates/eileen-rt/src/task.rs:9",
            "relative"
        );
        assert_eq!(event_name("task.panicked"), "task.panicked", "a declared name");
    }
}

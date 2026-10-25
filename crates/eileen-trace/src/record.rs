//! Span and event records: what the flight recorder keeps, what the schema check and the
//! Chrome converter read, and the field visitor every layer uses.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};

use crate::names::REMOTE_PARENT_FIELD;
use crate::redact::Redactor;
use crate::traceparent;

/// One span, from its open to its close.
#[derive(Debug, Clone, PartialEq)]
pub struct SpanRecord {
    /// Its record id, unique within its process for the whole recording.
    pub id: u64,
    /// Its `tracing` span id, the one a `traceparent` names (reused after the span closes).
    pub span_id: u64,
    /// Its parent's record id, in the same process.
    pub parent: Option<u64>,
    /// Its parent's span id in another process, from its `traceparent` field.
    pub remote_parent: Option<u64>,
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

/// The span id a `traceparent` field names, when it holds a valid one.
pub(crate) fn remote_parent(fields: &Map<String, Value>) -> Option<u64> {
    fields
        .get(REMOTE_PARENT_FIELD)
        .and_then(Value::as_str)
        .and_then(traceparent::parse)
        .map(|tp| tp.parent_id)
}

/// Runs every string value of `fields` (nested ones included) through `redactor`, before the
/// map is serialized, so a literal secret is matched raw rather than JSON-escaped.
pub(crate) fn redact_values(fields: &mut Map<String, Value>, redactor: &Redactor) {
    for value in fields.values_mut() {
        redact_value(value, redactor);
    }
}

fn redact_value(value: &mut Value, redactor: &Redactor) {
    match value {
        Value::String(text) => {
            if let std::borrow::Cow::Owned(clean) = redactor.redact(text) {
                *text = clean;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| redact_value(item, redactor)),
        Value::Object(map) => redact_values(map, redactor),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Sorted copy of a field map.
pub(crate) fn sorted(fields: &Map<String, Value>) -> BTreeMap<String, Value> {
    fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

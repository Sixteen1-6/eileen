//! The `pii.` policy: user content travels only in fields whose name starts with `pii.`
//! (`pii.transcript`, `pii.title`, `pii.path`), and each sink decides what to do with them.
//!
//! | Sink | `pii.` fields |
//! |---|---|
//! | Flight recorder (memory) | kept; secrets scanned at dump time |
//! | JSON log files | length and keyed hash, unless `logging.include_personal` |
//! | Console | kept (a terminal the owner is watching); secrets redacted |
//! | A span trace, text in another field that any sink can carry | length alone, always |
//!
//! The hash's key is per install: the runtime derives it at setup and passes it to every
//! process it starts. With no key the files keep the length alone, so a process started
//! without one never hashes user text under a key anyone could know.

use std::fmt;

use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};
use tracing_subscriber::field::{MakeVisitor, VisitFmt, VisitOutput};
use tracing_subscriber::fmt::format::{DefaultVisitor, Writer};

/// The prefix that marks a field as user content.
pub const PREFIX: &str = "pii.";

/// The per-install key of the files' keyed hash. Its `Debug` prints `PiiKey(..)`, so a
/// configuration, a policy or a panic message formatted with `{:?}` never shows it: the key's
/// secrecy is what stops anyone matching a reduced `{len, h}` against a dictionary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PiiKey([u8; 32]);

impl PiiKey {
    /// A key from its 32 bytes (the runtime derives them at setup).
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for PiiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PiiKey(..)")
    }
}

/// True for a field that carries user content.
#[must_use]
pub fn is_pii(field: &str) -> bool {
    field.starts_with(PREFIX)
}

/// What a sink does with `pii.` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Keep the value.
    Keep,
    /// Replace the value with `{"len": <chars>, "h": "<8 hex>"}`, a keyed hash, so two lines
    /// about the same window title can be matched without the title; with no key, with
    /// `{"len": <chars>}` alone.
    Reduce {
        /// The key (per install, from the runtime); `None` keeps the length alone.
        key: Option<PiiKey>,
    },
}

/// The reduced form of one value: its length, and its keyed hash when there is a key.
#[must_use]
pub fn reduce(value: &Value, key: Option<&PiiKey>) -> Value {
    let text = if let Value::String(s) = value { s.clone() } else { value.to_string() };
    let len = text.chars().count();
    match key {
        Some(key) => {
            let hash = blake3::keyed_hash(key.bytes(), text.as_bytes());
            let [a, b, c, d, ..] = *hash.as_bytes();
            json!({ "len": len, "h": format!("{a:02x}{b:02x}{c:02x}{d:02x}") })
        }
        None => json!({ "len": len }),
    }
}

/// Applies `policy` to every `pii.` field of `fields`, in place.
pub fn apply(fields: &mut Map<String, Value>, policy: Policy) {
    let Policy::Reduce { key } = policy else {
        return;
    };
    for (name, value) in fields.iter_mut() {
        if is_pii(name) {
            *value = reduce(value, key.as_ref());
        }
    }
}

/// How the error layer writes a span's fields for a `tracing_error::SpanTrace`: as
/// tracing-subscriber's `DefaultFields` does, except that a `pii.` field shows as its reduced
/// form without a key, `{"len":N}`. A span trace travels as text in another field (`error =
/// %e`, the panic hook's record), where no sink's policy can find the `pii.` values, so it
/// never holds them, whatever `logging.include_personal` says.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SpanTraceFields;

impl<'a> MakeVisitor<Writer<'a>> for SpanTraceFields {
    type Visitor = SpanTraceVisitor<'a>;

    fn make_visitor(&self, target: Writer<'a>) -> Self::Visitor {
        SpanTraceVisitor(DefaultVisitor::new(target, true))
    }
}

/// The visitor [`SpanTraceFields`] makes.
#[derive(Debug)]
pub(crate) struct SpanTraceVisitor<'a>(DefaultVisitor<'a>);

impl SpanTraceVisitor<'_> {
    /// Writes `field` as `value` reduced, with the length the files would give it.
    fn reduced(&mut self, field: &Field, value: &Value) {
        let reduced = reduce(value, None);
        self.0.record_debug(field, &format_args!("{reduced}"));
    }
}

impl Visit for SpanTraceVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if is_pii(field.name()) {
            self.reduced(field, &Value::from(value));
        } else {
            self.0.record_str(field, value);
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        if is_pii(field.name()) {
            self.reduced(field, &Value::from(value.to_string()));
        } else {
            self.0.record_error(field, value);
        }
    }

    // Numbers, booleans and bytes come here too, through `Visit`'s own defaults.
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if is_pii(field.name()) {
            self.reduced(field, &Value::from(format!("{value:?}")));
        } else {
            self.0.record_debug(field, value);
        }
    }
}

impl VisitOutput<fmt::Result> for SpanTraceVisitor<'_> {
    fn finish(self) -> fmt::Result {
        self.0.finish()
    }
}

impl VisitFmt for SpanTraceVisitor<'_> {
    fn writer(&mut self) -> &mut dyn fmt::Write {
        self.0.writer()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};
    use tracing_error::{ErrorLayer, SpanTrace};
    use tracing_subscriber::Registry;
    use tracing_subscriber::fmt::FormatFields;
    use tracing_subscriber::fmt::format::DefaultFields;
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::{PiiKey, Policy, SpanTraceFields, apply, is_pii, reduce};

    /// An error with a source, which `DefaultFields` lists after it.
    #[derive(Debug, thiserror::Error)]
    #[error("refused")]
    struct Refused(#[source] std::io::Error);

    /// The span trace inside the span `make` opens, under an error layer that writes span
    /// fields with `fields`.
    fn trace_of<F>(fields: F, make: fn() -> tracing::Span) -> String
    where
        F: for<'w> FormatFields<'w> + Send + Sync + 'static,
    {
        let subscriber =
            tracing_subscriber::registry().with(ErrorLayer::<Registry, F>::new(fields));
        tracing::subscriber::with_default(subscriber, || {
            make().in_scope(|| SpanTrace::capture().to_string())
        })
    }

    #[test]
    fn a_span_trace_shows_pii_fields_as_their_length() {
        fn personal() -> tracing::Span {
            let error: &(dyn std::error::Error + 'static) =
                &Refused(std::io::Error::other("the bus is gone"));
            tracing::info_span!(
                "turn",
                turn_id = "t",
                "pii.transcript" = "my diary entry",
                "pii.title" = ?"Holiday photos",
                "pii.path" = %"/home/someone/notes.txt",
                "pii.count" = 12_345,
                "pii.error" = error,
            )
        }
        let trace = trace_of(SpanTraceFields, personal);
        // The lengths the files give the same values: `Debug` keeps its quotes, an error is
        // its `Display`, a number its digits.
        let expected = r#"with turn_id="t" pii.transcript={"len":14} pii.title={"len":16} pii.path={"len":23} pii.count={"len":5} pii.error={"len":7}"#;
        assert!(trace.contains(expected), "each as its length alone:\n{trace}");
        for value in ["my diary", "Holiday", "/home/someone", "12345", "refused", "the bus"] {
            assert!(!trace.contains(value), "{value} reached the span trace:\n{trace}");
        }
    }

    #[test]
    fn a_span_trace_shows_other_fields_as_default_fields_does() {
        fn ordinary() -> tracing::Span {
            let error: &(dyn std::error::Error + 'static) =
                &Refused(std::io::Error::other("the bus is gone"));
            tracing::info_span!("job", job_id = "job_1", count = 3, ok = true, shown = %"a b", debug = ?"c", error)
        }
        let trace = trace_of(SpanTraceFields, ordinary);
        let expected = r#"with job_id="job_1" count=3 ok=true shown=a b debug="c" error=refused error.sources=[the bus is gone]"#;
        assert!(trace.contains(expected), "every field shown:\n{trace}");
        assert_eq!(trace, trace_of(DefaultFields::new(), ordinary), "as tracing-error's default");
    }

    #[test]
    fn reduce_keeps_the_length_and_a_keyed_hash() {
        let key = PiiKey::new([7; 32]);
        let reduced = reduce(&json!("Quarterly report.docx"), Some(&key));
        assert_eq!(reduced["len"], 21, "length in characters");
        assert_eq!(reduced["h"].as_str().map(str::len), Some(8), "8 hex digits");
        assert_eq!(reduce(&json!("Quarterly report.docx"), Some(&key)), reduced, "stable");
        let other = PiiKey::new([8; 32]);
        assert_ne!(reduce(&json!("Quarterly report.docx"), Some(&other)), reduced, "keyed");
    }

    #[test]
    fn the_key_never_shows_in_debug() {
        let policy = Policy::Reduce { key: Some(PiiKey::new([0xAB; 32])) };
        let shown = format!("{policy:?}");
        assert!(shown.contains("PiiKey(..)"), "{shown}");
        assert!(!shown.contains("171") && !shown.contains("ab"), "no byte of the key: {shown}");
    }

    #[test]
    fn without_a_key_only_the_length_is_kept() {
        assert_eq!(reduce(&json!("My diary"), None), json!({ "len": 8 }), "no hash");
    }

    #[test]
    fn apply_touches_only_pii_fields() {
        let mut fields: Map<String, Value> = Map::new();
        fields.insert("pii.title".to_owned(), json!("My diary"));
        fields.insert("tool".to_owned(), json!("desktop_tree"));
        apply(&mut fields, Policy::Reduce { key: Some(PiiKey::new([3; 32])) });
        assert!(fields["pii.title"].get("h").is_some(), "reduced");
        assert_eq!(fields["tool"], "desktop_tree", "left alone");
        assert!(is_pii("pii.transcript") && !is_pii("piixel"), "prefix");
    }
}

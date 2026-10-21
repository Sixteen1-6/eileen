//! The JSON-lines file sink: one line per event at INFO, with the event's span and the span
//! list, `pii.` fields reduced to length and keyed hash unless `logging.include_personal`,
//! and every line through the Redactor before it is written.

use std::io::{self, Write};
use std::sync::Arc;

use eileen_contracts::ids::WallTime;
use eileen_stdx::clock::{self, Clock as _};
use serde_json::{Map, Value, json};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::pii::{self, Policy};
use crate::record::JsonVisitor;
use crate::redact::Redactor;

/// A span's fields as the file layer saw them, raw (the policy applies at write time).
struct FileFields(Map<String, Value>);

/// The file layer, writing through `W` (a non-blocking rolling file in production, memory
/// in tests).
pub struct FileLayer<W> {
    process: &'static str,
    writer: W,
    policy: Policy,
    redactor: Arc<Redactor>,
}

impl<W> std::fmt::Debug for FileLayer<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileLayer").field("process", &self.process).finish_non_exhaustive()
    }
}

impl<W> FileLayer<W> {
    /// A layer for `process` writing to `writer`.
    pub const fn new(
        process: &'static str,
        writer: W,
        policy: Policy,
        redactor: Arc<Redactor>,
    ) -> Self {
        Self { process, writer, policy, redactor }
    }
}

/// `YYYY-MM-DDTHH:MM:SS.ffffffZ` of a wall time.
#[must_use]
pub fn rfc3339(at: WallTime) -> String {
    let date = at.date();
    let of_day = at.0.rem_euclid(86_400_000_000);
    let (secs, micros) = (of_day / 1_000_000, of_day % 1_000_000);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{micros:06}Z",
        date.y,
        date.m,
        date.d,
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    )
}

impl<S, W> Layer<S> for FileLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = JsonVisitor::default();
        attrs.record(&mut visitor);
        span.extensions_mut().insert(FileFields(visitor.0));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = JsonVisitor::default();
        values.record(&mut visitor);
        if let Some(fields) = span.extensions_mut().get_mut::<FileFields>() {
            fields.0.extend(visitor.0);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let mut fields = visitor.0;
        pii::apply(&mut fields, self.policy);
        let mut spans = Vec::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                let mut object =
                    span.extensions().get::<FileFields>().map(|f| f.0.clone()).unwrap_or_default();
                pii::apply(&mut object, self.policy);
                object.insert("name".to_owned(), json!(span.name()));
                spans.push(Value::Object(object));
            }
        }
        let meta = event.metadata();
        let mut line = Map::new();
        line.insert("timestamp".to_owned(), json!(rfc3339(clock::system().wall())));
        line.insert("level".to_owned(), json!(meta.level().as_str()));
        line.insert("target".to_owned(), json!(meta.target()));
        line.insert("filename".to_owned(), json!(meta.file()));
        line.insert("line_number".to_owned(), json!(meta.line()));
        line.insert("threadName".to_owned(), json!(std::thread::current().name()));
        line.insert("process".to_owned(), json!(self.process));
        line.insert("fields".to_owned(), Value::Object(fields));
        if let Some(current) = spans.last() {
            line.insert("span".to_owned(), current.clone());
        }
        line.insert("spans".to_owned(), Value::Array(spans));
        let mut text = self.redactor.redact(&Value::Object(line).to_string()).into_owned();
        text.push('\n');
        // A full or failed writer drops the line rather than block an always-on process.
        let _ignored: io::Result<()> = self.writer.make_writer().write_all(text.as_bytes());
    }
}

/// A `MakeWriter` that runs every write through the Redactor, for the `fmt` layers of sinks
/// that must not hold secrets.
#[derive(Debug, Clone)]
pub struct RedactingMakeWriter<M> {
    inner: M,
    redactor: Arc<Redactor>,
}

impl<M> RedactingMakeWriter<M> {
    /// Wraps `inner`.
    pub const fn new(inner: M, redactor: Arc<Redactor>) -> Self {
        Self { inner, redactor }
    }
}

/// The writer [`RedactingMakeWriter`] makes.
#[derive(Debug)]
pub struct RedactingWriter<W> {
    inner: W,
    redactor: Arc<Redactor>,
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.inner.write_all(self.redactor.redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'w, M: MakeWriter<'w>> MakeWriter<'w> for RedactingMakeWriter<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'w self) -> Self::Writer {
        RedactingWriter { inner: self.inner.make_writer(), redactor: Arc::clone(&self.redactor) }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::ids::WallTime;

    use super::rfc3339;

    #[test]
    fn timestamps_are_rfc3339_utc_with_microseconds() {
        assert_eq!(rfc3339(WallTime(0)), "1970-01-01T00:00:00.000000Z", "epoch");
        assert_eq!(
            rfc3339(WallTime(1_761_004_800_123_456)),
            "2025-10-21T00:00:00.123456Z",
            "a day"
        );
        assert_eq!(
            rfc3339(WallTime(1_761_053_462_000_007)),
            "2025-10-21T13:31:02.000007Z",
            "a time"
        );
    }
}

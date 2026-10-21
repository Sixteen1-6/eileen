//! The flight recorder: the last minutes of span and event records in memory, DEBUG for
//! Eileen's crates, dumped when something goes wrong (observability report section 4.1).
//!
//! Each record is one compact JSON line: `{"t":"open"|"record"|"close"|"event", ...}`. Span
//! ids in the lines are record ids, unique for the life of the process, because `tracing`
//! reuses a span's id once it closes. `pii.` fields are kept in memory; secrets are scanned
//! when the ring is dumped, never later.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eileen_stdx::clock::{self, Clock as _};
use serde_json::{Map, Value, json};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::record::{EventRecord, JsonVisitor, SpanRecord, remote_parent, sorted};
use crate::redact::Redactor;

struct Inner {
    lines: VecDeque<(i64, String)>,
    bytes: usize,
}

/// The ring of one process.
pub struct Ring {
    process: &'static str,
    max_bytes: usize,
    window_us: i64,
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    dump_requests: AtomicU64,
}

impl std::fmt::Debug for Ring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ring")
            .field("process", &self.process)
            .field("max_bytes", &self.max_bytes)
            .field("bytes", &self.lock().bytes)
            .finish_non_exhaustive()
    }
}

impl Ring {
    /// A ring for `process` keeping at most `max_bytes` and at most `window` of records.
    #[must_use]
    pub fn new(process: &'static str, max_bytes: usize, window: Duration) -> Self {
        Self {
            process,
            max_bytes,
            window_us: eileen_stdx::num::micros_i64(window),
            inner: Mutex::new(Inner { lines: VecDeque::new(), bytes: 0 }),
            next_id: AtomicU64::new(1),
            dump_requests: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, at_us: i64, line: String) {
        let mut inner = self.lock();
        inner.bytes += line.len();
        inner.lines.push_back((at_us, line));
        let oldest_kept = at_us.saturating_sub(self.window_us);
        while inner.bytes > self.max_bytes
            || inner.lines.front().is_some_and(|(at, _)| *at < oldest_kept)
        {
            let Some((_, dropped)) = inner.lines.pop_front() else { break };
            inner.bytes -= dropped.len();
        }
        drop(inner);
    }

    /// The bytes held now.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.lock().bytes
    }

    /// The records as JSON lines, every line through `redactor` (`events.jsonl` of an
    /// incident bundle).
    #[must_use]
    pub fn dump(&self, redactor: &Redactor) -> Vec<u8> {
        let lines: Vec<String> = self.lock().lines.iter().map(|(_, l)| l.clone()).collect();
        let mut out = Vec::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
        for line in &lines {
            out.extend_from_slice(redactor.redact(line).as_bytes());
            out.push(b'\n');
        }
        out
    }

    /// The span and event records, parsed from a redacted dump.
    #[must_use]
    pub fn records(&self, redactor: &Redactor) -> Parsed {
        let dump = self.dump(redactor);
        parse_lines(String::from_utf8_lossy(&dump).lines())
    }

    /// Asks the incident task to dump the rings (a soft assertion failed).
    pub fn request_dump(&self) {
        self.dump_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// True once for each dump requested since the last call.
    #[must_use]
    pub fn take_dump_request(&self) -> bool {
        self.dump_requests
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_ok()
    }
}

/// What [`parse_lines`] read back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    /// The spans, by start.
    pub spans: Vec<SpanRecord>,
    /// The events, in order.
    pub events: Vec<EventRecord>,
    /// Lines that were not records (not a JSON object, or no known `t`), counted so a timeline
    /// or a schema check that lost lines says so.
    pub skipped: usize,
}

/// Turns ring lines (or the merged `events.jsonl` of an incident) back into records. Blank
/// lines are ignored; every other line that is not a record is counted in
/// [`Parsed::skipped`].
pub fn parse_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Parsed {
    let mut spans: BTreeMap<(String, u64), SpanRecord> = BTreeMap::new();
    let mut events = Vec::new();
    let mut skipped = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(Value::Object(o)) = serde_json::from_str::<Value>(line) else {
            skipped += 1;
            continue;
        };
        let process = o.get("process").and_then(Value::as_str).unwrap_or_default().to_owned();
        let id = o.get("id").and_then(Value::as_u64).unwrap_or(0);
        let fields = o.get("fields").and_then(Value::as_object).cloned().unwrap_or_default();
        let at_us = o.get("at_us").and_then(Value::as_i64).unwrap_or(0);
        match o.get("t").and_then(Value::as_str) {
            Some("open") => {
                spans.insert(
                    (process.clone(), id),
                    SpanRecord {
                        id,
                        span_id: o.get("sid").and_then(Value::as_u64).unwrap_or(0),
                        parent: o.get("parent").and_then(Value::as_u64),
                        remote_parent: remote_parent(&fields),
                        name: o.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        process,
                        start_us: at_us,
                        end_us: None,
                        fields: sorted(&fields),
                    },
                );
            }
            Some("record") => {
                if let Some(span) = spans.get_mut(&(process, id)) {
                    span.fields.extend(fields);
                }
            }
            Some("close") => {
                if let Some(span) = spans.get_mut(&(process, id)) {
                    span.end_us = Some(at_us);
                }
            }
            Some("event") => events.push(EventRecord {
                name: o.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                level: o.get("level").and_then(Value::as_str).unwrap_or_default().to_owned(),
                target: o.get("target").and_then(Value::as_str).unwrap_or_default().to_owned(),
                span: o.get("span").and_then(Value::as_u64),
                at_us,
                fields: sorted(&fields),
            }),
            _ => skipped += 1,
        }
    }
    let mut spans: Vec<SpanRecord> = spans.into_values().collect();
    spans.sort_by_key(|s| (s.start_us, s.id));
    Parsed { spans, events, skipped }
}

/// The record id a layer gave a span, kept in the span's extensions.
pub(crate) struct RecordId(pub(crate) u64);

/// The layer that fills a [`Ring`].
#[derive(Debug, Clone)]
pub struct RingLayer {
    ring: Arc<Ring>,
}

impl RingLayer {
    /// A layer writing into `ring`.
    #[must_use]
    pub const fn new(ring: Arc<Ring>) -> Self {
        Self { ring }
    }
}

fn now_us() -> i64 {
    clock::system().wall().0
}

impl<S> Layer<S> for RingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = JsonVisitor::default();
        attrs.record(&mut visitor);
        let uid = self.ring.next_id.fetch_add(1, Ordering::Relaxed);
        let parent = span.parent().and_then(|p| p.extensions().get::<RecordId>().map(|r| r.0));
        span.extensions_mut().insert(RecordId(uid));
        let at_us = now_us();
        let line = json!({
            "t": "open", "at_us": at_us, "id": uid, "sid": id.into_u64(), "parent": parent,
            "name": span.name(), "target": span.metadata().target(), "process": self.ring.process,
            "fields": Value::Object(visitor.0),
        });
        self.ring.push(at_us, line.to_string());
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(id).and_then(|s| s.extensions().get::<RecordId>().map(|r| r.0))
        else {
            return;
        };
        let mut visitor = JsonVisitor::default();
        values.record(&mut visitor);
        let at_us = now_us();
        let line = json!({"t": "record", "at_us": at_us, "id": uid, "process": self.ring.process, "fields": Value::Object(visitor.0)});
        self.ring.push(at_us, line.to_string());
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let span =
            ctx.event_span(event).and_then(|s| s.extensions().get::<RecordId>().map(|r| r.0));
        let meta = event.metadata();
        let at_us = now_us();
        let mut line = Map::new();
        line.insert("t".to_owned(), json!("event"));
        line.insert("at_us".to_owned(), json!(at_us));
        line.insert("level".to_owned(), json!(meta.level().as_str()));
        line.insert("target".to_owned(), json!(meta.target()));
        line.insert("name".to_owned(), json!(meta.name()));
        line.insert("span".to_owned(), json!(span));
        line.insert("process".to_owned(), json!(self.ring.process));
        line.insert("fields".to_owned(), Value::Object(visitor.0));
        self.ring.push(at_us, Value::Object(line).to_string());
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(&id).and_then(|s| s.extensions().get::<RecordId>().map(|r| r.0))
        else {
            return;
        };
        let at_us = now_us();
        let line = json!({"t": "close", "at_us": at_us, "id": uid, "process": self.ring.process});
        self.ring.push(at_us, line.to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tracing_subscriber::layer::SubscriberExt as _;

    use super::{Ring, RingLayer};
    use crate::redact::Redactor;

    #[test]
    fn records_round_trip_and_the_ring_stays_bounded() {
        let ring = Arc::new(Ring::new("eileen", 4_096, Duration::from_secs(600)));
        let subscriber = tracing_subscriber::registry().with(RingLayer::new(Arc::clone(&ring)));
        tracing::subscriber::with_default(subscriber, || {
            let turn = tracing::info_span!("turn", turn_id = "turn_1");
            let _entered = turn.enter();
            tracing::info!(n = 1, "inside");
        });
        let super::Parsed { spans, events, skipped } = ring.records(&Redactor::new());
        assert_eq!(skipped, 0, "every line was a record");
        assert_eq!(spans.len(), 1, "one span");
        assert!(spans[0].end_us.is_some(), "closed");
        assert_eq!(events[0].span, Some(spans[0].id), "the event is inside the span");
        let big = "x".repeat(1_000);
        for _ in 0..20 {
            ring.push(0, big.clone());
        }
        assert!(ring.bytes() <= 4_096, "bounded by bytes");
        ring.request_dump();
        assert!(ring.take_dump_request(), "requested");
        assert!(!ring.take_dump_request(), "once");
    }

    #[test]
    fn lines_that_are_not_records_are_counted() {
        let lines = ["", "not json", r#"{"t":"event","name":"x","at_us":1}"#, r#"{"t":"?"}"#, "[1]"];
        let parsed = super::parse_lines(lines);
        assert_eq!(parsed.events.len(), 1, "one event");
        assert_eq!(parsed.skipped, 3, "the blank line is not counted, the other three are");
    }
}

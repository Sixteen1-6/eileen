//! The simulator's span collector: every span and event the core produces on the simulator's
//! thread, kept as [`SpanRecord`]s and [`EventRecord`]s as it happens, so invariant 13 checks
//! the whole run after every step without dumping and re-parsing a flight-recorder ring.
//!
//! Times are a counter of records, not the clock, so a run's records are the same run after
//! run and a child's close is ordered against its parent's exactly as the subscriber closed
//! them. The check runs again only after a step that produced a record.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use eileen_trace::{EventRecord, SpanRecord};
use serde_json::Value;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::invariants::{self, Violation};

/// The process the records name, the core's.
const PROCESS: &str = "eileen";

/// The records so far.
#[derive(Debug, Default)]
struct Records {
    spans: Vec<SpanRecord>,
    events: Vec<EventRecord>,
    clock: i64,
    /// A record arrived since the last check.
    changed: bool,
    /// The last check's verdict.
    verdict: Option<Violation>,
}

impl Records {
    const fn tick(&mut self) -> i64 {
        self.clock = self.clock.saturating_add(1);
        self.changed = true;
        self.clock
    }

    fn span_mut(&mut self, uid: u64) -> Option<&mut SpanRecord> {
        let index = usize::try_from(uid.checked_sub(1)?).ok()?;
        self.spans.get_mut(index)
    }
}

/// The record id a span carries in the registry's extensions.
#[derive(Debug, Clone, Copy)]
struct Uid(u64);

/// A span's or an event's fields, as JSON values.
#[derive(Debug, Default)]
struct Fields(BTreeMap<String, Value>);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.insert(field.name().to_owned(), Value::String(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), Value::String(value.to_owned()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), Value::Bool(value));
    }
}

/// The collector: a handle on the records, and the layer that writes them.
#[derive(Debug, Clone, Default)]
pub struct Collector {
    records: Arc<Mutex<Records>>,
}

impl Collector {
    /// The layer that feeds this collector.
    #[must_use]
    pub fn layer(&self) -> CollectLayer {
        CollectLayer { records: Arc::clone(&self.records) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Records> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Invariant 13 over every record so far, checked again only when a record arrived.
    #[must_use]
    pub fn check(&self) -> Option<Violation> {
        let mut records = self.lock();
        if records.changed {
            records.verdict =
                invariants::spans_follow_the_table(&records.spans, &records.events).err();
            records.changed = false;
        }
        records.verdict.clone()
    }

    /// How many spans the core has opened.
    #[must_use]
    pub fn spans(&self) -> usize {
        self.lock().spans.len()
    }
}

/// Writes spans and events into a [`Collector`].
#[derive(Debug)]
pub struct CollectLayer {
    records: Arc<Mutex<Records>>,
}

impl CollectLayer {
    fn lock(&self) -> std::sync::MutexGuard<'_, Records> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<S> Layer<S> for CollectLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let parent = span.parent().and_then(|p| p.extensions().get::<Uid>().map(|uid| uid.0));
        let mut records = self.lock();
        let uid = u64::try_from(records.spans.len()).unwrap_or(u64::MAX).saturating_add(1);
        let start_us = records.tick();
        records.spans.push(SpanRecord {
            id: uid,
            span_id: id.into_u64(),
            parent,
            remote_parent: None,
            name: span.name().to_owned(),
            process: PROCESS.to_owned(),
            start_us,
            end_us: None,
            fields: fields.0,
        });
        drop(records);
        span.extensions_mut().insert(Uid(uid));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(id).and_then(|s| s.extensions().get::<Uid>().map(|u| u.0)) else {
            return;
        };
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut records = self.lock();
        records.tick();
        if let Some(span) = records.span_mut(uid) {
            span.fields.extend(fields.0);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let span = ctx.event_span(event).and_then(|s| s.extensions().get::<Uid>().map(|u| u.0));
        let meta = event.metadata();
        let mut records = self.lock();
        let at_us = records.tick();
        records.events.push(EventRecord {
            name: meta.name().to_owned(),
            level: meta.level().as_str().to_owned(),
            target: meta.target().to_owned(),
            span,
            at_us,
            fields: fields.0,
        });
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(&id).and_then(|s| s.extensions().get::<Uid>().map(|u| u.0)) else {
            return;
        };
        let mut records = self.lock();
        let end_us = records.tick();
        if let Some(span) = records.span_mut(uid) {
            span.end_us = Some(end_us);
        }
    }
}

#[cfg(test)]
mod tests {
    use eileen_trace::names;
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::Collector;

    #[test]
    fn spans_are_collected_with_their_parents_and_checked() {
        let collector = Collector::default();
        let subscriber = tracing_subscriber::registry().with(collector.layer());
        tracing::subscriber::with_default(subscriber, || {
            let turn = tracing::info_span!(parent: None, names::TURN, turn_id = "turn_01K8ZZZZZZZZZZZZZZZZZZZZZZ");
            let decide =
                tracing::info_span!(parent: turn.id(), names::ADDRESSEE, decision = "name");
            drop(decide);
            drop(turn);
        });
        assert_eq!(collector.spans(), 2, "both spans");
        assert_eq!(collector.check(), None, "addressee sits under turn");
        let wrong = tracing_subscriber::registry().with(collector.layer());
        tracing::subscriber::with_default(wrong, || {
            let speak = tracing::info_span!(parent: None, names::BARGEIN);
            drop(speak);
        });
        assert!(collector.check().is_some(), "a barge-in may not be a root");
    }
}

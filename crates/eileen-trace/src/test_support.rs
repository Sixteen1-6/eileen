//! An in-memory capture layer for tests in every crate, with [`Captured::assert_schema_ok`].
//!
//! ```
//! use eileen_trace::{names, test_support};
//!
//! let ((), captured) = test_support::capture(|| {
//!     let turn = tracing::info_span!(names::TURN, turn_id = "turn_1");
//!     let _entered = turn.enter();
//!     tracing::info_span!(names::ROUTER_DECIDE, route = "fast_path").in_scope(|| {});
//! });
//! captured.assert_schema_ok();
//! assert_eq!(captured.spans_named(names::ROUTER_DECIDE).len(), 1);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use eileen_stdx::clock::{self, Clock as _};
use eileen_stdx::soft_assert::{self, Mode};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;

use crate::record::{EventRecord, JsonVisitor, SpanRecord, event_name, remote_parent, sorted};
use crate::schema;

/// What a capture saw.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Captured {
    /// Every span, in the order they opened.
    pub spans: Vec<SpanRecord>,
    /// Every event, in order.
    pub events: Vec<EventRecord>,
}

impl Captured {
    /// The spans named `name`.
    #[must_use]
    pub fn spans_named(&self, name: &str) -> Vec<&SpanRecord> {
        self.spans.iter().filter(|s| s.name == name).collect()
    }

    /// The events named `name`.
    #[must_use]
    pub fn events_named(&self, name: &str) -> Vec<&EventRecord> {
        self.events.iter().filter(|e| e.name == name).collect()
    }

    /// Fails the test with every violation of the span-parent table.
    #[track_caller]
    pub fn assert_schema_ok(&self) {
        assert_schema_ok(&self.spans);
    }
}

/// Fails the test with every violation of the span-parent table in `spans`.
#[track_caller]
pub fn assert_schema_ok(spans: &[SpanRecord]) {
    let violations = schema::check(spans);
    let listed: Vec<String> = violations.iter().map(ToString::to_string).collect();
    assert!(violations.is_empty(), "span schema violations:\n{}", listed.join("\n"));
}

/// The layer behind [`capture`], for tests that build their own subscriber.
#[derive(Debug, Clone, Default)]
pub struct CaptureLayer {
    seen: Arc<Mutex<Captured>>,
    next_id: Arc<AtomicU64>,
}

impl CaptureLayer {
    /// What it has seen so far.
    #[must_use]
    pub fn captured(&self) -> Captured {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn with<T>(&self, f: impl FnOnce(&mut Captured) -> T) -> T {
        f(&mut self.seen.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut visitor = JsonVisitor::default();
        attrs.record(&mut visitor);
        let uid = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let parent = span.parent().and_then(|p| p.extensions().get::<CaptureId>().map(|c| c.0));
        span.extensions_mut().insert(CaptureId(uid));
        let record = SpanRecord {
            id: uid,
            span_id: id.into_u64(),
            parent,
            remote_parent: remote_parent(&visitor.0),
            name: span.name().to_owned(),
            process: "test".to_owned(),
            start_us: clock::system().wall().0,
            end_us: None,
            fields: sorted(&visitor.0),
        };
        self.with(|c| c.spans.push(record));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(id).and_then(|s| s.extensions().get::<CaptureId>().map(|c| c.0))
        else {
            return;
        };
        let mut visitor = JsonVisitor::default();
        values.record(&mut visitor);
        self.with(|c| {
            if let Some(span) = c.spans.iter_mut().find(|s| s.id == uid) {
                span.fields.extend(visitor.0);
            }
        });
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let span =
            ctx.event_span(event).and_then(|s| s.extensions().get::<CaptureId>().map(|c| c.0));
        let meta = event.metadata();
        let record = EventRecord {
            name: event_name(meta.name()).into_owned(),
            level: meta.level().as_str().to_owned(),
            target: meta.target().to_owned(),
            span,
            at_us: clock::system().wall().0,
            fields: sorted(&visitor.0),
        };
        self.with(|c| c.events.push(record));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(uid) = ctx.span(&id).and_then(|s| s.extensions().get::<CaptureId>().map(|c| c.0))
        else {
            return;
        };
        let end = clock::system().wall().0;
        self.with(|c| {
            if let Some(span) = c.spans.iter_mut().find(|s| s.id == uid) {
                span.end_us = Some(end);
            }
        });
    }
}

/// The capture layer's record id for a span.
struct CaptureId(u64);

/// Runs `f` with a capturing subscriber as this thread's default and soft assertions in
/// `Panic` mode, and returns what it captured.
pub fn capture<T>(f: impl FnOnce() -> T) -> (T, Captured) {
    let layer = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(layer.clone());
    let out =
        tracing::subscriber::with_default(subscriber, || soft_assert::with_mode(Mode::Panic, f));
    (out, layer.captured())
}

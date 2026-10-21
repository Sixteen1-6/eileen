//! The slow-span and stage-stats layer: every span's duration on close, a `span.slow` event
//! when it passes its name's threshold (with its children summarised), and per-name latency
//! histograms for `eileen debug stats` and the HUD's developer panel.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use eileen_stdx::clock::{self, Clock as _};
use tracing::span::{Attributes, Id};
use tracing::{Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::names;

/// The histogram's upper bounds, milliseconds; a last bucket holds everything slower.
pub const BUCKETS_MS: [u64; 8] = [1, 5, 10, 50, 100, 500, 1_000, 5_000];

/// One span name's durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StageStats {
    /// Spans closed.
    pub count: u64,
    /// Their total duration, microseconds.
    pub total_us: u64,
    /// The longest, microseconds.
    pub max_us: u64,
    /// Counts per bucket of [`BUCKETS_MS`], then the slower rest.
    pub buckets: [u64; 9],
}

/// Durations per span name.
#[derive(Debug, Default)]
pub struct Stats {
    stages: Mutex<BTreeMap<&'static str, StageStats>>,
}

impl Stats {
    fn record(&self, name: &'static str, dur_us: u64) {
        let bucket =
            BUCKETS_MS.iter().position(|ms| dur_us <= ms * 1_000).unwrap_or(BUCKETS_MS.len());
        let mut stages = self.stages.lock().unwrap_or_else(PoisonError::into_inner);
        let stage = stages.entry(name).or_default();
        stage.count += 1;
        stage.total_us = stage.total_us.saturating_add(dur_us);
        stage.max_us = stage.max_us.max(dur_us);
        if let Some(slot) = stage.buckets.get_mut(bucket) {
            *slot += 1;
        }
        drop(stages);
    }

    /// A copy of every name's figures.
    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<&'static str, StageStats> {
        self.stages.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// What the layer keeps per open span.
struct Timing {
    start_ns: u64,
    children: u32,
    slowest_child: Option<(&'static str, u64)>,
}

/// The layer.
#[derive(Debug, Clone)]
pub struct SlowLayer {
    thresholds_ms: BTreeMap<String, u64>,
    stats: Arc<Stats>,
}

impl SlowLayer {
    /// A layer with per-name thresholds (milliseconds) feeding `stats`.
    #[must_use]
    pub const fn new(thresholds_ms: BTreeMap<String, u64>, stats: Arc<Stats>) -> Self {
        Self { thresholds_ms, stats }
    }

    /// The default thresholds (observability report section 8.2).
    #[must_use]
    pub fn default_thresholds() -> BTreeMap<String, u64> {
        [
            (names::BRAIN_TURN, 3_000),
            (names::DISPATCH_CALL, 2_000),
            (names::SPEAK, 1_500),
            (names::STT_STREAM, 800),
        ]
        .into_iter()
        .map(|(name, ms)| (name.to_owned(), ms))
        .collect()
    }
}

impl<S> Layer<S> for SlowLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, _: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Timing {
                start_ns: clock::system().mono().0,
                children: 0,
                slowest_child: None,
            });
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let now = clock::system().mono().0;
        let Some((dur_us, children, slowest)) = span
            .extensions()
            .get::<Timing>()
            .map(|t| (now.saturating_sub(t.start_ns) / 1_000, t.children, t.slowest_child))
        else {
            return;
        };
        let name = span.name();
        self.stats.record(name, dur_us);
        if let Some(parent) = span.parent()
            && let Some(timing) = parent.extensions_mut().get_mut::<Timing>()
        {
            timing.children = timing.children.saturating_add(1);
            if timing.slowest_child.is_none_or(|(_, us)| dur_us > us) {
                timing.slowest_child = Some((name, dur_us));
            }
        }
        let Some(&threshold_ms) = self.thresholds_ms.get(name) else { return };
        let took_ms = dur_us / 1_000;
        if took_ms < threshold_ms {
            return;
        }
        let (slowest_child, slowest_child_ms) = slowest.map_or(("", 0), |(n, us)| (n, us / 1_000));
        tracing::event!(
            name: names::SPAN_SLOW,
            target: "eileen_trace::slow",
            parent: Option::<Id>::None,
            Level::INFO,
            span = name,
            dur_ms = took_ms,
            threshold_ms,
            children,
            slowest_child,
            slowest_child_ms,
        );
    }
}

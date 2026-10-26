//! The invariants of BUILD.md section 4.7 that the skeleton checks after every step, each a
//! named function over what the run produced so far. Their numbers are the section's, which
//! regressions and acceptance lines cite. WP1 adds the other thirteen.
//!
//! - 6, [`bus_events_are_attributed`]: every bus event carries a turn or job id where it
//!   belongs, nothing is emitted for a job after its terminal event, order is kept, a slow
//!   subscriber gets an explicit `Lagged` and never silent loss, and no unaddressed speech
//!   becomes a bus event.
//! - 10, [`utterances_do_not_overlap`]: two utterances never overlap and the speech queue stays
//!   bounded.
//! - 13, [`spans_follow_the_table`]: the span-parent table holds for every span the run
//!   produced, and every log line inside a turn carries its turn id.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eileen_contracts::bus::{BusEvent, TurnRoute};
use eileen_contracts::ids::{Envelope, SpeakId, TurnId};
use eileen_trace::names;
use eileen_trace::{EventRecord, SpanRecord};
use serde_json::Value;

/// The invariants the skeleton checks, by number and name.
pub const CHECKED: &[(u8, &str)] = &[
    (6, "bus_events_are_attributed"),
    (10, "utterances_do_not_overlap"),
    (13, "spans_follow_the_table"),
];

/// A broken invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Its number in section 4.7.
    pub number: u8,
    /// Its function's name.
    pub name: &'static str,
    /// What broke it.
    pub detail: String,
}

impl Violation {
    fn new(number: u8, name: &'static str, detail: impl Into<String>) -> Self {
        Self { number, name, detail: detail.into() }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invariant {} ({}): {}", self.number, self.name, self.detail)
    }
}

impl std::error::Error for Violation {}

/// What a slow bus subscriber received, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The event with this envelope `seq`.
    Event(u64),
    /// The bus's notice that this many events were dropped before the next one delivered.
    Lagged(u64),
}

/// Invariant 6 over the events published so far and what the slow subscriber received.
pub fn bus_events_are_attributed(
    published: &[Envelope<BusEvent>],
    delivered: &[Delivery],
) -> Result<(), Violation> {
    const N: u8 = 6;
    const NAME: &str = "bus_events_are_attributed";
    let fail = |detail: String| Err(Violation::new(N, NAME, detail));
    let mut last_seq: Option<u64> = None;
    let mut ended_jobs = BTreeSet::new();
    let mut ignored_turns: BTreeSet<TurnId> = BTreeSet::new();
    let mut heard_turns: BTreeSet<TurnId> = BTreeSet::new();
    for event in published {
        if last_seq.is_some_and(|last| event.seq <= last) {
            return fail(format!("event seq {} published after seq {last_seq:?}", event.seq));
        }
        last_seq = Some(event.seq);
        let tag = tag_of(&event.body);
        if is_turn_event(&event.body) && event.ctx.turn.is_none() {
            return fail(format!("`{tag}` at seq {} carries no turn id", event.seq));
        }
        if let Some(job) = job_of(&event.body) {
            let in_ctx = event.ctx.job.map(|id| id.to_string());
            if in_ctx.as_deref() != Some(job.as_str()) {
                return fail(format!(
                    "`{tag}` for {job} at seq {} carries job {in_ctx:?}",
                    event.seq
                ));
            }
            if ended_jobs.contains(&job) {
                return fail(format!(
                    "`{tag}` at seq {} for {job} after its terminal event",
                    event.seq
                ));
            }
            if is_terminal(&event.body) {
                ended_jobs.insert(job);
            }
        }
        if let Some(turn) = event.ctx.turn {
            if matches!(&event.body, BusEvent::TurnRouted { route: TurnRoute::Ignored, .. }) {
                ignored_turns.insert(turn);
            }
            if matches!(&event.body, BusEvent::Heard { .. }) {
                heard_turns.insert(turn);
            }
            if ignored_turns.contains(&turn) && heard_turns.contains(&turn) {
                return fail(format!("{turn} was ignored and still heard on the bus"));
            }
        }
    }
    subscriber_lost_nothing(published, delivered).map_err(|detail| Violation::new(N, NAME, detail))
}

/// Every event the subscriber did not receive is counted by a `Lagged` notice before the next
/// one it did receive, exactly.
fn subscriber_lost_nothing(
    published: &[Envelope<BusEvent>],
    delivered: &[Delivery],
) -> Result<(), String> {
    let order: BTreeMap<u64, usize> =
        published.iter().enumerate().map(|(index, event)| (event.seq, index)).collect();
    let mut next = 0_usize;
    let mut owed = 0_u64;
    for item in delivered {
        match *item {
            Delivery::Lagged(missed) => owed = owed.saturating_add(missed),
            Delivery::Event(seq) => {
                let Some(&index) = order.get(&seq) else {
                    return Err(format!(
                        "the subscriber received seq {seq}, which was never published"
                    ));
                };
                if index < next {
                    return Err(format!("the subscriber received seq {seq} out of order"));
                }
                let skipped = u64::try_from(index - next).unwrap_or(u64::MAX);
                if skipped != owed {
                    return Err(format!(
                        "the subscriber missed events before seq {seq}: {skipped}, and was told {owed}"
                    ));
                }
                owed = 0;
                next = index + 1;
            }
        }
    }
    let tail = u64::try_from(published.len() - next.min(published.len())).unwrap_or(u64::MAX);
    if owed > tail {
        return Err(format!("the subscriber was told of {owed} missed events, but {tail} remain"));
    }
    Ok(())
}

/// Invariant 10 over the events published so far and the speech queue's length now.
pub fn utterances_do_not_overlap(
    published: &[Envelope<BusEvent>],
    speech_queue: usize,
    speech_queue_max: usize,
) -> Result<(), Violation> {
    const N: u8 = 10;
    const NAME: &str = "utterances_do_not_overlap";
    let mut speaking: Option<SpeakId> = None;
    for event in published {
        if let BusEvent::Speaking { speak, .. } = &event.body {
            if let Some(other) = speaking.filter(|other| other != speak) {
                return Err(Violation::new(
                    N,
                    NAME,
                    format!("{speak} started at seq {} while {other} was speaking", event.seq),
                ));
            }
            speaking = Some(*speak);
        }
        if let BusEvent::SpeakingStopped { speak, .. } = &event.body
            && speaking == Some(*speak)
        {
            speaking = None;
        }
    }
    if speech_queue > speech_queue_max {
        return Err(Violation::new(
            N,
            NAME,
            format!("{speech_queue} utterances queued, over the bound of {speech_queue_max}"),
        ));
    }
    Ok(())
}

/// Invariant 13 over the spans and log lines the run produced.
pub fn spans_follow_the_table(
    spans: &[SpanRecord],
    events: &[EventRecord],
) -> Result<(), Violation> {
    const N: u8 = 13;
    const NAME: &str = "spans_follow_the_table";
    let broken = eileen_trace::schema::check(spans);
    if let Some(first) = broken.first() {
        return Err(Violation::new(
            N,
            NAME,
            format!("{first} ({} span-parent violations in all)", broken.len()),
        ));
    }
    // The records come from one process's ring, so a record id names one span.
    let by_id: BTreeMap<u64, &SpanRecord> = spans.iter().map(|span| (span.id, span)).collect();
    for event in events {
        let mut at = event.span;
        let mut turn_span = None;
        while let Some(id) = at {
            let Some(span) = by_id.get(&id) else { break };
            if span.name == names::TURN {
                turn_span = Some(*span);
                break;
            }
            at = span.parent;
        }
        let Some(turn_span) = turn_span else { continue };
        let Some(turn_id) = turn_span.fields.get("turn_id").and_then(Value::as_str) else {
            return Err(Violation::new(
                N,
                NAME,
                "a `turn` span with a log line in it has no turn id",
            ));
        };
        if turn_id.is_empty() {
            return Err(Violation::new(N, NAME, "a `turn` span has an empty turn id"));
        }
        if let Some(own) = event.fields.get("turn_id").and_then(Value::as_str)
            && own != turn_id
        {
            return Err(Violation::new(
                N,
                NAME,
                format!("a log line inside turn {turn_id} names turn {own}"),
            ));
        }
    }
    Ok(())
}

/// A bus event's variant name, as it is serialized.
fn tag_of(event: &BusEvent) -> String {
    match serde_json::to_value(event) {
        Ok(Value::Object(map)) => map.keys().next().cloned().unwrap_or_default(),
        Ok(Value::String(tag)) => tag,
        Ok(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) | Err(_) => {
            String::new()
        }
    }
}

/// The job a bus event is about: the `job` field of its body, when it has one.
fn job_of(event: &BusEvent) -> Option<String> {
    let Ok(Value::Object(map)) = serde_json::to_value(event) else { return None };
    map.values().next()?.get("job")?.as_str().map(str::to_owned)
}

/// Events that belong to a turn and must carry its id.
const fn is_turn_event(event: &BusEvent) -> bool {
    matches!(
        event,
        BusEvent::TurnStarted { .. } | BusEvent::Heard { .. } | BusEvent::TurnRouted { .. }
    )
}

/// A job's terminal events.
const fn is_terminal(event: &BusEvent) -> bool {
    matches!(
        event,
        BusEvent::JobCompleted { .. }
            | BusEvent::JobFailed { .. }
            | BusEvent::JobCancelled { .. }
            | BusEvent::JobInterrupted { .. }
    )
}

#[cfg(test)]
mod tests;

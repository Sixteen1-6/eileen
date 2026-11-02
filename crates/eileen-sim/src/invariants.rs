//! The invariants of BUILD.md section 4.7, each a named function over what the run produced so
//! far. Their numbers are the section's, which regressions and acceptance lines cite. The world
//! checks 6, 8, 10 and 13 after every step ([`CHECKED`]); `turn::exhaustive` checks 2, 3, 9 and
//! 10 after every input it enumerates ([`EXHAUSTIVE`]). WP1 adds the others.
//!
//! - 2, [`barge_in_pauses_in_time`]: after a speech window while Eileen speaks, playback is
//!   ducked by the next callback and paused by the callback after the third consecutive
//!   window, onset to silence stays within [`ONSET_TO_SILENCE_MS`], and a pause is resumed or
//!   stopped within [`PAUSE_RESOLVES_MS`].
//! - 3, [`heard_prefix_matches_playback`]: what the brain is told the user heard equals the
//!   text mapped from the samples handed to the device. After a stop with no pause first the
//!   core can still be a character short (#151).
//! - 6, [`bus_events_are_attributed`]: every bus event carries a turn or job id where it
//!   belongs, nothing is emitted for a job after its terminal event, order is kept, a slow
//!   subscriber gets an explicit `Lagged` and never silent loss, and no unaddressed speech
//!   becomes a bus event.
//! - 8, [`children_restart_with_bounded_backoff`]: a hung child goes down within
//!   [`HUNG_KILL_MS`] of hanging, a load that never finishes within [`LOAD_KILL_MS`] of its
//!   start, and a crashed or hung child is restarted only after it went down, between
//!   [`BACKOFF_MIN_MS`] and [`BACKOFF_MAX_MS`] later, and at most [`RESTARTS_PER_WINDOW`] times
//!   in any [`RESTART_WINDOW_MS`]. That killed children leave no descendants waits for the
//!   simulated Job Objects.
//! - 9, [`nothing_sounds_while_muted`]: capture stops within one 10 ms frame of a mute the user
//!   asks for, and nothing is spoken, played or handed to the device while muted or while
//!   another app uses the microphone. The core does not act on the microphone part yet (#152).
//!   The call, cancel and input-lease parts wait for those features.
//! - 10, [`utterances_do_not_overlap`]: two utterances never overlap and the speech queue stays
//!   bounded.
//! - 13, [`spans_follow_the_table`]: the span-parent table holds for every span the run
//!   produced, and every log line inside a turn carries its turn id.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use eileen_contracts::bus::{BusEvent, TurnRoute};
use eileen_contracts::connectors::SidecarId;
use eileen_contracts::ids::{Envelope, TurnId};
use eileen_trace::names;
use eileen_trace::{EventRecord, SpanRecord};
use serde_json::Value;

mod audio;

pub use audio::{
    AudioEvent, AudioRecord, BargeInWatch, CAPTURE_STOPS_MS, HEARD_MARKER, Handed, HeardWatch,
    MuteWatch, ONSET_TO_SILENCE_MS, OverlapWatch, PAUSE_BY_WINDOW, PAUSE_RESOLVES_MS, WINDOW_MS,
    barge_in_pauses_in_time, heard_prefix_matches_playback, nothing_sounds_while_muted,
};

/// The invariants the world checks after every step, by number and name.
pub const CHECKED: &[(u8, &str)] = &[
    (6, "bus_events_are_attributed"),
    (8, "children_restart_with_bounded_backoff"),
    (10, "utterances_do_not_overlap"),
    (13, "spans_follow_the_table"),
];

/// The invariants `turn::exhaustive` checks after every input, by number and name (section
/// 4.7, "Exhaustive tests").
pub const EXHAUSTIVE: &[(u8, &str)] = &[
    (2, "barge_in_pauses_in_time"),
    (3, "heard_prefix_matches_playback"),
    (9, "nothing_sounds_while_muted"),
    (10, "utterances_do_not_overlap"),
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

/// The shortest wait from a child going down to its restart, ms: the first backoff step.
pub const BACKOFF_MIN_MS: u64 = 1_000;
/// The longest wait from a child going down to its restart, ms: the last backoff step, which
/// the speech process's audio ring covers (section 6.6).
pub const BACKOFF_MAX_MS: u64 = 60_000;
/// The window restarts are counted in, ms.
pub const RESTART_WINDOW_MS: u64 = 60_000;
/// The most restarts of one child in any window. Steps of 1, 2, 4, 8 and 16 s put five
/// restarts of a child that crashes at once in its first minute, and the steps start again
/// from 1 s only after a minute up.
pub const RESTARTS_PER_WINDOW: usize = 6;
/// The longest a hung child may stay up after it hung, ms: five missed one-second heartbeats,
/// counted from the last one, which came at or before the hang.
pub const HUNG_KILL_MS: u64 = 5_000;
/// The longest a load that never finishes may last from its start or restart, ms.
pub const LOAD_KILL_MS: u64 = 120_000;

/// What happened to a supervised child, as the world saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildEvent {
    /// The core started it.
    Started,
    /// It went down: the core had it hang-dumped and killed, or it exited.
    Down,
    /// It stopped answering while up (a fault the world injected).
    Hung,
    /// Its load will never finish (a fault the world injected).
    Stuck,
    /// The core restarted it.
    Restarted,
}

/// One event in a child's life, at a virtual time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildRecord {
    /// When, ms of virtual time.
    pub at_ms: u64,
    /// Which child.
    pub child: SidecarId,
    /// What happened.
    pub event: ChildEvent,
}

/// Invariant 8 over the children's lives so far, at `now_ms`.
pub fn children_restart_with_bounded_backoff(
    records: &[ChildRecord],
    now_ms: u64,
) -> Result<(), Violation> {
    const N: u8 = 8;
    const NAME: &str = "children_restart_with_bounded_backoff";
    let fail = |detail: String| Err(Violation::new(N, NAME, detail));
    let mut children: BTreeMap<SidecarId, Life> = BTreeMap::new();
    for record in records {
        let Life { down, restarts, loaded, kill_by } = children.entry(record.child).or_default();
        match record.event {
            ChildEvent::Started => {
                *down = None;
                *loaded = record.at_ms;
            }
            ChildEvent::Hung if kill_by.is_none() => {
                *kill_by = Some(record.at_ms.saturating_add(HUNG_KILL_MS));
            }
            ChildEvent::Stuck if kill_by.is_none() => {
                *kill_by = Some(loaded.saturating_add(LOAD_KILL_MS));
            }
            ChildEvent::Hung | ChildEvent::Stuck => {}
            ChildEvent::Down => {
                if let Some(by) = kill_by.take()
                    && record.at_ms > by
                {
                    return fail(format!(
                        "{:?} went down at {} ms, after its kill was due at {by} ms",
                        record.child, record.at_ms
                    ));
                }
                // The exit that follows a kill is the same going down.
                if down.is_none() {
                    *down = Some(record.at_ms);
                }
            }
            ChildEvent::Restarted => {
                let Some(since) = down.take() else {
                    return fail(format!(
                        "{:?} restarted at {} ms while it was up",
                        record.child, record.at_ms
                    ));
                };
                *loaded = record.at_ms;
                let wait = record.at_ms.saturating_sub(since);
                if !(BACKOFF_MIN_MS..=BACKOFF_MAX_MS).contains(&wait) {
                    return fail(format!(
                        "{:?} restarted {wait} ms after it went down at {since} ms",
                        record.child
                    ));
                }
                restarts.push_back(record.at_ms);
                while restarts
                    .front()
                    .is_some_and(|first| first.saturating_add(RESTART_WINDOW_MS) <= record.at_ms)
                {
                    restarts.pop_front();
                }
                if restarts.len() > RESTARTS_PER_WINDOW {
                    return fail(format!(
                        "{:?} restarted {} times in the {RESTART_WINDOW_MS} ms to {} ms",
                        record.child,
                        restarts.len(),
                        record.at_ms
                    ));
                }
            }
        }
    }
    for (child, life) in &children {
        if let Some(since) = life.down
            && now_ms.saturating_sub(since) > BACKOFF_MAX_MS
        {
            return fail(format!(
                "{child:?} went down at {since} ms and was not restarted by {now_ms} ms"
            ));
        }
        if let Some(by) = life.kill_by
            && now_ms > by
        {
            return fail(format!("{child:?} hung and was not killed by {by} ms"));
        }
    }
    Ok(())
}

/// One child's life as invariant 8 follows it.
#[derive(Debug, Default)]
struct Life {
    /// When it went down, while it is down.
    down: Option<u64>,
    /// Its restarts inside the window.
    restarts: VecDeque<u64>,
    /// When its latest start or restart went out.
    loaded: u64,
    /// When it must be down by, once it hung or its load stuck.
    kill_by: Option<u64>,
}

/// Invariant 10 over the events published so far and the speech queue's length now.
pub fn utterances_do_not_overlap(
    published: &[Envelope<BusEvent>],
    speech_queue: usize,
    speech_queue_max: usize,
) -> Result<(), Violation> {
    const N: u8 = 10;
    const NAME: &str = "utterances_do_not_overlap";
    let mut watch = OverlapWatch::default();
    for event in published {
        if let Some((speak, other)) = watch.follow(&event.body) {
            return Err(Violation::new(
                N,
                NAME,
                format!("{speak} started at seq {} while {other} was speaking", event.seq),
            ));
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

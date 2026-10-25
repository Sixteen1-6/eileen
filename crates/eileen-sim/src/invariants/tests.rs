//! Each invariant on a hand-built trace that breaks it, and on one that keeps it: against the
//! stub core every run keeps them trivially, so these show they can fail.

use std::collections::BTreeMap;

use eileen_contracts::bus::{BusEvent, SpeakCause, TurnRoute};
use eileen_contracts::ids::{
    Envelope, JobId, MonoTime, Personal, SpeakId, TraceCtx, TurnId, Untrusted, WallTime,
};
use eileen_contracts::jobs::{TurnAuthority, TurnSource};
use eileen_trace::names;
use eileen_trace::{EventRecord, SpanRecord};
use serde_json::{Value, json};

use super::{
    Delivery, bus_events_are_attributed, spans_follow_the_table, utterances_do_not_overlap,
};

fn turn(n: u64) -> TurnId {
    TurnId::from_parts(n, u128::from(n))
}

fn job(n: u64) -> JobId {
    JobId::from_parts(n, u128::from(n))
}

fn speak(n: u64) -> SpeakId {
    SpeakId::from_parts(n, u128::from(n))
}

fn envelope(seq: u64, ctx: TraceCtx, body: BusEvent) -> Envelope<BusEvent> {
    Envelope { seq, at: WallTime(0), mono: MonoTime(seq), ctx, body }
}

fn in_turn(t: TurnId) -> TraceCtx {
    TraceCtx { turn: Some(t), ..TraceCtx::default() }
}

fn in_job(j: JobId) -> TraceCtx {
    TraceCtx { job: Some(j), ..TraceCtx::default() }
}

fn started(seq: u64, t: TurnId) -> Envelope<BusEvent> {
    envelope(
        seq,
        in_turn(t),
        BusEvent::TurnStarted { source: TurnSource::Voice, authority: TurnAuthority::Addressed },
    )
}

fn completed(seq: u64, j: JobId) -> Envelope<BusEvent> {
    envelope(
        seq,
        in_job(j),
        BusEvent::JobCompleted {
            job: j,
            summary: Untrusted::new(String::new()),
            artifacts: vec![],
        },
    )
}

#[test]
fn invariant_6_holds_for_attributed_ordered_events() {
    let events = vec![started(1, turn(1)), completed(2, job(1))];
    let delivered = [Delivery::Event(1), Delivery::Event(2)];
    assert_eq!(bus_events_are_attributed(&events, &delivered), Ok(()), "a clean trace");
}

#[test]
fn invariant_6_fails_on_a_turn_event_without_its_turn() {
    let mut event = started(1, turn(1));
    event.ctx = TraceCtx::default();
    let error = bus_events_are_attributed(&[event], &[]).unwrap_err();
    assert_eq!(error.number, 6, "{error}");
    assert!(error.detail.contains("turn_started"), "{error}");
}

#[test]
fn invariant_6_fails_on_a_job_event_with_another_jobs_context() {
    let mut event = completed(1, job(1));
    event.ctx = in_job(job(2));
    let error = bus_events_are_attributed(&[event], &[]).unwrap_err();
    assert!(error.detail.contains("carries job"), "{error}");
}

#[test]
fn invariant_6_fails_on_an_event_after_a_jobs_terminal_event() {
    let events = vec![completed(1, job(1)), completed(2, job(1))];
    let error = bus_events_are_attributed(&events, &[]).unwrap_err();
    assert!(error.detail.contains("after its terminal event"), "{error}");
}

#[test]
fn invariant_6_fails_when_order_goes_back() {
    let events = vec![started(2, turn(1)), started(1, turn(2))];
    let error = bus_events_are_attributed(&events, &[]).unwrap_err();
    assert!(error.detail.contains("published after"), "{error}");
}

#[test]
fn invariant_6_fails_on_silent_loss_and_passes_an_explicit_lagged() {
    let events = vec![started(1, turn(1)), started(2, turn(2)), started(3, turn(3))];
    let silent = [Delivery::Event(1), Delivery::Event(3)];
    let error = bus_events_are_attributed(&events, &silent).unwrap_err();
    assert!(error.detail.contains("missed 1 events before seq 3 and was told 0"), "{error}");
    let told = [Delivery::Event(1), Delivery::Lagged(1), Delivery::Event(3)];
    assert_eq!(bus_events_are_attributed(&events, &told), Ok(()), "a Lagged notice counts");
}

#[test]
fn invariant_6_fails_when_ignored_speech_is_heard() {
    let t = turn(1);
    let events = vec![
        envelope(
            1,
            in_turn(t),
            BusEvent::TurnRouted { route: TurnRoute::Ignored, reason: "not addressed".to_owned() },
        ),
        envelope(2, in_turn(t), BusEvent::Heard { text: Personal::new("stop".to_owned()) }),
    ];
    let error = bus_events_are_attributed(&events, &[]).unwrap_err();
    assert!(error.detail.contains("ignored and still heard"), "{error}");
}

fn speaking(seq: u64, s: SpeakId) -> Envelope<BusEvent> {
    envelope(
        seq,
        TraceCtx::default(),
        BusEvent::Speaking { speak: s, cause: SpeakCause::Reply, text: "hello".to_owned() },
    )
}

fn stopped(seq: u64, s: SpeakId) -> Envelope<BusEvent> {
    envelope(
        seq,
        TraceCtx::default(),
        BusEvent::SpeakingStopped { speak: s, heard_chars: 5, interrupted: false, resumed: false },
    )
}

#[test]
fn invariant_10_holds_for_utterances_one_after_another() {
    let events = vec![speaking(1, speak(1)), stopped(2, speak(1)), speaking(3, speak(2))];
    assert_eq!(utterances_do_not_overlap(&events, 1, 8), Ok(()), "no overlap");
}

#[test]
fn invariant_10_fails_on_overlapping_utterances() {
    let events = vec![speaking(1, speak(1)), speaking(2, speak(2))];
    let error = utterances_do_not_overlap(&events, 0, 8).unwrap_err();
    assert_eq!(error.number, 10, "{error}");
    assert!(error.detail.contains("was speaking"), "{error}");
}

#[test]
fn invariant_10_fails_on_an_unbounded_queue() {
    let error = utterances_do_not_overlap(&[], 9, 8).unwrap_err();
    assert!(error.detail.contains("over the bound of 8"), "{error}");
}

fn span(id: u64, parent: Option<u64>, name: &str, fields: &[(&str, Value)]) -> SpanRecord {
    SpanRecord {
        id,
        span_id: id,
        parent,
        remote_parent: None,
        name: name.to_owned(),
        process: "eileen".to_owned(),
        start_us: i64::try_from(id).unwrap(),
        end_us: None,
        fields: fields.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect(),
    }
}

fn line(span: u64, fields: &[(&str, Value)]) -> EventRecord {
    EventRecord {
        name: "event core.rs:1".to_owned(),
        level: "INFO".to_owned(),
        target: "eileen_core".to_owned(),
        span: Some(span),
        at_us: 10,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn invariant_13_holds_for_a_turn_with_its_children() {
    let turn_id = json!("turn_01JD0000000000000000000000");
    let spans = vec![
        span(1, None, names::TURN, &[("turn_id", turn_id.clone())]),
        span(2, Some(1), names::ROUTER_DECIDE, &[("route", json!("fast_path"))]),
    ];
    let lines = vec![line(2, &[("turn_id", turn_id)])];
    assert_eq!(spans_follow_the_table(&spans, &lines), Ok(()), "a clean trace");
}

#[test]
fn invariant_13_fails_on_a_span_the_table_does_not_declare() {
    let spans = vec![span(1, None, "made.up.span", &[])];
    let error = spans_follow_the_table(&spans, &[]).unwrap_err();
    assert_eq!(error.number, 13, "{error}");
    assert!(error.detail.contains("made.up.span"), "{error}");
}

#[test]
fn invariant_13_fails_on_a_child_under_the_wrong_parent() {
    let spans = vec![span(1, None, names::ROUTER_DECIDE, &[])];
    let error = spans_follow_the_table(&spans, &[]).unwrap_err();
    assert!(error.detail.contains(names::ROUTER_DECIDE), "{error}");
}

#[test]
fn invariant_13_fails_on_a_log_line_naming_another_turn() {
    let spans = vec![span(1, None, names::TURN, &[("turn_id", json!("turn_A"))])];
    let lines = vec![line(1, &[("turn_id", json!("turn_B"))])];
    let error = spans_follow_the_table(&spans, &lines).unwrap_err();
    assert!(error.detail.contains("names turn turn_B"), "{error}");
}

#[test]
fn invariant_13_fails_on_a_turn_without_its_id() {
    let spans = vec![span(1, None, names::TURN, &[])];
    let lines = vec![line(1, &[])];
    let error = spans_follow_the_table(&spans, &lines).unwrap_err();
    assert!(error.detail.contains("no turn id"), "{error}");
}

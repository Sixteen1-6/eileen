//! The Chrome JSON trace converter: span records from any number of processes onto one
//! Perfetto timeline (observability report section 3.3).
//!
//! One `pid` per Eileen process, all aligned by wall clock. Perfetto needs the duration
//! events of a track to nest, and async spans hop threads, so each track root (a span with no
//! parent in its process, and every `turn`, `job` and `speak`) gets its own track with its
//! descendants on it. A span whose `traceparent` names a span in another process gets a flow
//! arrow from it, found as [`crate::schema`] finds a remote parent: by span id among the spans
//! not shown to be in another trace, so the arrow stays when a ring dropped the parent's
//! ancestors.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::names;
use crate::record::SpanRecord;
use crate::schema::Parents;

/// Spans that start a track of their own even when they have a parent.
pub const TRACK_ROOTS: [&str; 3] = [names::TURN, names::JOB, names::SPEAK];

/// The trace as Perfetto and `chrome://tracing` read it: `{"traceEvents": [...]}`.
#[must_use]
pub fn to_chrome(spans: &[SpanRecord]) -> Value {
    let parents = Parents::new(spans);
    let mut order: Vec<&SpanRecord> = spans.iter().collect();
    order.sort_by_key(|s| (s.start_us, s.id));
    let latest = spans
        .iter()
        .filter_map(|s| s.end_us)
        .chain(spans.iter().map(|s| s.start_us))
        .max()
        .unwrap_or(0);

    let mut pids: BTreeMap<&str, u64> = BTreeMap::new();
    for span in &order {
        let next = u64::try_from(pids.len()).unwrap_or(u64::MAX) + 1;
        pids.entry(span.process.as_str()).or_insert(next);
    }
    let mut events: Vec<Value> = pids
        .iter()
        .map(|(process, pid)| json!({"ph": "M", "name": "process_name", "pid": pid, "args": {"name": process}}))
        .collect();

    let mut tracks: BTreeMap<(&str, u64), u64> = BTreeMap::new();
    let mut next_tid: BTreeMap<&str, u64> = BTreeMap::new();
    let mut flow = 0_u64;
    for span in &order {
        let process = span.process.as_str();
        let pid = pids.get(process).copied().unwrap_or(0);
        let inherited = span
            .parent
            .filter(|_| !TRACK_ROOTS.contains(&span.name.as_str()))
            .and_then(|parent| tracks.get(&(process, parent)).copied());
        let tid = inherited.unwrap_or_else(|| {
            let counter = next_tid.entry(process).or_insert(0);
            *counter += 1;
            events.push(json!({
                "ph": "M", "name": "thread_name", "pid": pid, "tid": *counter,
                "args": {"name": format!("{} {}", span.name, span.id)},
            }));
            *counter
        });
        tracks.insert((process, span.id), tid);
        let end = span.end_us.unwrap_or(latest);
        events.push(json!({
            "ph": "X", "cat": "eileen", "name": span.name, "pid": pid, "tid": tid,
            "ts": span.start_us, "dur": end.saturating_sub(span.start_us).max(0), "args": span.fields,
        }));
        if let Some(parent) = parents.remote(span) {
            flow += 1;
            let from_pid = pids.get(parent.process.as_str()).copied().unwrap_or(0);
            let from_track =
                tracks.get(&(parent.process.as_str(), parent.id)).copied().unwrap_or(0);
            events.push(json!({
                "ph": "s", "cat": "flow", "name": "remote", "id": flow,
                "pid": from_pid, "tid": from_track, "ts": span.start_us,
            }));
            events.push(json!({
                "ph": "f", "bp": "e", "cat": "flow", "name": "remote", "id": flow,
                "pid": pid, "tid": tid, "ts": span.start_us,
            }));
        }
    }
    json!({"traceEvents": events, "displayTimeUnit": "ms"})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use eileen_contracts::ids::{JobId, TraceCtx, TurnId};
    use serde_json::{Value, json};
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::to_chrome;
    use crate::names;
    use crate::record::{SpanRecord, remote_parent, sorted};
    use crate::redact::Redactor;
    use crate::ring::{Ring, RingLayer, parse_lines};
    use crate::traceparent;

    fn span(
        process: &str,
        id: u64,
        parent: Option<u64>,
        name: &str,
        start: i64,
        end: i64,
    ) -> SpanRecord {
        SpanRecord {
            id,
            span_id: id,
            parent,
            remote_parent: None,
            name: name.to_owned(),
            process: process.to_owned(),
            start_us: start,
            end_us: Some(end),
            fields: BTreeMap::new(),
        }
    }

    /// `span` with `fields`, its remote parent read from them the way the ring reads a line.
    fn with_fields(mut span: SpanRecord, fields: &Value) -> SpanRecord {
        let fields = fields.as_object().cloned().unwrap_or_default();
        span.remote_parent = remote_parent(&fields);
        span.fields = sorted(&fields);
        span
    }

    /// A turn's context, from which the core writes a `traceparent`.
    const fn in_turn(turn: TurnId) -> TraceCtx {
        TraceCtx { turn: Some(turn), job: None, call: None }
    }

    #[test]
    fn processes_get_pids_roots_get_tracks_and_remote_parents_get_flows() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let apps = with_fields(
            span("eileen-apps", 1, None, names::APPS_CALL, 20, 30),
            &json!({"traceparent": in_turn(turn).traceparent(4)}),
        );
        let spans = [
            with_fields(
                span("eileen", 1, None, names::TURN, 0, 100),
                &json!({"turn_id": turn.to_string()}),
            ),
            span("eileen", 2, Some(1), names::ROUTER_DECIDE, 1, 5),
            span("eileen", 3, Some(1), names::JOB, 6, 90),
            span("eileen", 4, Some(3), names::MCP_CALL, 10, 40),
            apps,
        ];
        let trace = to_chrome(&spans);
        let events = trace["traceEvents"].as_array().unwrap();
        let complete: Vec<&Value> = events.iter().filter(|e| e["ph"] == "X").collect();
        assert_eq!(complete.len(), 5, "one slice per span");
        let tid = |name: &str| complete.iter().find(|e| e["name"] == name).unwrap()["tid"].clone();
        assert_eq!(tid(names::ROUTER_DECIDE), tid(names::TURN), "children share the root's track");
        assert_ne!(tid(names::JOB), tid(names::TURN), "a job gets its own track");
        assert_eq!(tid(names::MCP_CALL), tid(names::JOB), "under the job");
        let pid = |name: &str| complete.iter().find(|e| e["name"] == name).unwrap()["pid"].clone();
        assert_ne!(pid(names::APPS_CALL), pid(names::TURN), "one pid per process");
        assert_eq!(
            events.iter().filter(|e| e["ph"] == "s" || e["ph"] == "f").count(),
            2,
            "one flow arrow"
        );
    }

    #[test]
    fn a_flow_starts_at_the_parent_in_the_childs_trace() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let other = TurnId::from_parts(1_761_000_000_000, 2);
        let spans = [
            // Another turn's speech segment holds the mcp.call's span id and starts first.
            with_fields(
                span("eileen-speech", 3, None, names::SPEECH_SEGMENT, 0, 100),
                &json!({
                    "turn_id": other.to_string(),
                    "traceparent": in_turn(other).traceparent(9),
                }),
            ),
            with_fields(
                span("eileen", 1, None, names::TURN, 1, 90),
                &json!({"turn_id": turn.to_string()}),
            ),
            span("eileen", 2, Some(1), names::BRAIN_TURN, 2, 80),
            span("eileen", 4, Some(2), names::DISPATCH_CALL, 5, 60),
            span("eileen", 3, Some(4), names::MCP_CALL, 10, 50),
            with_fields(
                span("eileen-desktop", 1, None, names::DESKTOP_CAPTURE, 20, 30),
                &json!({"traceparent": in_turn(turn).traceparent(3)}),
            ),
        ];
        let trace = to_chrome(&spans);
        let events = trace["traceEvents"].as_array().unwrap();
        let core = events
            .iter()
            .find(|e| e["name"] == "process_name" && e["args"]["name"] == "eileen")
            .unwrap();
        let call = events.iter().find(|e| e["ph"] == "X" && e["name"] == names::MCP_CALL).unwrap();
        let start = events.iter().find(|e| e["ph"] == "s").unwrap();
        assert_eq!(
            (&start["pid"], &start["tid"]),
            (&core["pid"], &call["tid"]),
            "the flow starts on the core's mcp.call, not on the speech segment"
        );
    }

    /// The `pid` that `trace` gives `process`.
    fn pid_of<'a>(trace: &'a Value, process: &str) -> &'a Value {
        let events = trace["traceEvents"].as_array().unwrap();
        let named = |e: &&Value| e["name"] == "process_name" && e["args"]["name"] == process;
        &events.iter().find(named).unwrap()["pid"]
    }

    #[test]
    fn a_flow_starts_at_a_parent_whose_trace_is_unknown() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        // The core's mcp.call, two records below a worker.tool_use whose worker.session's
        // record is missing, so the records cannot show its trace.
        let core = [
            span("eileen", 12, Some(11), names::WORKER_TOOL_USE, 0, 100),
            span("eileen", 13, Some(12), names::DISPATCH_CALL, 5, 90),
            span("eileen", 14, Some(13), names::MCP_CALL, 10, 80),
        ];
        let action = with_fields(
            span("eileen-desktop", 1, None, names::DESKTOP_ACTION, 20, 30),
            &json!({"traceparent": in_turn(turn).traceparent(14)}),
        );
        // An apps call of the turn that holds span id 14 too.
        let apps = with_fields(
            span("eileen-apps", 14, None, names::APPS_CALL, 15, 40),
            &json!({"traceparent": in_turn(turn).traceparent(9)}),
        );
        for (case, clash) in [("alone", None), ("beside an apps call", Some(apps))] {
            let spans: Vec<SpanRecord> =
                core.iter().cloned().chain(clash).chain([action.clone()]).collect();
            let trace = to_chrome(&spans);
            let events = trace["traceEvents"].as_array().unwrap();
            let flows: Vec<&Value> =
                events.iter().filter(|e| e["ph"] == "s" || e["ph"] == "f").collect();
            assert_eq!(flows.len(), 2, "{case}: one flow arrow");
            let call =
                events.iter().find(|e| e["ph"] == "X" && e["name"] == names::MCP_CALL).unwrap();
            let start = flows.iter().find(|e| e["ph"] == "s").unwrap();
            assert_eq!(
                (&start["pid"], &start["tid"]),
                (pid_of(&trace, "eileen"), &call["tid"]),
                "{case}: from the core's mcp.call"
            );
        }
    }

    #[test]
    fn a_flow_starts_at_a_parent_whose_ancestors_a_ring_dropped() {
        let job = JobId::from_parts(1_761_000_000_000, 2);
        let ring = |process| Arc::new(Ring::new(process, 4_096, Duration::from_secs(600)));
        let (core, desktop) = (ring("eileen"), ring("eileen-desktop"));
        let in_core = tracing_subscriber::registry().with(RingLayer::new(Arc::clone(&core)));
        let in_desktop = tracing_subscriber::registry().with(RingLayer::new(Arc::clone(&desktop)));
        tracing::subscriber::with_default(in_core, || {
            let job_span = tracing::info_span!(names::JOB, job_id = %job);
            let _job = job_span.enter();
            let session = tracing::info_span!(names::WORKER_SESSION, route = "claude");
            let _session = session.enter();
            for n in 0..100 {
                tracing::debug!(n, "filler");
            }
            let tool_use = tracing::info_span!(names::WORKER_TOOL_USE, tool = "capture");
            let _tool_use = tool_use.enter();
            let dispatch = tracing::info_span!(names::DISPATCH_CALL, tool = "capture");
            let _dispatch = dispatch.enter();
            let call = tracing::info_span!(names::MCP_CALL);
            let _call = call.enter();
            let ctx = TraceCtx { turn: None, job: Some(job), call: None };
            let parent = traceparent::current(&ctx).unwrap();
            tracing::subscriber::with_default(in_desktop, || {
                tracing::info_span!(names::DESKTOP_CAPTURE, traceparent = parent.as_str())
                    .in_scope(|| {});
            });
        });
        let redactor = Redactor::new();
        let lines = [core.dump(&redactor), desktop.dump(&redactor)].concat();
        let spans = parse_lines(String::from_utf8_lossy(&lines).lines()).spans;
        let kept = |process: &str| -> Vec<&str> {
            spans.iter().filter(|s| s.process == process).map(|s| s.name.as_str()).collect()
        };
        assert_eq!(
            kept("eileen"),
            [names::WORKER_TOOL_USE, names::DISPATCH_CALL, names::MCP_CALL],
            "the core's ring dropped the job's and the session's opens"
        );
        assert_eq!(kept("eileen-desktop"), [names::DESKTOP_CAPTURE], "the capture");
        let trace = to_chrome(&spans);
        let events = trace["traceEvents"].as_array().unwrap();
        let starts: Vec<&Value> = events.iter().filter(|e| e["ph"] == "s").collect();
        assert_eq!(starts.len(), 1, "one flow arrow");
        assert_eq!(events.iter().filter(|e| e["ph"] == "f").count(), 1, "with one end");
        assert_eq!(&starts[0]["pid"], pid_of(&trace, "eileen"), "from the core");
    }
}

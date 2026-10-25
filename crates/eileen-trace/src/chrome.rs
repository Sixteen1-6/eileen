//! The Chrome JSON trace converter: span records from any number of processes onto one
//! Perfetto timeline (observability report section 3.3).
//!
//! One `pid` per Eileen process, all aligned by wall clock. Perfetto needs the duration
//! events of a track to nest, and async spans hop threads, so each track root (a span with no
//! parent in its process, and every `turn`, `job` and `speak`) gets its own track with its
//! descendants on it. A span whose `traceparent` names a span in another process gets a flow
//! arrow from it, found as [`crate::schema`] finds a remote parent: by trace and span id.

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

    use eileen_contracts::ids::{TraceCtx, TurnId};
    use serde_json::{Value, json};

    use super::to_chrome;
    use crate::names;
    use crate::record::{SpanRecord, remote_parent, sorted};

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
}

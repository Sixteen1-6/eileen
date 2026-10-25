//! The Chrome JSON trace converter: span records from any number of processes onto one
//! Perfetto timeline (observability report section 3.3).
//!
//! One `pid` per Eileen process, all aligned by wall clock. Perfetto needs the duration
//! events of a track to nest, and async spans hop threads, so each track root (a span with no
//! parent in its process, and every `turn`, `job` and `speak`) gets its own track with its
//! descendants on it. A span whose parent is in another process gets a flow arrow from it.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::names;
use crate::record::SpanRecord;

/// Spans that start a track of their own even when they have a parent.
pub const TRACK_ROOTS: [&str; 3] = [names::TURN, names::JOB, names::SPEAK];

/// The trace as Perfetto and `chrome://tracing` read it: `{"traceEvents": [...]}`.
#[must_use]
pub fn to_chrome(spans: &[SpanRecord]) -> Value {
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
        let remote = span.remote_parent.and_then(|remote| {
            order.iter().find(|p| {
                p.process != span.process
                    && p.span_id == remote
                    && p.start_us <= span.start_us
                    && p.end_us.is_none_or(|e| e >= span.start_us)
            })
        });
        if let Some(parent) = remote {
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

    use serde_json::Value;

    use super::to_chrome;
    use crate::names;
    use crate::record::SpanRecord;

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

    #[test]
    fn processes_get_pids_roots_get_tracks_and_remote_parents_get_flows() {
        let mut apps = span("eileen-apps", 1, None, names::APPS_CALL, 20, 30);
        apps.remote_parent = Some(4);
        let spans = [
            span("eileen", 1, None, names::TURN, 0, 100),
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
}

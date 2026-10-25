//! The span-parent check: every span is declared in [`crate::names`] and sits under a parent
//! its row allows (observability report section 2.5, after the `trace_schema.py` of `LiveKit`
//! Agents). It runs on in-memory spans in tests (`test_support::Captured::assert_schema_ok`),
//! in every simulation run, and on the records of an incident.
//!
//! A span's parent is the record its `parent` names in its own process. A span without one
//! sits under the span its `traceparent` names in another process: the one with that span id,
//! open at the child's start, in the traceparent's trace. Every process hands out span ids
//! from the same small range, so the trace is what tells their spans apart; where two
//! processes still share an id within one trace, the span the child's row allows is taken. A
//! span is in the trace of each turn and job its process's records name for it or an ancestor
//! (`turn_id`, and a job's `job_id` and `origin_turn`; the trace id is the turn's ULID, or a
//! turnless job's), and in the one its local root's `traceparent` names.
//!
//! An incident's records are partial: a ring drops its oldest lines, so the `open` line of a
//! long job or turn is often gone while its children remain. A parent whose record is not
//! among them is accepted when the row allows a parent of that kind at all.

use std::collections::BTreeMap;
use std::fmt;

use eileen_contracts::ids::{JobId, TurnId};
use serde_json::Value;

use crate::names::{self, Kind, NameDef, Parent};
use crate::record::SpanRecord;

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// A span whose name is not in [`names::ALL`].
    Undeclared {
        /// The name.
        name: String,
    },
    /// A span with an event's name.
    NotASpan {
        /// The name.
        name: String,
    },
    /// A span under a parent its row does not allow (`None`: at the root).
    WrongParent {
        /// The span.
        name: String,
        /// Its parent, or `None` for the root.
        parent: Option<String>,
    },
    /// A span whose parent's record is missing (a ring dropped it, or its process's records
    /// are absent), under a row that allows no parent of that kind: none in the span's own
    /// process, or none in another when `remote`.
    WrongUnrecordedParent {
        /// The span.
        name: String,
        /// Whether the missing parent is the one its `traceparent` names.
        remote: bool,
    },
    /// A span that closed after its parent, which its row does not allow.
    OutlivesParent {
        /// The span.
        name: String,
        /// Its parent.
        parent: String,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undeclared { name } => {
                write!(f, "span `{name}` is not declared in eileen_trace::names")
            }
            Self::NotASpan { name } => write!(f, "`{name}` is declared as an event, not a span"),
            Self::WrongParent { name, parent: Some(parent) } => {
                write!(f, "span `{name}` may not sit under `{parent}`")
            }
            Self::WrongParent { name, parent: None } => {
                write!(f, "span `{name}` may not be a root")
            }
            Self::WrongUnrecordedParent { name, remote } => {
                let process = if *remote { "another" } else { "its own" };
                write!(
                    f,
                    "span `{name}` may not sit under a span of {process} process (whose record \
                     is missing)"
                )
            }
            Self::OutlivesParent { name, parent } => {
                write!(f, "span `{name}` outlived its parent `{parent}`")
            }
        }
    }
}

/// Checks every span. Records may come from several processes, and an incident's are partial:
/// a parent whose record is missing is accepted when the row allows a parent of that kind (in
/// the span's own process, or in another) at all.
#[must_use]
pub fn check(spans: &[SpanRecord]) -> Vec<Violation> {
    let parents = Parents::new(spans);
    let mut violations = Vec::new();
    for span in spans {
        let Some(def) = names::lookup(&span.name) else {
            violations.push(Violation::Undeclared { name: span.name.clone() });
            continue;
        };
        if def.kind == Kind::Event {
            violations.push(Violation::NotASpan { name: span.name.clone() });
            continue;
        }
        check_place(def, span, parents.place(span), &mut violations);
    }
    violations
}

/// The parent rules of `def`'s row for `span`, which the records put at `place`.
fn check_place(def: &NameDef, span: &SpanRecord, place: Place<'_>, out: &mut Vec<Violation>) {
    let name = || span.name.clone();
    match place {
        Place::Root if !allows(def, |rule| *rule == Parent::Root) => {
            out.push(Violation::WrongParent { name: name(), parent: None });
        }
        Place::Local(parent) => {
            if !allows(def, |rule| matches!(rule, Parent::Span(p) if *p == parent.name)) {
                out.push(Violation::WrongParent {
                    name: name(),
                    parent: Some(parent.name.clone()),
                });
            }
            let outlived =
                matches!((span.end_us, parent.end_us), (Some(child), Some(up)) if child > up);
            if outlived && !def.may_outlive_parent {
                out.push(Violation::OutlivesParent { name: name(), parent: parent.name.clone() });
            }
        }
        Place::Remote(parent)
            if !allows(def, |rule| matches!(rule, Parent::Remote(p) if *p == parent.name)) =>
        {
            out.push(Violation::WrongParent { name: name(), parent: Some(parent.name.clone()) });
        }
        Place::Unrecorded { remote }
            if !allows(def, |rule| {
                matches!((rule, remote), (Parent::Span(_), false) | (Parent::Remote(_), true))
            }) =>
        {
            out.push(Violation::WrongUnrecordedParent { name: name(), remote });
        }
        Place::Root | Place::Remote(_) | Place::Unrecorded { .. } => {}
    }
}

/// Whether `def`'s row has `Any` or a rule that `wanted` accepts.
fn allows(def: &NameDef, wanted: impl Fn(&Parent) -> bool) -> bool {
    def.parents.iter().any(|rule| *rule == Parent::Any || wanted(rule))
}

/// How a field's value reads as a trace id.
type ReadTrace = fn(&str) -> Option<u128>;

/// The fields that name the turn or job a span belongs to, each with how its value reads as a
/// trace id: `TraceCtx::traceparent` writes the turn's ULID, or the job's for a job with no
/// turn.
const TRACE_FIELDS: [(&str, ReadTrace); 3] =
    [("turn_id", turn_trace), ("origin_turn", turn_trace), ("job_id", job_trace)];

fn turn_trace(text: &str) -> Option<u128> {
    text.parse::<TurnId>().ok().map(|turn| turn.ulid().0)
}

fn job_trace(text: &str) -> Option<u128> {
    text.parse::<JobId>().ok().map(|job| job.ulid().0)
}

/// Where the records put a span.
#[derive(Debug, Clone, Copy)]
enum Place<'a> {
    /// At the root: no parent in its process and no `traceparent`.
    Root,
    /// Under this record, in its own process.
    Local(&'a SpanRecord),
    /// Under this record, in another process, through its `traceparent`.
    Remote(&'a SpanRecord),
    /// Under a parent the records do not hold.
    Unrecorded {
        /// Whether that parent is the one its `traceparent` names.
        remote: bool,
    },
}

/// The records indexed for finding a span's parent; [`crate::chrome`] draws its flow arrows
/// with it too.
pub(crate) struct Parents<'a> {
    /// Each record by its process and record id.
    local: BTreeMap<(&'a str, u64), &'a SpanRecord>,
    /// The records holding each `tracing` span id, in the order given.
    by_span_id: BTreeMap<u64, Vec<&'a SpanRecord>>,
}

impl<'a> Parents<'a> {
    /// Indexes `spans`.
    pub(crate) fn new(spans: &'a [SpanRecord]) -> Self {
        let mut local = BTreeMap::new();
        let mut by_span_id: BTreeMap<u64, Vec<&'a SpanRecord>> = BTreeMap::new();
        for span in spans {
            local.insert((span.process.as_str(), span.id), span);
            by_span_id.entry(span.span_id).or_default().push(span);
        }
        Self { local, by_span_id }
    }

    /// Where `span` sits: under its local parent when it has one, else under the span its
    /// `traceparent` names, else at the root.
    fn place(&self, span: &SpanRecord) -> Place<'a> {
        if let Some(parent) = span.parent {
            return self
                .local
                .get(&(span.process.as_str(), parent))
                .copied()
                .map_or(Place::Unrecorded { remote: false }, Place::Local);
        }
        match (span.remote_parent, self.remote(span)) {
            (None, _) => Place::Root,
            (Some(_), Some(parent)) => Place::Remote(parent),
            (Some(_), None) => Place::Unrecorded { remote: true },
        }
    }

    /// The span that `span`'s `traceparent` names in another process: the one with that span
    /// id, open at `span`'s start, in the traceparent's trace. Where there are several, the
    /// first whose name `span`'s row allows as a remote parent, else the first.
    pub(crate) fn remote(&self, span: &SpanRecord) -> Option<&'a SpanRecord> {
        let named = span.remote_parent?;
        let row = names::lookup(&span.name).map(|def| def.parents).unwrap_or_default();
        let mut first = None;
        for &parent in self.by_span_id.get(&named.parent_id)? {
            let open_then = parent.start_us <= span.start_us
                && parent.end_us.is_none_or(|end| end >= span.start_us);
            if parent.process == span.process
                || !open_then
                || !self.in_trace(parent, named.trace_id)
            {
                continue;
            }
            if row.iter().any(|rule| matches!(rule, Parent::Remote(p) if *p == parent.name)) {
                return Some(parent);
            }
            first = first.or(Some(parent));
        }
        first
    }

    /// Whether `span` is in `trace`: the trace of a turn or job its process's records name for
    /// it or an ancestor, or the one its local root's `traceparent` names. `false` where an
    /// ancestor's record is missing before any of them is found.
    fn in_trace(&self, span: &'a SpanRecord, trace: u128) -> bool {
        let mut at = span;
        // A walk longer than the records can only follow a parent loop in damaged records.
        for _ in 0..=self.local.len() {
            let named = TRACE_FIELDS.iter().any(|&(field, read)| {
                at.fields.get(field).and_then(Value::as_str).and_then(read) == Some(trace)
            });
            if named {
                return true;
            }
            let Some(parent) = at.parent else {
                return at.remote_parent.is_some_and(|tp| tp.trace_id == trace);
            };
            let Some(&up) = self.local.get(&(at.process.as_str(), parent)) else {
                return false;
            };
            at = up;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::Duration;

    use eileen_contracts::ids::{JobId, TraceCtx, TurnId};
    use serde_json::{Value, json};
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::{TRACE_FIELDS, Violation, check};
    use crate::names;
    use crate::record::{SpanRecord, remote_parent, sorted};
    use crate::redact::Redactor;
    use crate::ring::{Ring, RingLayer};

    fn span(id: u64, parent: Option<u64>, name: &str, start: i64, end: i64) -> SpanRecord {
        SpanRecord {
            id,
            span_id: id,
            parent,
            remote_parent: None,
            name: name.to_owned(),
            process: "eileen".to_owned(),
            start_us: start,
            end_us: Some(end),
            fields: BTreeMap::new(),
        }
    }

    /// `span` as `process` recorded it with `fields`, its remote parent read from them the way
    /// the ring and the capture layer read a `traceparent`.
    fn recorded(process: &str, mut span: SpanRecord, fields: &Value) -> SpanRecord {
        let fields = fields.as_object().cloned().unwrap_or_default();
        span.process = process.to_owned();
        span.remote_parent = remote_parent(&fields);
        span.fields = sorted(&fields);
        span
    }

    /// A turn's context, from which the core writes a `traceparent`.
    const fn in_turn(turn: TurnId) -> TraceCtx {
        TraceCtx { turn: Some(turn), job: None, call: None }
    }

    #[test]
    fn a_well_formed_turn_passes() {
        let spans = [
            span(1, None, names::TURN, 0, 100),
            span(2, Some(1), names::ROUTER_DECIDE, 1, 2),
            span(3, Some(1), names::JOB, 3, 500),
        ];
        assert_eq!(check(&spans), Vec::new(), "a job may outlive its turn");
    }

    #[test]
    fn misplaced_undeclared_and_outliving_spans_fail() {
        let spans = [
            span(1, None, names::TURN, 0, 100),
            span(2, Some(1), names::DISPATCH_CALL, 1, 2),
            span(3, None, "made.up", 0, 1),
            span(4, Some(1), names::ADDRESSEE, 1, 200),
            span(5, None, names::BRAIN_TURN, 0, 1),
        ];
        let found = check(&spans);
        assert!(
            found.contains(&Violation::WrongParent {
                name: names::DISPATCH_CALL.to_owned(),
                parent: Some(names::TURN.to_owned())
            }),
            "{found:?}"
        );
        assert!(found.contains(&Violation::Undeclared { name: "made.up".to_owned() }), "{found:?}");
        assert!(
            found.contains(&Violation::OutlivesParent {
                name: names::ADDRESSEE.to_owned(),
                parent: names::TURN.to_owned()
            }),
            "{found:?}"
        );
        assert!(
            found.contains(&Violation::WrongParent {
                name: names::BRAIN_TURN.to_owned(),
                parent: None
            }),
            "{found:?}"
        );
    }

    #[test]
    fn a_sidecar_span_with_neither_a_remote_nor_a_local_parent_is_misplaced() {
        // A regression that drops `_meta.traceparent` turns a desktop action into a root.
        let mut action = span(1, None, names::DESKTOP_ACTION, 0, 1);
        action.process = "eileen-desktop".to_owned();
        assert_eq!(
            check(&[action]),
            [Violation::WrongParent { name: names::DESKTOP_ACTION.to_owned(), parent: None }],
            "a desktop.action belongs under a remote mcp.call"
        );
        let mut synth = span(2, None, names::TTS_SYNTH, 0, 1);
        synth.process = "eileen-speech".to_owned();
        assert_eq!(
            check(&[synth]),
            [Violation::WrongParent { name: names::TTS_SYNTH.to_owned(), parent: None }],
            "so does a tts.synth without its speak"
        );
        assert_eq!(
            check(&[span(3, None, names::SPEAK, 0, 1)]),
            Vec::new(),
            "an announcement's speak may start its own trace"
        );
    }

    #[test]
    fn remote_parents_resolve_across_processes() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        // The apps sidecar's call, under the span with id `remote` in the core.
        let records = |remote: u64| {
            [
                recorded(
                    "eileen",
                    span(5, None, names::TURN, 0, 100),
                    &json!({"turn_id": turn.to_string()}),
                ),
                span(8, Some(5), names::BRAIN_TURN, 1, 90),
                span(6, Some(8), names::DISPATCH_CALL, 5, 60),
                span(7, Some(6), names::MCP_CALL, 10, 50),
                recorded(
                    "eileen-apps",
                    span(1, None, names::APPS_CALL, 20, 30),
                    &json!({"traceparent": in_turn(turn).traceparent(remote)}),
                ),
            ]
        };
        assert_eq!(check(&records(7)), Vec::new(), "under the mcp.call its traceparent names");
        assert_eq!(
            check(&records(5)),
            [Violation::WrongParent {
                name: names::APPS_CALL.to_owned(),
                parent: Some(names::TURN.to_owned())
            }],
            "the parent found decides: an apps.call may not sit under the turn"
        );
    }

    #[test]
    fn another_process_holding_the_same_span_id_does_not_take_a_remote_child() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let other = TurnId::from_parts(1_761_000_000_000, 2);
        let core = [
            recorded(
                "eileen",
                span(1, None, names::TURN, 0, 90),
                &json!({"turn_id": turn.to_string()}),
            ),
            span(2, Some(1), names::BRAIN_TURN, 2, 80),
            span(4, Some(2), names::DISPATCH_CALL, 5, 60),
            span(3, Some(4), names::MCP_CALL, 10, 50),
        ];
        let capture = recorded(
            "eileen-desktop",
            span(1, None, names::DESKTOP_CAPTURE, 20, 30),
            &json!({"traceparent": in_turn(turn).traceparent(3)}),
        );
        // A speech segment with the mcp.call's span id, open over the capture and first in the
        // records: in another turn's trace, then in the same one.
        for (case, segment_turn) in [("another trace", other), ("the same trace", turn)] {
            let segment = recorded(
                "eileen-speech",
                span(3, None, names::SPEECH_SEGMENT, 0, 100),
                &json!({
                    "turn_id": segment_turn.to_string(),
                    "traceparent": in_turn(segment_turn).traceparent(1),
                }),
            );
            let spans: Vec<SpanRecord> = [segment]
                .into_iter()
                .chain(core.iter().cloned())
                .chain([capture.clone()])
                .collect();
            assert_eq!(check(&spans), Vec::new(), "{case}");
        }
    }

    #[test]
    fn a_span_is_in_the_trace_of_its_turn_or_of_its_job() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let job = JobId::from_parts(1_761_000_000_000, 2);
        let in_job = TraceCtx { turn: None, job: Some(job), call: None };
        // A job whose turn the ring dropped, which names that turn, and a job with no turn.
        let dropped_turn = recorded(
            "eileen",
            span(2, Some(1), names::JOB, 5, 500),
            &json!({"job_id": job.to_string(), "origin_turn": turn.to_string()}),
        );
        let no_turn = recorded(
            "eileen",
            span(2, None, names::JOB, 5, 500),
            &json!({"job_id": job.to_string()}),
        );
        // Each traceparent names the worker.session's span id, and no desktop.action may sit
        // under a worker.session, so the violation shows that the parent was found.
        let records = |job_span: &SpanRecord, ctx: TraceCtx| {
            [
                job_span.clone(),
                span(3, Some(2), names::WORKER_SESSION, 6, 400),
                recorded(
                    "eileen-desktop",
                    span(1, None, names::DESKTOP_ACTION, 10, 20),
                    &json!({"traceparent": ctx.traceparent(3)}),
                ),
            ]
        };
        let found = [Violation::WrongParent {
            name: names::DESKTOP_ACTION.to_owned(),
            parent: Some(names::WORKER_SESSION.to_owned()),
        }];
        assert_eq!(check(&records(&dropped_turn, in_turn(turn))), found, "its origin_turn");
        assert_eq!(check(&records(&dropped_turn, in_job)), found, "its job_id");
        assert_eq!(check(&records(&no_turn, in_job)), found, "a job with no turn");
        assert_eq!(
            check(&records(&no_turn, in_turn(turn))),
            Vec::new(),
            "another trace: the parent is not among the records"
        );
    }

    #[test]
    fn a_parent_missing_from_the_records_is_not_a_misplaced_root() {
        assert_eq!(
            check(&[span(2, Some(1), names::ROUTER_DECIDE, 5, 6)]),
            Vec::new(),
            "the record names a parent, so the span is not a root"
        );
    }

    #[test]
    fn a_ring_that_dropped_the_parents_open_lines_passes() {
        let ring = Arc::new(Ring::new("eileen", 4_096, Duration::from_secs(600)));
        let subscriber = tracing_subscriber::registry().with(RingLayer::new(Arc::clone(&ring)));
        tracing::subscriber::with_default(subscriber, || {
            let job = tracing::info_span!(names::JOB, job_id = "job_1");
            let _job = job.enter();
            let session = tracing::info_span!(names::WORKER_SESSION, route = "claude");
            let _session = session.enter();
            for n in 0..100 {
                tracing::debug!(n, "filler");
            }
            tracing::info_span!(names::WORKER_TOOL_USE, tool = "Read").in_scope(|| {});
        });
        let spans = ring.records(&Redactor::new()).spans;
        let kept: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            kept,
            [names::WORKER_TOOL_USE],
            "the ring dropped the job's and session's opens"
        );
        assert_eq!(check(&spans), Vec::new(), "a child whose parent the ring dropped is no root");
    }

    #[test]
    fn a_missing_parent_of_a_kind_the_row_never_allows_fails() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let spans = [
            span(2, Some(1), names::TURN, 5, 6),
            recorded(
                "eileen",
                span(3, None, names::ROUTER_DECIDE, 5, 6),
                &json!({"traceparent": in_turn(turn).traceparent(9)}),
            ),
        ];
        let expected = [
            Violation::WrongUnrecordedParent { name: names::TURN.to_owned(), remote: false },
            Violation::WrongUnrecordedParent {
                name: names::ROUTER_DECIDE.to_owned(),
                remote: true,
            },
        ];
        assert_eq!(check(&spans), expected, "a turn is a root; a router.decide is under its turn");
        assert_eq!(
            expected[0].to_string(),
            "span `turn` may not sit under a span of its own process (whose record is missing)",
            "the message"
        );
    }

    #[test]
    fn a_parent_loop_in_damaged_records_ends() {
        let turn = TurnId::from_parts(1_761_000_000_000, 1);
        let spans = [
            span(1, Some(2), names::DISPATCH_CALL, 0, 100),
            span(2, Some(1), names::MCP_CALL, 0, 100),
            recorded(
                "eileen-apps",
                span(1, None, names::APPS_CALL, 20, 30),
                &json!({"traceparent": in_turn(turn).traceparent(2)}),
            ),
        ];
        assert_eq!(
            check(&spans),
            [Violation::WrongParent {
                name: names::DISPATCH_CALL.to_owned(),
                parent: Some(names::MCP_CALL.to_owned())
            }],
            "the walk up the loop gives up, so the mcp.call is not shown to be in the trace"
        );
    }

    #[test]
    fn the_trace_fields_are_declared_in_the_table() {
        for (field, _) in TRACE_FIELDS {
            assert!(names::ALL.iter().any(|def| def.fields.contains(&field)), "{field}");
        }
    }
}

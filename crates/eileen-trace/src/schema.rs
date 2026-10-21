//! The span-parent check: every span is declared in [`crate::names`] and sits under a parent
//! its row allows (observability report section 2.5, after the `trace_schema.py` of `LiveKit` Agents). It
//! runs on in-memory spans in tests (`test_support::Captured::assert_schema_ok`), in every
//! simulation run, and on the records of an incident.

use std::collections::BTreeMap;
use std::fmt;

use crate::names::{self, Kind, Parent};
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
            Self::OutlivesParent { name, parent } => {
                write!(f, "span `{name}` outlived its parent `{parent}`")
            }
        }
    }
}

/// Checks every span. Records may come from several processes; a remote parent that is not
/// among them is accepted when the row allows a remote parent at all.
#[must_use]
pub fn check(spans: &[SpanRecord]) -> Vec<Violation> {
    let by_id: BTreeMap<(&str, u64), &SpanRecord> =
        spans.iter().map(|s| ((s.process.as_str(), s.id), s)).collect();
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
        let any = def.parents.contains(&Parent::Any);
        if let Some(parent) = span.parent.and_then(|p| by_id.get(&(span.process.as_str(), p))) {
            let allowed =
                any || def.parents.iter().any(|r| *r == Parent::Span(names_static(&parent.name)));
            if !allowed {
                violations.push(Violation::WrongParent {
                    name: span.name.clone(),
                    parent: Some(parent.name.clone()),
                });
            }
            let outlived =
                matches!((span.end_us, parent.end_us), (Some(child), Some(up)) if child > up);
            if outlived && !def.may_outlive_parent {
                violations.push(Violation::OutlivesParent {
                    name: span.name.clone(),
                    parent: parent.name.clone(),
                });
            }
        } else if let Some(remote) = span.remote_parent {
            let found = spans.iter().find(|p| {
                p.process != span.process
                    && p.span_id == remote
                    && p.start_us <= span.start_us
                    && p.end_us.is_none_or(|end| end >= span.start_us)
            });
            let allowed = match found {
                Some(parent) => {
                    any || def
                        .parents
                        .iter()
                        .any(|r| *r == Parent::Remote(names_static(&parent.name)))
                }
                None => any || def.parents.iter().any(|r| matches!(r, Parent::Remote(_))),
            };
            if !allowed {
                violations.push(Violation::WrongParent {
                    name: span.name.clone(),
                    parent: found.map(|p| p.name.clone()),
                });
            }
        } else if !(any || def.parents.contains(&Parent::Root)) {
            violations.push(Violation::WrongParent { name: span.name.clone(), parent: None });
        }
    }
    violations
}

/// The declared `&'static str` for a name, so rows compare by value.
fn names_static(name: &str) -> &'static str {
    names::lookup(name).map_or("", |def| def.name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Violation, check};
    use crate::names;
    use crate::record::SpanRecord;

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
    fn remote_parents_resolve_across_processes() {
        let mut call = span(7, None, names::MCP_CALL, 10, 50);
        call.parent = Some(6);
        let mut apps = span(1, None, names::APPS_CALL, 20, 30);
        apps.process = "eileen-apps".to_owned();
        apps.remote_parent = Some(7);
        let spans = [
            span(5, None, names::TURN, 0, 100),
            {
                let mut d = span(6, Some(5), names::DISPATCH_CALL, 5, 60);
                d.parent = Some(5);
                d
            },
            call,
            apps,
        ];
        let found = check(&spans);
        assert!(
            !found.iter().any(
                |v| matches!(v, Violation::WrongParent { name, .. } if name == names::APPS_CALL)
            ),
            "{found:?}"
        );
    }
}

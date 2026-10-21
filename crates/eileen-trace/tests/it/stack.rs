//! The whole stack: the file's line shape, the slow-span event, the reload handle, the stage
//! stats, and `init` with soft assertions reporting.

use std::path::PathBuf;

use eileen_stdx::soft_assert::{self, Mode};
use eileen_trace::{Console, ProcessKind, TraceConfig, TraceError, names, test_support};
use serde_json::Value;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_lines_carry_their_span_and_reduce_pii() {
    let dir =
        TempDir(std::env::temp_dir().join(format!("eileen-trace-stack-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    config.slow_span_ms.insert(names::ADDRESSEE.to_owned(), 0);
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Desktop, &config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        let turn = tracing::info_span!(target: "eileen_core", names::TURN, turn_id = "turn_1");
        let _entered = turn.enter();
        tracing::info_span!(target: "eileen_core", names::ADDRESSEE, decision = "addressed").in_scope(|| {
            tracing::info!(target: "eileen_core", window = 7, "pii.title" = "Holiday photos", "focused");
        });
        guard.set_file_filter("warn").unwrap();
        tracing::info!(target: "eileen_core", "after the reload");
    });
    let stats = guard.stats().snapshot();
    drop(guard);
    let entries: Vec<_> =
        std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(entries.len(), 1, "one daily file");
    let name = entries[0].to_string_lossy().into_owned();
    assert!(name.starts_with("eileen-desktop."), "{name}");
    assert_eq!(
        std::path::Path::new(&name).extension().and_then(|e| e.to_str()),
        Some("jsonl"),
        "{name}"
    );
    let text = std::fs::read_to_string(dir.0.join(&name)).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let focused = lines.iter().find(|l| l["fields"]["message"] == "focused").unwrap();
    assert_eq!(focused["span"]["name"], names::ADDRESSEE, "the current span");
    assert_eq!(focused["spans"][0]["turn_id"], "turn_1", "the span list carries the turn id");
    assert_eq!(focused["fields"]["pii.title"]["len"], 14, "pii reduced to its length");
    assert_eq!(focused["process"], "eileen-desktop", "process");
    assert!(focused["timestamp"].as_str().unwrap().ends_with('Z'), "UTC timestamp");
    assert!(focused["line_number"].is_u64() && focused["filename"].is_string(), "a code location");
    assert!(
        lines
            .iter()
            .any(|l| l["fields"]["span"] == names::ADDRESSEE && l["fields"]["dur_ms"].is_u64()),
        "span.slow"
    );
    assert!(!text.contains("after the reload"), "the reloaded filter applied at once");
    assert_eq!(stats.get(names::ADDRESSEE).map(|s| s.count), Some(1), "stage stats");
}

#[test]
fn capture_checks_the_schema() {
    let ((), captured) = test_support::capture(|| {
        let turn = tracing::info_span!(names::TURN);
        let _entered = turn.enter();
        tracing::info_span!(names::JOB, job_id = "job_1").in_scope(|| tracing::info!("inside"));
    });
    captured.assert_schema_ok();
    assert_eq!(
        captured.spans_named(names::JOB)[0].parent,
        Some(captured.spans_named(names::TURN)[0].id),
        "parent"
    );
    let misplaced = std::panic::catch_unwind(|| {
        let ((), wrong) =
            test_support::capture(|| tracing::info_span!(names::DISPATCH_CALL).in_scope(|| {}));
        wrong.assert_schema_ok();
    });
    assert!(misplaced.is_err(), "a dispatch.call at the root fails the check");
}

#[test]
fn init_installs_once_and_soft_assertions_report() {
    let mut config = TraceConfig::new(None);
    config.console = Console::Never;
    let guard = eileen_trace::init(ProcessKind::Cli, &config).unwrap();
    assert!(
        matches!(
            eileen_trace::init(ProcessKind::Cli, &config),
            Err(TraceError::AlreadyInitialised)
        ),
        "a second init is refused"
    );
    assert_eq!(soft_assert::mode(), Mode::Report, "product processes report");
    let failed = eileen_stdx::never!(true, "a canary soft assertion");
    assert!(failed, "the condition comes back");
    assert!(guard.ring().take_dump_request(), "a dump was requested");
    let parsed = guard.ring().records(guard.redactor());
    assert_eq!(parsed.skipped, 0, "every ring line read back as a record");
    assert!(parsed.events.iter().any(|e| e.name == names::SOFT_ASSERT_FAILED), "logged");
}

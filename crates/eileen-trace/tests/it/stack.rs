//! The whole stack: the file's line shape, a span trace in a line, the slow-span event, the
//! reload handle, the stage stats, which daily files a process keeps, and `init` with soft
//! assertions reporting, a `log` logger set before it and a scoped default held across it
//! included.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eileen_contracts::ids::Date;
use eileen_stdx::clock::{self, Clock as _};
use eileen_stdx::soft_assert::{self, Mode};
use eileen_trace::{Console, ProcessKind, TraceConfig, TraceError, names, test_support};
use serde_json::Value;
use tracing::dispatcher::DefaultGuard;
use tracing_error::SpanTrace;
use tracing_subscriber::util::SubscriberInitExt as _;

struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty folder for the test `name`.
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("eileen-trace-{name}-{}", std::process::id()));
        let _ignored = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn touch(dir: &Path, name: &str) {
    std::fs::File::create(dir.join(name)).unwrap();
}

fn names_in(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect()
}

/// The text of every file in `dir`, in name order.
fn files_text(dir: &Path) -> String {
    names_in(dir).iter().map(|name| std::fs::read_to_string(dir.join(name)).unwrap()).collect()
}

/// The core's daily file name `back` days before `today`.
fn core_file(today: Date, back: i64) -> String {
    format!("eileen.{}.jsonl", Date::from_days_since_epoch(today.days_since_epoch() - back))
}

/// Builds `kind`'s stack on `dir` with the default retention and drops it, as a process that
/// starts and stops.
fn start_and_stop(kind: ProcessKind, dir: &Path) {
    let mut config = TraceConfig::new(Some(dir.to_path_buf()));
    config.console = Console::Never;
    assert_eq!(config.max_log_files, 14, "the default retention");
    let (_subscriber, guard) = eileen_trace::build(kind, &config).unwrap();
    drop(guard);
}

#[test]
fn the_core_keeps_the_other_processes_files() {
    // The core's name, `eileen`, begins every other process's file name.
    let dir = TempDir::new("keeps-others");
    let others: BTreeSet<String> = ["eileen-speech", "eileen-desktop"]
        .into_iter()
        .flat_map(|process| (1..=14).map(move |day| format!("{process}.2026-10-{day:02}.jsonl")))
        .collect();
    for name in &others {
        touch(&dir.0, name);
    }
    start_and_stop(ProcessKind::Core, &dir.0);
    let left = names_in(&dir.0);
    let gone: Vec<&String> = others.difference(&left).collect();
    assert!(gone.is_empty(), "the core deleted {} of the others' files: {gone:?}", gone.len());
}

#[test]
fn the_core_prunes_its_own_files_by_the_date_in_their_names() {
    let dir = TempDir::new("prunes-own");
    let today = clock::system().wall().date();
    // Made newest first, so their birth times run against the dates in their names.
    for back in 1..=20 {
        touch(&dir.0, &core_file(today, back));
    }
    let not_daily = [
        "eileen.jsonl".to_owned(),
        "eileen.notes.jsonl".to_owned(),
        "eileen.2026-02-30.jsonl".to_owned(),
        format!("{}.old", core_file(today, 30)),
    ];
    for name in &not_daily {
        touch(&dir.0, name);
    }
    std::fs::create_dir_all(dir.0.join(core_file(today, 40))).unwrap();
    // Dated after today, as a clock that ran ahead leaves: it stays and takes none of the 13.
    touch(&dir.0, &core_file(today, -2));
    start_and_stop(ProcessKind::Core, &dir.0);
    // Today's, which the stack opened, and the 13 newest days before it.
    let mut kept: BTreeSet<String> = (0..=13).map(|back| core_file(today, back)).collect();
    kept.extend(not_daily);
    kept.insert(core_file(today, 40));
    kept.insert(core_file(today, -2));
    assert_eq!(names_in(&dir.0), kept, "the oldest days went, and nothing else");
}

#[test]
fn a_first_start_makes_the_folder_and_todays_file() {
    let parent = TempDir::new("first-start");
    let dir = parent.0.join("logs");
    start_and_stop(ProcessKind::Core, &dir);
    let today = clock::system().wall().date();
    assert_eq!(names_in(&dir), BTreeSet::from([core_file(today, 0)]), "only today's file");
}

#[test]
fn a_max_of_zero_keeps_every_daily_file() {
    // tracing-appender's `max_log_files(0)` turned pruning off, and 0 keeps that meaning.
    let dir = TempDir::new("keep-all");
    let today = clock::system().wall().date();
    for back in 1..=5 {
        touch(&dir.0, &core_file(today, back));
    }
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    config.max_log_files = 0;
    let (_subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    drop(guard);
    let every: BTreeSet<String> = (0..=5).map(|back| core_file(today, back)).collect();
    assert_eq!(names_in(&dir.0), every, "today's and every older day stay");
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
    assert_eq!(Path::new(&name).extension().and_then(|e| e.to_str()), Some("jsonl"), "{name}");
    let text = std::fs::read_to_string(dir.0.join(&name)).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let focused = lines.iter().find(|l| l["fields"]["message"] == "focused").unwrap();
    assert_eq!(focused["span"]["name"], names::ADDRESSEE, "the current span");
    assert_eq!(focused["spans"][0]["turn_id"], "turn_1", "the span list carries the turn id");
    assert_eq!(focused["fields"]["pii.title"]["len"], 14, "pii reduced to its length");
    assert_eq!(
        focused["fields"]["pii.title"].get("h"),
        None,
        "TraceConfig::new sets no pii key, so no hash under a key anyone could know"
    );
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
fn a_logged_span_trace_shows_pii_fields_as_their_length() {
    // An internal error logs the span trace it captured as a field (observability report
    // section 2.4 rule 6), and the panic hook records one (section 4.5).
    let dir = TempDir::new("span-trace");
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        let turn = tracing::info_span!(target: "eileen_core", names::TURN, turn_id = "t", "pii.transcript" = "my diary entry");
        let _entered = turn.enter();
        let trace = SpanTrace::capture();
        tracing::error!(target: "eileen_core", trace = %trace, debug = ?trace, "failed");
    });
    drop(guard);
    let text: String = names_in(&dir.0)
        .iter()
        .map(|name| std::fs::read_to_string(dir.0.join(name)).unwrap())
        .collect();
    assert!(!text.contains("my diary entry"), "the transcript reached the file:\n{text}");
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let failed = lines.iter().find(|l| l["fields"]["message"] == "failed").unwrap();
    let shown = failed["fields"]["trace"].as_str().unwrap();
    assert!(
        shown.contains(r#"with turn_id="t" pii.transcript={"len":14}"#),
        "the trace names its turn and gives the transcript's length alone:\n{shown}"
    );
    let debug = failed["fields"]["debug"].as_str().unwrap();
    assert!(debug.contains(r#"turn_id=\"t\" pii.transcript={\"len\":14}"#), "Debug too:\n{debug}");
    assert_eq!(failed["spans"][0]["pii.transcript"], serde_json::json!({ "len": 14 }), "span list");
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

/// `init` with a log folder once tracing-log's `log` logger is set: the stack starts all the
/// same. `scoped`, when given, is this thread's default while `init` runs, and it ends before
/// this thread logs to the stack.
fn init_starts_after_a_log_logger(name: &str, scoped: Option<DefaultGuard>) {
    let dir = TempDir::new(name);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let guard = eileen_trace::init(ProcessKind::Cli, &config).unwrap();
    assert!(
        matches!(
            eileen_trace::init(ProcessKind::Cli, &config),
            Err(TraceError::AlreadyInitialised)
        ),
        "a second init is still refused"
    );
    assert_eq!(soft_assert::mode(), Mode::Report, "soft assertions report");
    assert!(eileen_stdx::never!(true, "a canary soft assertion"), "the condition comes back");
    assert!(guard.ring().take_dump_request(), "the hook found the ring");
    drop(scoped);
    tracing::info!(target: "eileen_core", "after init");
    let parsed = guard.ring().records(guard.redactor());
    let warned = parsed.events.iter().filter(|e| e.name == names::LOG_BRIDGE_FAILED).count();
    assert_eq!(warned, 1, "one warning says that `log` records stay with the other logger");
    drop(guard);
    let text = files_text(&dir.0);
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(lines.iter().any(|l| l["fields"]["message"] == "after init"), "still writing:\n{text}");
    assert!(
        lines.iter().any(|l| l["level"] == "WARN"
            && l["target"] == "eileen_trace"
            && l["fields"]["error"].as_str().is_some_and(|e| e.contains("logger"))),
        "the warning reached the file too:\n{text}"
    );
}

#[test]
fn init_starts_the_stack_when_a_log_logger_came_first() {
    // `set_default` sets tracing-log's `log` logger and no global default, as a library's
    // `env_logger::init()` before `init` would.
    drop(tracing_subscriber::registry().set_default());
    init_starts_after_a_log_logger("log-first", None);
}

#[test]
fn init_starts_the_stack_while_a_scoped_default_is_held() {
    // The guard, held across `init` as a test harness or a library may hold one, sets
    // tracing-log's `log` logger and keeps this thread's default its own.
    let scoped = tracing_subscriber::registry().set_default();
    init_starts_after_a_log_logger("log-first-scoped", Some(scoped));
}

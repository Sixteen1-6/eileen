//! Tasks under a capturing subscriber on a current-thread runtime.

use std::path::PathBuf;

use eileen_rt::task;
use eileen_trace::{Console, ProcessKind, TraceConfig, names, test_support};

fn current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// What a task finds when it looks for a window (an error that quotes the user's title).
fn find_window(title: &str) -> Result<(), String> {
    Err(format!("no window titled {title}"))
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_task_keeps_the_span_it_was_spawned_in() {
    let ((), captured) = test_support::capture(|| {
        current_thread().block_on(async {
            let turn = tracing::info_span!(names::TURN, turn_id = "turn_1");
            let handle = {
                let _entered = turn.enter();
                task::spawn("test.child", async { tracing::info!("from the task") })
            };
            handle.await.unwrap();
        });
    });
    let turn = captured.spans_named(names::TURN)[0].id;
    let event = captured
        .events
        .iter()
        .find(|e| e.fields.get("message").and_then(|m| m.as_str()) == Some("from the task"));
    assert_eq!(event.unwrap().span, Some(turn), "the task's event carries the turn's span");
}

#[test]
fn a_running_task_shows_in_the_dump_and_leaves_it_when_done() {
    current_thread().block_on(async {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = task::spawn("test.waiting", async move {
            let _ignored = rx.await;
        });
        tokio::task::yield_now().await;
        assert!(task::dump().contains("test.waiting"), "listed:\n{}", task::dump());
        tx.send(()).unwrap();
        handle.await.unwrap();
        assert!(!task::dump().contains("test.waiting"), "gone:\n{}", task::dump());
    });
}

#[test]
fn a_panicking_task_is_logged_and_fails_its_handle() {
    let ((), captured) = test_support::capture(|| {
        current_thread().block_on(async {
            let result =
                task::spawn("test.panics", async { panic!("boom at `secret text`") }).await;
            assert!(result.unwrap_err().is_panic(), "the join handle reports the panic");
            let blocking =
                task::spawn_blocking("test.blocking.panics", || panic!("blocking boom")).await;
            assert!(blocking.unwrap_err().is_panic(), "the blocking handle reports the panic");
        });
    });
    let panics = captured.events_named(names::TASK_PANICKED);
    assert_eq!(panics.len(), 2, "both logged");
    assert_eq!(panics[0].fields["task"], "test.panics", "named");
    assert_eq!(
        panics[0].fields["pii.message"], "boom at `secret text`",
        "the whole message, in the pii field the files reduce"
    );
    assert!(!panics[0].fields.contains_key("message"), "no plain message field");
}

#[test]
fn a_panic_message_reaches_the_log_files_only_as_its_length() {
    let dir = TempDir(std::env::temp_dir().join(format!("eileen-rt-panic-{}", std::process::id())));
    let _ignored = std::fs::remove_dir_all(&dir.0);
    let mut config = TraceConfig::new(Some(dir.0.clone()));
    config.console = Console::Never;
    let (subscriber, guard) = eileen_trace::build(ProcessKind::Core, &config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        current_thread().block_on(async {
            let unwrapped = task::spawn("test.unwraps", async {
                find_window("Jordan Vale tax return").unwrap();
            })
            .await;
            assert!(unwrapped.unwrap_err().is_panic(), "the handle reports the panic");
        });
    });
    drop(guard);
    let mut files = String::new();
    for entry in std::fs::read_dir(&dir.0).unwrap() {
        files.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
    }
    assert!(files.contains("test.unwraps"), "the panic reached the file:\n{files}");
    assert!(files.contains(r#""pii.message":{"len":"#), "as its length:\n{files}");
    for part in ["Jordan", "tax return", "no window titled", "unwrap()"] {
        assert!(!files.contains(part), "{part} reached the file:\n{files}");
    }
}

#[test]
fn a_blocking_call_outside_a_runtime_leaves_no_entry_in_the_dump() {
    let outside = std::panic::catch_unwind(|| task::spawn_blocking("test.outside", || ()));
    assert!(outside.is_err(), "tokio refuses a blocking task outside a runtime");
    assert!(!task::dump().contains("test.outside"), "no phantom entry:\n{}", task::dump());
}

#[test]
fn spawn_blocking_runs_in_the_callers_span() {
    let ((), captured) = test_support::capture(|| {
        current_thread().block_on(async {
            let job = tracing::info_span!(names::JOB, job_id = "job_1");
            let handle = {
                let _entered = job.enter();
                task::spawn_blocking("test.blocking", || tracing::info!("blocking work"))
            };
            handle.await.unwrap();
        });
    });
    let job = captured.spans_named(names::JOB)[0].id;
    let event = captured
        .events
        .iter()
        .find(|e| e.fields.get("message").and_then(|m| m.as_str()) == Some("blocking work"));
    assert_eq!(event.unwrap().span, Some(job), "in the job's span");
}

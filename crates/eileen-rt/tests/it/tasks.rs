//! Tasks under a capturing subscriber on a current-thread runtime.

use eileen_rt::task;
use eileen_trace::{names, test_support};

fn current_thread() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
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
    assert_eq!(panics[0].fields["message"], "boom at `<redacted>`", "quoted text removed");
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

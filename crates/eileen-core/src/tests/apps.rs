//! App tasks through `Core::step` (BUILD.md section 2.5, "App tasks"): a render the apps
//! sidecar reports, with its progress and its spoken end, its cancel by voice or through the
//! brain, the focus and route words passing it over, and its end when the sidecar exits.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::connectors::{AppEvent, ConnectorEvent, SidecarEvent, SidecarId};
use eileen_contracts::core_io::Input;
use eileen_contracts::ids::JobId;
use eileen_contracts::jobs::{JobKind, JobStatus};
use serde_json::json;

use super::jobs::{finished, init, tool_reply};
use super::steering::{accepted_all, announcements, control, replies, row, start, starts};
use super::{Harness, ask_brain, done, events};

/// What Eileen says when she stops tracking a render the app goes on with.
const APP_LEFT: &str = "I stopped tracking that; the app is still running it.";

/// The apps sidecar's report of Resolve's render.
fn render(event: AppEvent) -> Input {
    let app = "resolve".to_owned();
    Input::Sidecar(SidecarId::Apps, SidecarEvent::Event(ConnectorEvent::App { app, event }))
}

/// Resolve's render `render_job` starting, as the apps sidecar reports it.
fn render_started(render_job: &str) -> Input {
    render(AppEvent::RenderStarted { render_job: render_job.to_owned() })
}

/// The render job a step opened.
fn opened(h: &mut Harness, render_job: &str) -> JobId {
    let effects = h.step(render_started(render_job));
    accepted_all(&effects).first().map(|(job, _, _)| *job).expect("an app task")
}

#[test]
fn a_render_is_an_app_task_with_progress_and_a_spoken_end() {
    let mut h = Harness::new();
    let (coding, _) = start(&mut h, "coding", "the parser fix");
    let effects = h.step(render_started("r1"));
    let accepted = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::JobAccepted { job, name, kind, .. } = e {
            Some((*job, name.clone(), *kind))
        } else {
            None
        }
    });
    let (job, name, kind) = accepted.expect("an app task");
    assert_eq!((name.as_str(), kind), ("the render", JobKind::AppTask), "named the render");
    assert!(starts(&effects).is_empty(), "no Claude session");
    assert_eq!(row(&h, job).status, JobStatus::Running, "running at once");
    assert!(row(&h, coding).focus, "an app's own render does not take the focus");
    let again = h.step(render_started("r1"));
    assert!(accepted_all(&again).is_empty(), "one job per render");
    let effects = h.step(render(AppEvent::RenderProgress { render_job: "r1".to_owned(), pct: 60 }));
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::JobProgressed { job: j, pct: Some(60), .. } if *j == job
        )),
        "progress with its percent: {effects:?}"
    );
    assert_eq!(row(&h, job).progress_pct, Some(60), "on its row");
    let stray = h.step(render(AppEvent::RenderProgress { render_job: "r9".to_owned(), pct: 5 }));
    assert!(events(&stray).is_empty(), "a render no job tracks");
    let effects = h.step(render(AppEvent::RenderDone { render_job: "r1".to_owned(), ok: true }));
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCompleted { job: j, .. } if *j == job)),
        "completed"
    );
    assert_eq!(announcements(&mut h), ["The render is done."], "announced, unattributed");
}

#[test]
fn a_render_the_app_reports_failed_is_announced_as_failed() {
    let mut h = Harness::new();
    let job = opened(&mut h, "r1");
    let report = render(AppEvent::RenderDone { render_job: "r1".to_owned(), ok: false });
    let effects = h.step(report);
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobFailed { job: j, .. } if *j == job)),
        "failed: {effects:?}"
    );
    assert_eq!(
        announcements(&mut h),
        ["The render failed: resolve reported that the render failed."],
        "announced with the app's own word for it"
    );
}

#[test]
fn cancelling_a_render_stops_tracking_it_and_says_the_app_goes_on() {
    let mut h = Harness::new();
    h.step(render_started("r1"));
    let effects = h.say("cancel the render");
    assert!(
        events(&effects).iter().any(|e| matches!(e, BusEvent::JobCancelled { .. })),
        "{effects:?}"
    );
    assert_eq!(replies(&effects), [APP_LEFT], "Eileen cannot stop the app's own render");
    let late = h.step(render(AppEvent::RenderDone { render_job: "r1".to_owned(), ok: true }));
    assert!(events(&late).is_empty(), "one terminal state only");
}

#[test]
fn cancelling_a_render_through_the_brain_says_the_app_goes_on_too() {
    let mut h = Harness::new();
    let job = opened(&mut h, "r1");
    let turn = ask_brain(&mut h, "Eileen, stop following the render");
    let args = json!({ "job": { "name": "the render" }, "action": "cancel" });
    let effects = h.step(control(turn, args));
    assert_eq!(tool_reply(&effects), Some((false, APP_LEFT.to_owned())), "never that it stopped");
    assert_eq!(row(&h, job).status, JobStatus::Cancelled, "the tracking ends");
}

#[test]
fn the_focus_and_a_route_word_pass_over_an_apps_own_render() {
    let mut h = Harness::new();
    let (research, _) = start(&mut h, "research", "the price check");
    let (_, report) = start(&mut h, "general", "the report");
    let render_job = opened(&mut h, "r1");
    h.step(Input::Worker(report, init()));
    h.step(Input::Worker(report, finished()));
    assert!(row(&h, research).focus, "the focus passes over the newer render");
    assert!(!row(&h, render_job).focus, "which the user did not ask for");
    let effects = h.say("cancel claude's job");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCancelled { job, .. } if *job == research)),
        "Claude's job is the price check: {effects:?}"
    );
    assert_eq!(row(&h, render_job).status, JobStatus::Running, "never the render");
    assert!(row(&h, render_job).focus, "which takes the focus once no other job is open");
}

#[test]
fn renders_tracked_when_the_apps_sidecar_exits_end_lost_and_the_focus_moves_on() {
    let mut h = Harness::new();
    let (research, _) = start(&mut h, "research", "the price check");
    let job = opened(&mut h, "r1");
    let turn = ask_brain(&mut h, "Eileen, follow the render");
    h.step(control(turn, json!({ "job": { "name": "the render" }, "action": "focus" })));
    h.step(done(turn));
    assert!(row(&h, job).focus, "the render followed");
    let exited = Input::Sidecar(SidecarId::Apps, SidecarEvent::Exited { code: Some(1) });
    let effects = h.step(exited);
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobFailed { job: j, .. } if *j == job)),
        "the render's job ends: {effects:?}"
    );
    assert_eq!(row(&h, job).status, JobStatus::Failed, "in one terminal state");
    assert!(row(&h, research).focus, "the focus moves on");
    assert_eq!(
        announcements(&mut h),
        ["I lost track of the render when the app's bridge stopped."],
        "said as lost, never as failed"
    );
    let late = h.step(render(AppEvent::RenderDone { render_job: "r1".to_owned(), ok: true }));
    assert!(events(&late).is_empty(), "a late end changes nothing");
    let again = opened(&mut h, "r1");
    assert_ne!(again, job, "a render still running once the sidecar is back opens a new job");
}

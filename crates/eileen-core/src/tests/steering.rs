//! Many jobs through `Core::step` (BUILD.md section 2.5): a fan-out and its children, the
//! focus job and how it moves, which job spoken words name, steer reaching the bound session,
//! reroute, the router, and an app task with its progress.

use eileen_contracts::bus::{BusEvent, CancelSource, SpeakCause};
use eileen_contracts::config::KindRouting;
use eileen_contracts::connectors::{AppEvent, ConnectorEvent, SidecarEvent, SidecarId};
use eileen_contracts::core_io::{Effect, Input, ToolCallRequest};
use eileen_contracts::harness::{SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::{JobId, ToolName, TurnId, Untrusted, WorkerSessionId};
use eileen_contracts::jobs::{Job, JobKind, JobStatus};
use eileen_contracts::tools::brain::JobsDispatchOutput;
use serde_json::{Value, json};

use super::jobs::{BRAIN, addressed, dispatch_from, finished, init, tool_reply};
use super::{Harness, ask_brain, done, events};
use crate::CoreConfig;

/// A `jobs_control` call from the brain's shim, for `turn`.
fn control(turn: TurnId, args: Value) -> Input {
    Input::ToolCall(ToolCallRequest {
        caller: BRAIN,
        origin: addressed(turn),
        tool: ToolName::new("jobs_control").unwrap(),
        args: Untrusted::new(args),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    })
}

/// The jobs a step accepted: id, name and ordinal.
fn accepted_all(effects: &[Effect]) -> Vec<(JobId, String, Option<u8>)> {
    events(effects)
        .into_iter()
        .filter_map(|e| {
            if let BusEvent::JobAccepted { job, name, ordinal, .. } = e {
                Some((*job, name.clone(), *ordinal))
            } else {
                None
            }
        })
        .collect()
}

/// The sessions a step started, with their specs.
fn starts(effects: &[Effect]) -> Vec<(WorkerSessionId, SessionSpec)> {
    effects
        .iter()
        .filter_map(|e| {
            if let Effect::Worker(session, WorkerCmd::Start(spec)) = e {
                Some((*session, (**spec).clone()))
            } else {
                None
            }
        })
        .collect()
}

/// The commands a step sent to worker sessions.
fn commands(effects: &[Effect]) -> Vec<(WorkerSessionId, WorkerCmd)> {
    effects
        .iter()
        .filter_map(|e| if let Effect::Worker(s, cmd) = e { Some((*s, cmd.clone())) } else { None })
        .collect()
}

/// The structured output of a successful `jobs_dispatch`.
fn dispatched(effects: &[Effect]) -> JobsDispatchOutput {
    let output = effects.iter().find_map(|e| {
        if let Effect::ToolReply { result, .. } = e { result.output.clone() } else { None }
    });
    serde_json::from_value(output.expect("an output").get().clone()).expect("its schema")
}

/// The job `id` as the core keeps it.
fn row(h: &Harness, id: JobId) -> Job {
    h.core.snapshot().jobs.into_iter().find(|job| job.id == id).expect("kept")
}

/// What Eileen replied aloud in a step.
fn replies(effects: &[Effect]) -> Vec<String> {
    events(effects)
        .into_iter()
        .filter_map(|e| {
            if let BusEvent::Speaking { cause: SpeakCause::Reply, text, .. } = e {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Everything waiting to be announced, in order.
fn announcements(h: &mut Harness) -> Vec<String> {
    std::iter::from_fn(|| h.core.announcements.pop()).map(|a| a.text).collect()
}

fn job_args(kind: &str, title: &str) -> Value {
    json!({ "kind": kind, "task": format!("do {title}"), "title_hint": title })
}

/// Dispatches one job of `kind` named `title` in a new brain turn; returns it and its session.
fn start(h: &mut Harness, kind: &str, title: &str) -> (JobId, WorkerSessionId) {
    let turn = ask_brain(h, "Eileen, start a job");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), job_args(kind, title)));
    h.step(done(turn));
    let (job, _, _) = accepted_all(&effects).first().cloned().expect("accepted");
    let (session, _) = starts(&effects).first().cloned().expect("a session");
    (job, session)
}

fn flaky(fanout: u8) -> Value {
    json!({
        "kind": "coding",
        "task": "fix the flaky login tests",
        "title_hint": "flaky login tests",
        "fanout": fanout
    })
}

#[test]
fn a_fan_out_accepts_a_parent_and_one_child_per_worker_each_with_its_own_session() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, launch three agents to fix the flaky login tests");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), flaky(3)));
    let jobs = accepted_all(&effects);
    let names: Vec<(&str, Option<u8>)> =
        jobs.iter().map(|(_, name, ordinal)| (name.as_str(), *ordinal)).collect();
    assert_eq!(
        names,
        [
            ("flaky login tests", None),
            ("agent one", Some(1)),
            ("agent two", Some(2)),
            ("agent three", Some(3))
        ],
        "the parent, then its children in accept order"
    );
    let sessions = starts(&effects);
    assert_eq!(sessions.len(), 3, "one cold session per worker, none for the parent");
    let folders: std::collections::BTreeSet<&str> =
        sessions.iter().map(|(_, spec)| spec.cwd.as_str()).collect();
    assert_eq!(folders.len(), 3, "each worker in a folder of its own");
    let output = dispatched(&effects);
    assert_eq!(output.ack, "Starting three Claude workers on flaky login tests.", "the ack");
    assert_eq!(output.jobs.len(), 4, "the parent and its children");
    let parent = row(&h, jobs[0].0);
    assert_eq!((parent.session, parent.spec.fanout), (None, 3), "the parent has no session");
    assert!(parent.focus, "the fan-out takes the focus");
    for (child, _, _) in &jobs[1..] {
        let child = row(&h, *child);
        assert_eq!(child.spec.parent, Some(parent.id), "{}: points at its parent", child.name);
        assert_eq!(child.spec.task, "fix the flaky login tests", "compete: the whole task");
        assert!(!child.focus, "{}: not the focus", child.name);
        assert_eq!(child.slug, format!("{}-{}", parent.slug, child.ordinal.unwrap()), "slug");
    }
}

#[test]
fn split_mode_gives_each_worker_its_own_sub_task_and_the_parent_runs_with_the_first() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, fix the parser and the lexer at once");
    let args = json!({
        "kind": "coding",
        "task": "fix the parser and the lexer",
        "title_hint": "the front end",
        "fanout": 2,
        "mode": "split",
        "subtasks": ["fix the parser", "fix the lexer"]
    });
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
    let parent = accepted_all(&effects)[0].0;
    let sessions = starts(&effects);
    let effects = h.step(Input::Worker(sessions[1].0, init()));
    let bound = commands(&effects).into_iter().find_map(|(_, cmd)| {
        if let WorkerCmd::Bind(binding) = cmd { Some(binding.task) } else { None }
    });
    assert_eq!(bound.as_deref(), Some("fix the lexer"), "agent two's own sub-task");
    let started: Vec<JobId> = events(&effects)
        .into_iter()
        .filter_map(|e| if let BusEvent::JobStarted { job, .. } = e { Some(*job) } else { None })
        .collect();
    assert!(started.contains(&parent), "the parent runs with its first child: {effects:?}");
    assert_eq!(row(&h, parent).status, JobStatus::Running, "its row");
    let again = h.step(Input::Worker(sessions[0].0, init()));
    assert!(
        !events(&again)
            .iter()
            .any(|e| matches!(e, BusEvent::JobStarted { job, .. } if *job == parent)),
        "and starts once"
    );
}

#[test]
fn a_fan_out_ends_once_after_its_last_child_with_one_announcement() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, launch three agents to fix the flaky login tests");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), flaky(3)));
    h.step(done(turn));
    let parent = accepted_all(&effects)[0].0;
    let sessions: Vec<WorkerSessionId> = starts(&effects).into_iter().map(|(s, _)| s).collect();
    for session in &sessions {
        h.step(Input::Worker(*session, init()));
    }
    h.step(Input::Worker(sessions[0], finished()));
    h.step(Input::Worker(sessions[2], WorkerEvent::Exited { code: Some(1) }));
    assert_eq!(row(&h, parent).status, JobStatus::Running, "one child still runs");
    let effects = h.step(Input::Worker(sessions[1], finished()));
    let ends: Vec<&BusEvent> = events(&effects)
        .into_iter()
        .filter(|e| matches!(e, BusEvent::JobCompleted { job, .. } if *job == parent))
        .collect();
    assert_eq!(ends.len(), 1, "the parent completes once: {effects:?}");
    assert!(
        matches!(ends[0], BusEvent::JobCompleted { summary, .. } if summary.get() == "2 of 3 agents finished."),
        "{ends:?}"
    );
    assert_eq!(
        announcements(&mut h),
        [
            "Agent three failed: the worker stopped before it finished.",
            "Flaky login tests is done: 2 of 3 agents finished."
        ],
        "a child's failure, then the whole fan-out; no child's completion on its own"
    );
}

#[test]
fn cancelling_a_fan_out_cancels_each_running_child_once() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, launch two agents to fix the flaky login tests");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), flaky(2)));
    h.step(done(turn));
    let parent = accepted_all(&effects)[0].0;
    let effects = h.say("cancel");
    let cancelled: Vec<(JobId, CancelSource)> = events(&effects)
        .into_iter()
        .filter_map(|e| {
            if let BusEvent::JobCancelled { job, by } = e { Some((*job, *by)) } else { None }
        })
        .collect();
    assert_eq!(cancelled.len(), 3, "the parent and its two children: {effects:?}");
    assert_eq!(cancelled[0], (parent, CancelSource::Voice), "the focus job, by voice");
    assert!(cancelled[1..].iter().all(|(_, by)| *by == CancelSource::Parent), "by their parent");
    let killed = commands(&effects).iter().filter(|(_, c)| *c == WorkerCmd::Cancel).count();
    assert_eq!(killed, 2, "both sessions end");
    assert!(announcements(&mut h).is_empty(), "nothing the user cancelled is announced");
}

#[test]
fn the_focus_moves_to_a_new_job_to_a_job_named_and_off_a_finished_one() {
    let mut h = Harness::new();
    let (first, first_session) = start(&mut h, "general", "the report");
    assert!(row(&h, first).focus, "a new job takes the focus");
    let (second, _) = start(&mut h, "research", "the price check");
    assert!(row(&h, second).focus && !row(&h, first).focus, "the newer job takes it");
    let turn = ask_brain(&mut h, "Eileen, let me follow the report");
    let effects =
        h.step(control(turn, json!({ "job": { "name": "the report" }, "action": "focus" })));
    assert_eq!(tool_reply(&effects).map(|(e, _)| e), Some(false), "{effects:?}");
    assert!(row(&h, first).focus && !row(&h, second).focus, "focus on the job named");
    h.step(Input::Worker(first_session, init()));
    h.step(Input::Worker(first_session, finished()));
    assert!(!row(&h, first).focus, "a finished job gives the focus up");
    assert!(row(&h, second).focus, "to the newest unfinished job");
    let focused: Vec<bool> = h.core.snapshot().jobs.iter().map(|job| job.focus).collect();
    assert_eq!(focused.iter().filter(|f| **f).count(), 1, "one focus job: {focused:?}");
}

#[test]
fn a_steer_interrupts_the_bound_session_and_gives_it_the_words_as_a_new_turn() {
    let mut h = Harness::new();
    let (job, session) = start(&mut h, "coding", "the test agent");
    let turn = ask_brain(&mut h, "Eileen, tell the test agent to only look at the parser");
    let early = json!({
        "job": { "name": "the test agent" },
        "action": { "steer": { "text": "only look at the parser" } }
    });
    let effects = h.step(control(turn, early));
    assert!(commands(&effects).is_empty(), "not bound yet: nothing to interrupt");
    let effects = h.step(Input::Worker(session, init()));
    let bound = commands(&effects).into_iter().find_map(|(_, cmd)| {
        if let WorkerCmd::Bind(binding) = cmd { Some(binding.task) } else { None }
    });
    assert_eq!(
        bound.as_deref(),
        Some("do the test agent\n\nThe user added: only look at the parser"),
        "the words go with the task"
    );
    let turn = ask_brain(&mut h, "Eileen, tell it to skip the slow tests");
    let late = json!({
        "job": { "name": "it" },
        "action": { "steer": { "text": "skip the slow tests" } }
    });
    let effects = h.step(control(turn, late));
    assert_eq!(tool_reply(&effects), Some((false, "Told the test agent.".to_owned())), "said");
    assert_eq!(
        commands(&effects),
        [
            (session, WorkerCmd::Interrupt),
            (session, WorkerCmd::Prompt { text: "skip the slow tests".to_owned() })
        ],
        "an interrupt, then the words as a new user turn, to the bound session"
    );
    let interrupted = WorkerEvent::TurnDone {
        cost: eileen_contracts::jobs::Cost::default(),
        is_error: true,
        summary: Untrusted::new(String::new()),
    };
    let effects = h.step(Input::Worker(session, interrupted));
    assert!(events(&effects).is_empty(), "the interrupted turn does not end the job");
    let effects = h.step(Input::Worker(session, finished()));
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCompleted { job: j, .. } if *j == job)),
        "the steered turn does: {effects:?}"
    );
}

#[test]
fn a_reroute_starts_the_same_task_on_a_stronger_model_in_the_same_folder_and_cancels_the_old() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), super::jobs::hello()));
    h.step(done(turn));
    let (old, _, _) = accepted_all(&effects)[0].clone();
    let (old_session, old_spec) = starts(&effects)[0].clone();
    assert_eq!(row(&h, old).model, "mid", "considered depth by default");
    h.step(Input::Worker(old_session, init()));
    let words = "Eileen, send this one to a stronger model instead";
    let turn = ask_brain(&mut h, words);
    let args =
        json!({ "job": { "name": "this one" }, "action": { "reroute": { "depth": "deep" } } });
    let effects = h.step(control(turn, args));
    let (is_error, text) = tool_reply(&effects).expect("a reply");
    assert!(!is_error, "{text}");
    let new = accepted_all(&effects);
    assert_eq!(new.len(), 1, "one new job");
    let (new, name, _) = new[0].clone();
    assert_eq!(name, "the hello file", "the name the user knows");
    let reason = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::JobAccepted { reason, .. } = e { Some(reason.clone()) } else { None }
    });
    assert_eq!(
        reason.as_deref(),
        Some("rerouted from hello-file: asked for deep depth"),
        "the router's reason"
    );
    let (_, spec) = starts(&effects)[0].clone();
    assert_eq!(spec.cwd, old_spec.cwd, "the same working folder");
    assert_eq!(spec.model, "strong", "the strongest model");
    let row_new = row(&h, new);
    assert_eq!(row_new.spec.task, "create hello.txt containing hi", "the same task");
    assert_eq!(
        row_new.override_words.as_ref().map(|w| w.get().as_str()),
        Some(words),
        "recorded with the words that caused it"
    );
    assert!(row_new.focus, "the new job takes the focus");
    assert!(
        events(&effects).iter().any(
            |e| matches!(e, BusEvent::JobCancelled { job, by: CancelSource::Voice } if *job == old)
        ),
        "the old job is cancelled"
    );
    assert!(commands(&effects).contains(&(old_session, WorkerCmd::Cancel)), "and its session ends");
    assert_eq!(row(&h, old).status, JobStatus::Cancelled, "in one terminal state");
    let turn = ask_brain(&mut h, "Eileen, send it to Codex");
    let args = json!({ "job": { "name": "it" }, "action": { "reroute": { "route": "codex" } } });
    let (is_error, text) = tool_reply(&h.step(control(turn, args))).unwrap();
    assert!(is_error && text == "Only Claude can take jobs in this version.", "{text}");
}

#[test]
fn the_router_takes_an_override_then_the_policy_then_the_depths_model() {
    let mut config = CoreConfig::default();
    let research = KindRouting { model: Some("strong".to_owned()), ..KindRouting::default() };
    config.config.routing.kinds.insert(JobKind::Research, research);
    let mut h = Harness::with(config, Vec::new());
    let mut route = |args: Value| {
        let turn = ask_brain(&mut h, "Eileen, use the fast model for this");
        let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
        h.step(done(turn));
        let accepted = events(&effects).into_iter().find_map(|e| {
            if let BusEvent::JobAccepted { job, model, reason, .. } = e {
                Some((*job, model.clone(), reason.clone()))
            } else {
                None
            }
        });
        let (job, model, reason) = accepted.expect("accepted");
        (model, reason, row(&h, job).override_words.map(|w| w.get().clone()))
    };
    let policy = route(job_args("research", "the price check"));
    let wanted = ("strong".to_owned(), "the routing policy for research".to_owned(), None);
    assert_eq!(policy, wanted, "the policy for the kind, with no override words");
    let mut asked = job_args("research", "the price check");
    asked["model"] = json!("fast");
    let (model, reason, words) = route(asked);
    assert_eq!((model.as_str(), reason.as_str()), ("fast", "asked for the model fast"), "override");
    assert_eq!(words.as_deref(), Some("Eileen, use the fast model for this"), "with the words");
    let mut quick = job_args("general", "the report");
    quick["depth"] = json!("quick");
    assert_eq!(route(quick).0, "fast", "quick uses the fast model");
    let default = route(job_args("general", "the report"));
    assert_eq!(default.0, "mid", "considered uses the mid model");
    assert_eq!(default.1, "the default for considered depth", "and says so");
}

#[test]
fn spoken_words_name_a_job_by_kind_ask_between_two_and_say_when_none_fits() {
    let mut h = Harness::new();
    let (research, _) = start(&mut h, "research", "the price check");
    let (report, _) = start(&mut h, "general", "the report");
    let effects = h.say("cancel the research");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCancelled { job, .. } if *job == research)),
        "the research job, by its kind word: {effects:?}"
    );
    assert_eq!(row(&h, report).status, JobStatus::Queued, "the other job goes on");
    assert_eq!(replies(&h.say("cancel the other file")), ["I can't find that job."], "none fits");
    let (one, _) = start(&mut h, "research", "the flight search");
    let (two, _) = start(&mut h, "research", "the hotel search");
    let turn = ask_brain(&mut h, "Eileen, follow the report");
    h.step(control(turn, json!({ "job": { "name": "the report" }, "action": "focus" })));
    let effects = h.say("cancel the research");
    assert_eq!(
        replies(&effects),
        ["Do you mean the hotel search or the flight search?"],
        "one question for two candidates, newest first"
    );
    assert!(
        !events(&effects).iter().any(|e| matches!(e, BusEvent::JobCancelled { .. })),
        "nothing cancelled on a guess"
    );
    let turn = ask_brain(&mut h, "Eileen, follow the flight search");
    h.step(control(turn, json!({ "job": { "name": "the flight search" }, "action": "focus" })));
    let effects = h.say("cancel the research");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCancelled { job, .. } if *job == one)),
        "the focus job among them: {effects:?}"
    );
    let effects = h.say("cancel claude's job");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCancelled { job, .. } if *job == two)),
        "a route word: the focus moved to the newest, the hotel search: {effects:?}"
    );
}

/// The apps sidecar's report of Resolve's render `render_job`.
fn render(event: AppEvent) -> Input {
    let app = "resolve".to_owned();
    Input::Sidecar(SidecarId::Apps, SidecarEvent::Event(ConnectorEvent::App { app, event }))
}

#[test]
fn a_render_is_an_app_task_with_progress_and_a_spoken_end() {
    let mut h = Harness::new();
    let (coding, _) = start(&mut h, "coding", "the parser fix");
    let effects = h.step(render(AppEvent::RenderStarted { render_job: "r1".to_owned() }));
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
    let again = h.step(render(AppEvent::RenderStarted { render_job: "r1".to_owned() }));
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
fn cancelling_a_render_stops_tracking_it_and_says_the_app_goes_on() {
    let mut h = Harness::new();
    h.step(render(AppEvent::RenderStarted { render_job: "r1".to_owned() }));
    let effects = h.say("cancel the render");
    assert!(
        events(&effects).iter().any(|e| matches!(e, BusEvent::JobCancelled { .. })),
        "{effects:?}"
    );
    assert_eq!(
        replies(&effects),
        ["I stopped tracking that; the app is still running it."],
        "Eileen cannot stop the app's own render"
    );
    let late = h.step(render(AppEvent::RenderDone { render_job: "r1".to_owned(), ok: true }));
    assert!(events(&late).is_empty(), "one terminal state only");
}

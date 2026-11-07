//! Changing jobs through `Core::step` (BUILD.md section 2.5): the focus job and how it moves,
//! which job spoken words name, a steer reaching the bound session or waiting for the bind,
//! and a reroute, with what it carries over and the bound on live workers it keeps. Fan-outs
//! are in `fan_out`, the router in `router`, app tasks in `apps`, the refusals in `refusals`;
//! the helpers here serve them all.

use eileen_contracts::bus::{BusEvent, CancelSource, SpeakCause};
use eileen_contracts::core_io::{Effect, Input, ToolCallRequest};
use eileen_contracts::harness::{SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::{JobId, ToolName, TurnId, Untrusted, WorkerSessionId};
use eileen_contracts::jobs::{Job, JobStatus};
use serde_json::{Value, json};

use super::jobs::{BRAIN, addressed, dispatch_from, finished, init, tool_reply};
use super::{Harness, ask_brain, done, events};

/// A `jobs_control` call from the brain's shim, for `turn`.
pub(super) fn control(turn: TurnId, args: Value) -> Input {
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
pub(super) fn accepted_all(effects: &[Effect]) -> Vec<(JobId, String, Option<u8>)> {
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
pub(super) fn starts(effects: &[Effect]) -> Vec<(WorkerSessionId, SessionSpec)> {
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
pub(super) fn commands(effects: &[Effect]) -> Vec<(WorkerSessionId, WorkerCmd)> {
    effects
        .iter()
        .filter_map(|e| if let Effect::Worker(s, cmd) = e { Some((*s, cmd.clone())) } else { None })
        .collect()
}

/// The task the step bound a session to, if it bound one.
pub(super) fn bound_task(effects: &[Effect]) -> Option<String> {
    commands(effects).into_iter().find_map(|(_, cmd)| {
        if let WorkerCmd::Bind(binding) = cmd { Some(binding.task) } else { None }
    })
}

/// The job `id` as the core keeps it.
pub(super) fn row(h: &Harness, id: JobId) -> Job {
    h.core.snapshot().jobs.into_iter().find(|job| job.id == id).expect("kept")
}

/// What Eileen replied aloud in a step.
pub(super) fn replies(effects: &[Effect]) -> Vec<String> {
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
pub(super) fn announcements(h: &mut Harness) -> Vec<String> {
    std::iter::from_fn(|| h.core.announcements.pop()).map(|a| a.text).collect()
}

pub(super) fn job_args(kind: &str, title: &str) -> Value {
    json!({ "kind": kind, "task": format!("do {title}"), "title_hint": title })
}

/// Dispatches one job of `kind` named `title` in a new brain turn; returns it and its session.
pub(super) fn start(h: &mut Harness, kind: &str, title: &str) -> (JobId, WorkerSessionId) {
    let turn = ask_brain(h, "Eileen, start a job");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), job_args(kind, title)));
    h.step(done(turn));
    let (job, _, _) = accepted_all(&effects).first().cloned().expect("accepted");
    let (session, _) = starts(&effects).first().cloned().expect("a session");
    (job, session)
}

/// A reroute of the job named `name` to deep depth, from a new brain turn; returns its effects.
fn reroute_to_deep(h: &mut Harness, name: &str) -> Vec<Effect> {
    let turn = ask_brain(h, "Eileen, send it to a stronger model");
    let args = json!({ "job": { "name": name }, "action": { "reroute": { "depth": "deep" } } });
    let effects = h.step(control(turn, args));
    h.step(done(turn));
    effects
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
    assert_eq!(
        bound_task(&effects).as_deref(),
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
fn a_steer_waiting_for_the_bind_cuts_a_long_task_and_never_the_words() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, sort out the long list");
    let long = "x".repeat(7_990);
    let args = json!({ "kind": "general", "task": long, "title_hint": "the long list" });
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
    h.step(done(turn));
    let (session, _) = starts(&effects).first().cloned().expect("a session");
    let turn = ask_brain(&mut h, "Eileen, tell the long list to skip the first row");
    let steer = json!({
        "job": { "name": "the long list" },
        "action": { "steer": { "text": "skip the first row" } }
    });
    h.step(control(turn, steer));
    let task = bound_task(&h.step(Input::Worker(session, init()))).expect("bound");
    assert!(task.ends_with("\n\nThe user added: skip the first row"), "the words whole: {task}");
    assert!(task.starts_with("xxx"), "after as much of the task as fits");
    assert_eq!(task.chars().count(), 8_000, "the whole within the task limit");
}

#[test]
fn a_steer_to_a_worker_eight_turns_behind_is_refused_with_a_sentence() {
    let mut h = Harness::new();
    let (_, session) = start(&mut h, "coding", "the test agent");
    h.step(Input::Worker(session, init()));
    let args = json!({
        "job": { "name": "the test agent" },
        "action": { "steer": { "text": "skip the slow tests" } }
    });
    let steer = |h: &mut Harness| {
        let turn = ask_brain(h, "Eileen, tell the test agent to skip the slow tests");
        let effects = h.step(control(turn, args.clone()));
        h.step(done(turn));
        effects
    };
    for n in 1..=7 {
        let effects = steer(&mut h);
        assert_eq!(commands(&effects).len(), 2, "steer {n}: an interrupt and the words");
    }
    let effects = steer(&mut h);
    assert!(commands(&effects).is_empty(), "the bound task and seven steers owed: {effects:?}");
    assert_eq!(
        tool_reply(&effects),
        Some((true, "the test agent is still catching up; tell it again in a moment.".to_owned())),
        "and Eileen says why rather than that she told it"
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
fn a_reroute_gives_the_new_worker_what_the_user_told_the_old_one() {
    let mut h = Harness::new();
    let (_, session) = start(&mut h, "coding", "the test agent");
    let tell = |h: &mut Harness, text: &str| {
        let turn = ask_brain(h, "Eileen, tell the test agent something");
        let args = json!({
            "job": { "name": "the test agent" },
            "action": { "steer": { "text": text } }
        });
        let effects = h.step(control(turn, args));
        h.step(done(turn));
        effects
    };
    tell(&mut h, "only look at the parser");
    let effects = reroute_to_deep(&mut h, "the test agent");
    let (new_session, _) = starts(&effects).first().cloned().expect("a new session");
    assert_ne!(new_session, session, "a session of its own");
    let task = bound_task(&h.step(Input::Worker(new_session, init()))).expect("bound");
    assert_eq!(
        task, "do the test agent\n\nThe user added: only look at the parser",
        "a word given before the old session was ready goes with the task"
    );
    let sent = tell(&mut h, "skip the slow tests");
    assert_eq!(commands(&sent).len(), 2, "this one reaches the bound session: {sent:?}");
    let effects = reroute_to_deep(&mut h, "the test agent");
    let (third, _) = starts(&effects).first().cloned().expect("a third session");
    let task = bound_task(&h.step(Input::Worker(third, init()))).expect("bound");
    assert_eq!(
        task, "do the test agent\n\nThe user added: only look at the parser\nskip the slow tests",
        "words already sent to a bound worker are given again, oldest first"
    );
}

#[test]
fn a_reroute_with_sixteen_workers_live_ends_the_old_session_before_it_starts_the_new() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, start sixteen reports");
    for n in 0..16 {
        let args = job_args("general", &format!("report {n}"));
        let (is_error, text) =
            tool_reply(&h.step(dispatch_from(BRAIN, addressed(turn), args))).expect("a reply");
        assert!(!is_error, "report {n} starts: {text}");
    }
    h.step(done(turn));
    let effects = reroute_to_deep(&mut h, "report 0");
    assert_eq!(tool_reply(&effects).map(|(e, _)| e), Some(false), "moved: {effects:?}");
    let mut live = 16_i32;
    let mut most = live;
    for effect in &effects {
        if matches!(effect, Effect::Worker(_, WorkerCmd::Start(_))) {
            live += 1;
        }
        if matches!(effect, Effect::Worker(_, WorkerCmd::Cancel)) {
            live -= 1;
        }
        most = most.max(live);
    }
    assert_eq!(most, 16, "never more than sixteen sessions, even for a moment: {effects:?}");
    assert_eq!(live, 16, "one ended and one started");
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

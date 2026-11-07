//! A fan-out through `Core::step` (BUILD.md section 2.5): its parent and one child per worker,
//! split mode, the one end after the last child, counted even when finished children were
//! dropped to make room, its cancel, and a steer that reaches only some of its workers.

use eileen_contracts::bus::{BusEvent, CancelSource};
use eileen_contracts::core_io::{Effect, Input, UserInput};
use eileen_contracts::harness::{WorkerCmd, WorkerEvent};
use eileen_contracts::ids::{JobId, WorkerSessionId};
use eileen_contracts::jobs::JobStatus;
use eileen_contracts::tools::brain::JobsDispatchOutput;
use serde_json::{Value, json};

use super::jobs::{BRAIN, addressed, dispatch_from, finished, init, tool_reply};
use super::steering::{
    accepted_all, announcements, bound_task, commands, control, row, start, starts,
};
use super::{Harness, ask_brain, done, events};

/// The structured output of a successful `jobs_dispatch`.
fn dispatched(effects: &[Effect]) -> JobsDispatchOutput {
    let output = effects.iter().find_map(|e| {
        if let Effect::ToolReply { result, .. } = e { result.output.clone() } else { None }
    });
    serde_json::from_value(output.expect("an output").get().clone()).expect("its schema")
}

fn flaky(fanout: u8) -> Value {
    json!({
        "kind": "coding",
        "task": "fix the flaky login tests",
        "title_hint": "flaky login tests",
        "fanout": fanout
    })
}

/// A fan-out of `fanout` agents on the flaky login tests, dispatched in a turn of its own;
/// returns its jobs, the parent first, and its sessions.
fn launch(h: &mut Harness, fanout: u8) -> (Vec<JobId>, Vec<WorkerSessionId>) {
    let turn = ask_brain(h, "Eileen, launch agents to fix the flaky login tests");
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), flaky(fanout)));
    h.step(done(turn));
    let jobs = accepted_all(&effects).into_iter().map(|(job, _, _)| job).collect();
    let sessions = starts(&effects).into_iter().map(|(session, _)| session).collect();
    (jobs, sessions)
}

/// The summary the parent `parent` completed with in a step, if it completed.
fn completed_with(effects: &[Effect], parent: JobId) -> Option<String> {
    events(effects).into_iter().find_map(|e| {
        if let BusEvent::JobCompleted { job, summary, .. } = e
            && *job == parent
        {
            Some(summary.get().clone())
        } else {
            None
        }
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
    assert_eq!(bound_task(&effects).as_deref(), Some("fix the lexer"), "agent two's own sub-task");
    let started = events(&effects)
        .into_iter()
        .any(|e| matches!(e, BusEvent::JobStarted { job, .. } if *job == parent));
    assert!(started, "the parent runs with its first child: {effects:?}");
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
    let (jobs, sessions) = launch(&mut h, 3);
    let parent = jobs[0];
    for session in &sessions {
        h.step(Input::Worker(*session, init()));
    }
    h.step(Input::Worker(sessions[0], finished()));
    h.step(Input::Worker(sessions[2], WorkerEvent::Exited { code: Some(1) }));
    assert_eq!(row(&h, parent).status, JobStatus::Running, "one child still runs");
    let effects = h.step(Input::Worker(sessions[1], finished()));
    let ends = events(&effects)
        .into_iter()
        .filter(|e| matches!(e, BusEvent::JobCompleted { job, .. } if *job == parent))
        .count();
    assert_eq!(ends, 1, "the parent completes once: {effects:?}");
    assert_eq!(
        completed_with(&effects, parent).as_deref(),
        Some("2 of 3 agents finished."),
        "Eileen's own count"
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
fn a_fan_out_counts_its_finished_agents_after_they_are_dropped_to_make_room() {
    let mut h = Harness::new();
    let (jobs, sessions) = launch(&mut h, 8);
    let parent = jobs[0];
    for session in &sessions {
        h.step(Input::Worker(*session, init()));
    }
    for session in &sessions[..7] {
        h.step(Input::Worker(*session, finished()));
    }
    for n in 0..30 {
        let (_, session) = start(&mut h, "general", &format!("report {n}"));
        h.step(Input::Worker(session, init()));
        h.step(Input::Worker(session, finished()));
    }
    let kept = h.core.snapshot().jobs;
    assert!(
        jobs[1..8].iter().all(|agent| kept.iter().all(|job| job.id != *agent)),
        "agents one to seven were dropped to make room"
    );
    let effects = h.step(Input::Worker(sessions[7], finished()));
    assert_eq!(
        completed_with(&effects, parent).as_deref(),
        Some("8 of 8 agents finished."),
        "every agent counts, kept or not: {effects:?}"
    );
}

#[test]
fn cancelling_a_fan_out_cancels_each_running_child_and_then_the_parent_once() {
    let mut h = Harness::new();
    let (jobs, _) = launch(&mut h, 2);
    let parent = jobs[0];
    let effects = h.say("cancel");
    let cancelled: Vec<(JobId, CancelSource)> = events(&effects)
        .into_iter()
        .filter_map(|e| {
            if let BusEvent::JobCancelled { job, by } = e { Some((*job, *by)) } else { None }
        })
        .collect();
    assert_eq!(cancelled.len(), 3, "the parent and its two children: {effects:?}");
    assert!(
        cancelled[..2].iter().all(|(job, by)| *job != parent && *by == CancelSource::Parent),
        "each agent first, by its parent: {cancelled:?}"
    );
    assert_eq!(cancelled[2], (parent, CancelSource::Voice), "then the focus job, by voice, once");
    let killed = commands(&effects).iter().filter(|(_, c)| *c == WorkerCmd::Cancel).count();
    assert_eq!(killed, 2, "both sessions end");
    assert!(announcements(&mut h).is_empty(), "nothing the user cancelled is announced");
}

#[test]
fn a_fan_out_ends_cancelled_by_whoever_cancelled_its_last_agent() {
    let mut h = Harness::new();
    let (jobs, _) = launch(&mut h, 2);
    let (parent, one, two) = (jobs[0], jobs[1], jobs[2]);
    h.step(Input::User(UserInput::CancelJob(one)));
    assert_eq!(row(&h, parent).status, JobStatus::Queued, "agent two is still open");
    let effects = h.step(Input::User(UserInput::CancelJob(two)));
    let by = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::JobCancelled { job, by } = e
            && *job == parent
        {
            Some(*by)
        } else {
            None
        }
    });
    assert_eq!(by, Some(CancelSource::Hud), "from the HUD, as its last agent was: {effects:?}");
    assert!(announcements(&mut h).is_empty(), "nothing the user cancelled is announced");
}

#[test]
fn a_steer_to_a_fan_out_names_the_agents_still_catching_up() {
    let mut h = Harness::new();
    let (_, sessions) = launch(&mut h, 2);
    for session in &sessions {
        h.step(Input::Worker(*session, init()));
    }
    let steer = |h: &mut Harness, name: &str| {
        let turn = ask_brain(h, "Eileen, tell them to skip the slow tests");
        let args = json!({
            "job": { "name": name },
            "action": { "steer": { "text": "skip the slow tests" } }
        });
        let effects = h.step(control(turn, args));
        h.step(done(turn));
        effects
    };
    for n in 1..=7 {
        let effects = steer(&mut h, "agent two");
        assert_eq!(commands(&effects).len(), 2, "steer {n} reaches agent two");
    }
    let effects = steer(&mut h, "flaky login tests");
    assert_eq!(
        tool_reply(&effects),
        Some((false, "Told flaky login tests; agent two is still catching up.".to_owned())),
        "Eileen names the agent the words did not reach"
    );
    let to: Vec<WorkerSessionId> = commands(&effects).into_iter().map(|(to, _)| to).collect();
    assert_eq!(to, [sessions[0], sessions[0]], "only agent one is interrupted and told");
}

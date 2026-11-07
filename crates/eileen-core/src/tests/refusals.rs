//! Every refusal of `jobs_dispatch` and `jobs_control` through `Core::step`, each with its
//! exact sentence, and none starting a session or writing a job row.

use eileen_contracts::connectors::{AppEvent, ConnectorEvent, SidecarEvent, SidecarId};
use eileen_contracts::core_io::{Caller, Effect, Input, Record, ShimRole, ToolCallRequest};
use eileen_contracts::ids::{ToolName, TurnId, Untrusted, WorkerSessionId};
use eileen_contracts::jobs::CallOrigin;
use serde_json::{Value, json};

use super::jobs::{BRAIN, addressed, dispatch_from, hello, init, tool_reply};
use super::steering::{control, job_args, start};
use super::{Harness, ask_brain, brain_turn, cli, done};

/// What a call from anything but the brain's own shim, for a brain turn, is told.
const CONVERSATION_ONLY: &str = "Only the conversation can start or change a job.";

/// What a fan-out of the wrong size is told.
const FANOUT: &str = "A fan-out takes one to eight agents.";

/// What arguments that do not parse are told.
const INVALID: &str = "That job request was not valid.";

/// What an empty steer is told.
const SAY_WHAT: &str = "Say what to tell it.";

/// What a reroute that asks for nothing is told.
const SAY_WHERE: &str = "Say which model or route to move it to.";

/// What a reroute of a job no worker runs is told.
const NO_WORKER: &str = "Only a job a worker runs can move to another model.";

/// What an action not built yet is told.
const NOT_YET: &str = "I can't do that to a job yet.";

/// What words that name no job are told.
const NOT_FOUND: &str = "I can't find that job.";

/// A worker's shim, which may neither start nor change a job.
const WORKER: Caller =
    Caller::Shim { connection: 2, role: ShimRole::Worker(WorkerSessionId::from_parts(7, 7)) };

/// Asserts the step refused with exactly `sentence`, started no session and wrote no row.
fn assert_refused(effects: &[Effect], sentence: &str, case: &str) {
    assert_eq!(tool_reply(effects), Some((true, sentence.to_owned())), "{case}: {effects:?}");
    let started = effects.iter().any(|e| matches!(e, Effect::Worker(_, _)));
    assert!(!started, "{case}: no session started or told: {effects:?}");
    let written = effects.iter().any(|e| matches!(e, Effect::Persist(Record::JobUpsert(_))));
    assert!(!written, "{case}: no job row written: {effects:?}");
}

/// `hello()` with `extra`'s fields over it.
fn hello_with(extra: &Value) -> Value {
    let mut args = hello();
    if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
        args.extend(extra.clone());
    }
    args
}

/// The apps sidecar's report that Resolve started the render `render_job`.
fn render_started(render_job: String) -> Input {
    let event = AppEvent::RenderStarted { render_job };
    let app = "resolve".to_owned();
    Input::Sidecar(SidecarId::Apps, SidecarEvent::Event(ConnectorEvent::App { app, event }))
}

#[test]
fn each_dispatch_refusal_has_its_sentence() {
    let cases = [
        ("a worker's shim", WORKER, hello(), CONVERSATION_ONLY),
        ("a fan-out of nine", BRAIN, hello_with(&json!({ "fanout": 9 })), FANOUT),
        ("a fan-out of none", BRAIN, hello_with(&json!({ "fanout": 0 })), FANOUT),
        (
            "too few sub-tasks",
            BRAIN,
            hello_with(&json!({ "fanout": 2, "mode": "split", "subtasks": ["a"] })),
            "Give one sub-task for each agent.",
        ),
        (
            "too many sub-tasks",
            BRAIN,
            hello_with(&json!({ "fanout": 2, "mode": "split", "subtasks": ["a", "b", "c"] })),
            "Give one sub-task for each agent.",
        ),
        (
            "a kind the core runs itself",
            BRAIN,
            hello_with(&json!({ "kind": "librarian" })),
            "I can't start a job of that kind.",
        ),
        (
            "a route not built",
            BRAIN,
            hello_with(&json!({ "route": "codex" })),
            "Only Claude can take jobs in this version.",
        ),
        ("arguments that do not parse", BRAIN, json!({ "kind": 3 }), INVALID),
    ];
    for (case, caller, args, sentence) in cases {
        let mut h = Harness::new();
        let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
        assert_refused(&h.step(dispatch_from(caller, addressed(turn), args)), sentence, case);
    }
}

#[test]
fn a_dispatch_past_sixteen_live_workers_is_refused_as_busy() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, start seventeen reports");
    for n in 0..16 {
        let args = job_args("general", &format!("report {n}"));
        let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
        assert_eq!(tool_reply(&effects).map(|(e, _)| e), Some(false), "report {n} starts");
    }
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), job_args("general", "report 16")));
    let busy = "Too many workers are running; I can start more when some finish.";
    assert_refused(&effects, busy, "a seventeenth worker");
}

#[test]
fn a_fan_out_that_does_not_fit_beside_the_open_jobs_is_refused_as_full() {
    let mut h = Harness::new();
    for n in 0..16 {
        h.step(render_started(format!("r{n}")));
    }
    let turn = ask_brain(&mut h, "Eileen, start eight reports and then eight agents");
    for n in 0..8 {
        let args = job_args("general", &format!("report {n}"));
        let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
        assert_eq!(tool_reply(&effects).map(|(e, _)| e), Some(false), "report {n} starts");
    }
    let fan_out = hello_with(&json!({ "fanout": 8 }));
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), fan_out));
    let full = "Too many jobs are open; cancel one and ask again.";
    assert_refused(&effects, full, "a parent and eight agents beside 24 open jobs");
}

#[test]
fn each_control_refusal_has_its_sentence() {
    let mut h = Harness::new();
    let (_, session) = start(&mut h, "coding", "the test agent");
    h.step(Input::Worker(session, init()));
    h.step(render_started("r1".to_owned()));
    let agent = json!({ "name": "the test agent" });
    let render = json!({ "name": "the render" });
    let cases = [
        ("an empty steer", agent.clone(), json!({ "steer": { "text": "  " } }), SAY_WHAT),
        ("a reroute to nowhere", agent.clone(), json!({ "reroute": {} }), SAY_WHERE),
        ("a render rerouted", render.clone(), json!({ "reroute": { "depth": "deep" } }), NO_WORKER),
        (
            "a render steered",
            render,
            json!({ "steer": { "text": "go faster" } }),
            "the render has no worker I can tell.",
        ),
        ("an interrupt", agent.clone(), json!("interrupt"), NOT_YET),
        ("show its changes", agent.clone(), json!("show_changes"), NOT_YET),
        ("keep its changes", agent.clone(), json!("keep"), NOT_YET),
        ("undo its changes", agent, json!("undo"), NOT_YET),
        ("a job no one named", json!({ "name": "the other file" }), json!("cancel"), NOT_FOUND),
    ];
    for (case, job, action, sentence) in cases {
        let turn = ask_brain(&mut h, "Eileen, change the test agent");
        let effects = h.step(control(turn, json!({ "job": job, "action": action })));
        h.step(done(turn));
        assert_refused(&effects, sentence, case);
    }
}

#[test]
fn jobs_control_from_an_ambient_turn_or_a_worker_is_refused() {
    let mut h = Harness::new();
    let (_, session) = start(&mut h, "coding", "the test agent");
    h.step(Input::Worker(session, init()));
    let cancel = json!({ "job": { "name": "the test agent" }, "action": "cancel" });
    let (ambient, _) = brain_turn(&h.step(cli("cancel the test agent"))).expect("an ambient turn");
    let effects = h.step(control(ambient, cancel.clone()));
    assert_refused(&effects, "Only something the user asked can change a job.", "ambient");
    let turn = ask_brain(&mut h, "Eileen, cancel the test agent");
    let from_worker = Input::ToolCall(ToolCallRequest {
        caller: WORKER,
        origin: addressed(turn),
        tool: ToolName::new("jobs_control").unwrap(),
        args: Untrusted::new(cancel.clone()),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    });
    assert_refused(&h.step(from_worker), CONVERSATION_ONLY, "a worker's shim");
    let origin = CallOrigin::Session;
    let from_session = Input::ToolCall(ToolCallRequest {
        caller: BRAIN,
        origin,
        tool: ToolName::new("jobs_control").unwrap(),
        args: Untrusted::new(cancel),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    });
    assert_refused(&h.step(from_session), CONVERSATION_ONLY, "a session's origin");
    let stale =
        control(TurnId::from_parts(99, 99), json!({ "job": { "name": "it" }, "action": "focus" }));
    assert_refused(&h.step(stale), "That request was cancelled or has ended.", "a turn never sent");
}

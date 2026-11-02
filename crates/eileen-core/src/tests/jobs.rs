//! The one job through `Core::step`: dispatch, its cold session, its hooks, its end and its
//! announcement; the refusal of a kind not built, a restart, slugs, the folder a coding job
//! works in, and cancel by name.

use eileen_contracts::bus::{BusEvent, CancelSource, SpeakCause, UserErrorKind};
use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use eileen_contracts::core_io::{
    Caller, Content, Effect, Input, PathAccess, PathClass, PathFact, PlaybackEvent, Record,
    ShimRole, TimerId, ToolCallRequest, UserInput,
};
use eileen_contracts::harness::{
    HookDecision, HookReply, HookRequest, PermissionDecision, SessionSpec, WorkerCmd, WorkerEvent,
};
use eileen_contracts::ids::{JobId, Personal, ToolName, TurnId, Untrusted, WorkerSessionId};
use eileen_contracts::jobs::{CallOrigin, Cost, Job, JobKind, JobStatus, Route, TurnAuthority};
use eileen_contracts::speech::VoiceEvent;
use serde_json::{Value, json};

use super::{Harness, ask_brain, brain_turn, done, events, ready, speech, spoken, timer_set};

const BRAIN: Caller = Caller::Shim { connection: 1, role: ShimRole::Brain };

fn hello() -> Value {
    json!({
        "kind": "general",
        "task": "create hello.txt containing hi",
        "title_hint": "the hello file"
    })
}

/// A `jobs_dispatch` call from `caller`, claiming `origin`.
fn dispatch_from(caller: Caller, origin: CallOrigin, args: Value) -> Input {
    Input::ToolCall(ToolCallRequest {
        caller,
        origin,
        tool: ToolName::new("jobs_dispatch").unwrap(),
        args: Untrusted::new(args),
        paths: Vec::new(),
        urls: Vec::new(),
        traceparent: None,
    })
}

fn addressed(turn: TurnId) -> CallOrigin {
    CallOrigin::Brain { turn, authority: TurnAuthority::Addressed }
}

fn dispatch(turn: TurnId) -> Input {
    dispatch_from(BRAIN, addressed(turn), hello())
}

fn tool_reply(effects: &[Effect]) -> Option<(bool, String)> {
    effects.iter().find_map(|e| {
        if let Effect::ToolReply { result, .. } = e {
            let text = result
                .content
                .iter()
                .filter_map(|c| if let Content::Text(t) = c { Some(t.get().clone()) } else { None })
                .collect::<String>();
            Some((result.is_error, text))
        } else {
            None
        }
    })
}

fn started_session(effects: &[Effect]) -> Option<WorkerSessionId> {
    effects.iter().find_map(|e| {
        if let Effect::Worker(session, WorkerCmd::Start(_)) = e { Some(*session) } else { None }
    })
}

fn path(class: PathClass) -> PathFact {
    PathFact {
        arg: "file_path".to_owned(),
        class,
        access: PathAccess::Write,
        canonical: Personal::new("C:\\scratch\\hello.txt".to_owned()),
        runs_code: false,
        opens_web: false,
        git_component: false,
    }
}

fn pre_tool_use(request: u64, tool: &str, paths: Vec<PathFact>) -> HookRequest {
    HookRequest::PreToolUse {
        request,
        tool_name: tool.to_owned(),
        tool_input: Untrusted::new(json!({})),
        tool_use_id: format!("toolu_{request}"),
        paths,
        git: Vec::new(),
        urls: Vec::new(),
    }
}

fn hook_decision(effects: &[Effect]) -> Option<HookDecision> {
    effects.iter().find_map(|e| {
        if let Effect::Hook(_, HookReply::PreToolUse { decision, .. }) = e {
            Some(decision.clone())
        } else {
            None
        }
    })
}

fn init() -> WorkerEvent {
    WorkerEvent::Init {
        session_id: "s1".to_owned(),
        pid: 42,
        model: "mid-model".to_owned(),
        tools: Vec::new(),
        mcp_servers: Vec::new(),
        plugins: Vec::new(),
        capabilities: Vec::new(),
    }
}

#[test]
fn one_job_runs_and_a_second_dispatch_is_refused_with_a_sentence() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let effects = h.step(dispatch(turn));
    let accepted = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::JobAccepted { job, name, .. } = e {
            Some((*job, name.clone()))
        } else {
            None
        }
    });
    let (job, name) = accepted.expect("accepted");
    assert_eq!(name, "the hello file", "named by the brain's hint");
    let session = started_session(&effects).expect("a cold session starts");
    let spec = effects.iter().find_map(|e| {
        if let Effect::Worker(_, WorkerCmd::Start(spec)) = e { Some(spec.clone()) } else { None }
    });
    let spec = spec.expect("its spec");
    assert_eq!(spec.builtin_tools, ["Read", "Grep", "Edit", "Write"], "the kind's file tools");
    assert_eq!(
        spec.allowed_tools,
        ["Read", "Grep", "Edit", "Write", "mcp__eileen__*"],
        "the same, with Eileen's own tools"
    );
    let (is_error, text) = tool_reply(&effects).unwrap();
    assert!(!is_error && text.contains(&job.to_string()), "the reply carries the job's id: {text}");
    let (is_error, text) = tool_reply(&h.step(dispatch(turn))).unwrap();
    assert!(is_error, "refused");
    assert_eq!(
        text, "the hello file is still running; I can start another when it finishes.",
        "said why"
    );
    let effects = h.step(Input::Worker(session, init()));
    assert!(
        effects.iter().any(
            |e| matches!(e, Effect::Worker(s, WorkerCmd::Bind(b)) if *s == session && b.job == job)
        ),
        "bound once the session is ready: {effects:?}"
    );
    assert!(
        events(&effects).iter().any(|e| matches!(e, BusEvent::JobStarted { warm: false, .. })),
        "started"
    );
}

#[test]
fn hooks_pass_only_file_tools_in_the_job_folder_and_never_allow() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).unwrap();
    let mut decide = |request, tool, paths| {
        hook_decision(&h.step(Input::Hook(session, pre_tool_use(request, tool, paths)))).unwrap()
    };
    assert_eq!(
        decide(1, "Write", vec![path(PathClass::JobFolder)]),
        HookDecision::None,
        "the job's own file"
    );
    assert_eq!(decide(2, "Read", vec![path(PathClass::JobTemp)]), HookDecision::None, "its temp");
    for (request, tool, paths) in [
        (3, "Write", vec![path(PathClass::Elsewhere)]),
        (4, "Write", vec![path(PathClass::JobFolder), path(PathClass::Credential)]),
        (5, "Bash", vec![path(PathClass::JobFolder)]),
        (6, "Write", Vec::new()),
        (9, "Write", vec![PathFact { git_component: true, ..path(PathClass::JobFolder) }]),
        (10, "MultiEdit", vec![path(PathClass::JobFolder)]),
        (11, "Glob", vec![path(PathClass::JobFolder)]),
    ] {
        let decision = decide(request, tool, paths);
        assert!(
            matches!(&decision, HookDecision::Deny { reason } if reason.contains("do not retry")),
            "request {request}: {decision:?}"
        );
    }
    let other = WorkerSessionId::from_parts(1, 1);
    let effects =
        h.step(Input::Hook(other, pre_tool_use(7, "Write", vec![path(PathClass::JobFolder)])));
    assert!(matches!(hook_decision(&effects), Some(HookDecision::Deny { .. })), "another session");
    let effects = h.step(Input::Hook(
        session,
        HookRequest::PermissionRequest {
            request: 8,
            tool_name: "Write".to_owned(),
            tool_input: Untrusted::new(json!({})),
            tool_use_id: Some("toolu_8".to_owned()),
        },
    ));
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Hook(
                _,
                HookReply::PermissionRequest { decision: PermissionDecision::Deny { .. }, .. }
            )
        )),
        "the tripwire denies"
    );
}

#[test]
fn a_completed_job_is_announced_once_after_a_second_of_quiet() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).unwrap();
    h.step(Input::Worker(session, init()));
    h.step(done(turn));
    let effects = h.step(Input::Worker(
        session,
        WorkerEvent::TurnDone {
            cost: Cost::default(),
            is_error: false,
            summary: Untrusted::new("Created hello.txt with hi. Checked it.".to_owned()),
        },
    ));
    assert!(events(&effects).iter().any(|e| matches!(e, BusEvent::JobCompleted { .. })), "done");
    assert!(effects.contains(&Effect::Worker(session, WorkerCmd::Cancel)), "the session ends");
    assert!(spoken(&effects).is_empty(), "the brain only just went quiet: {effects:?}");
    let (timer, at) = timer_set(&effects).expect("a second of quiet");
    h.now_ms = at.0 / 1_000_000;
    let effects = h.step(Input::Timer(timer));
    let line = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::Speaking { cause: SpeakCause::Announcement, text, .. } = e {
            Some(text.clone())
        } else {
            None
        }
    });
    assert_eq!(
        line.as_deref(),
        Some("The hello file is done. Claude says: Created hello.txt with hi."),
        "attributed: {effects:?}"
    );
    let (speak, clauses) = spoken(&effects)[0].clone();
    for clause in 0..clauses.len() {
        let clause = u16::try_from(clause).unwrap();
        h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause, chars: 10, samples: 100 }));
    }
    h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    h.wait(10_000);
    let effects = h.step(Input::Timer(TimerId(9_999)));
    assert!(spoken(&effects).is_empty(), "announced once");
    assert_eq!(
        h.core.snapshot().jobs.first().map(|job| job.status),
        Some(JobStatus::Completed),
        "the snapshot holds the job's terminal state"
    );
}

#[test]
fn cancel_ends_the_job_with_no_announcement() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).unwrap();
    h.step(done(turn));
    let effects = h.say("cancel");
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::JobCancelled { by: CancelSource::Voice, .. })),
        "{effects:?}"
    );
    assert!(effects.contains(&Effect::Worker(session, WorkerCmd::Cancel)), "the session is killed");
    h.wait(5_000);
    let effects = h.step(Input::Timer(TimerId(9_999)));
    assert!(spoken(&effects).is_empty(), "a job the user cancelled is not announced");
}

#[test]
fn a_session_with_no_live_job_takes_no_events() {
    let mut h = Harness::new();
    let stray = WorkerSessionId::from_parts(3, 3);
    assert!(h.step(Input::Worker(stray, init())).is_empty(), "no live job");
    let exited = WorkerEvent::Exited { code: Some(1) };
    assert!(h.step(Input::Worker(stray, exited)).is_empty(), "nothing");
}

/// Asserts the call was refused with a sentence, and nothing started.
fn assert_refused(effects: &[Effect], case: &str) -> String {
    let (is_error, text) = tool_reply(effects).unwrap_or_else(|| panic!("{case}: no reply"));
    assert!(is_error, "{case}: an error result: {text}");
    assert!(!text.is_empty(), "{case}: a sentence");
    assert!(
        !events(effects).iter().any(|e| matches!(e, BusEvent::JobAccepted { .. })),
        "{case}: no job: {effects:?}"
    );
    assert_eq!(started_session(effects), None, "{case}: no session");
    text
}

#[test]
fn a_dispatch_is_refused_with_a_sentence_unless_an_addressed_brain_turn_asked() {
    let session = WorkerSessionId::from_parts(7, 7);
    let worker = Caller::Shim { connection: 2, role: ShimRole::Worker(session) };
    let from_worker =
        CallOrigin::Worker { job: JobId::from_parts(7, 7), session, route: Route::Claude };
    let with = |extra: Value| {
        let mut args = hello();
        if let (Some(args), Some(extra)) = (args.as_object_mut(), extra.as_object()) {
            args.extend(extra.clone());
        }
        args
    };
    let unknown = addressed(TurnId::from_parts(99, 99));
    let cases = [
        ("a worker's shim with a brain origin", worker, None, hello()),
        ("a worker origin", BRAIN, Some(from_worker), hello()),
        ("the session origin", BRAIN, Some(CallOrigin::Session), hello()),
        ("a turn the core never sent", BRAIN, Some(unknown), hello()),
        ("a fanout of two", BRAIN, None, with(json!({ "fanout": 2 }))),
        ("a kind the core runs itself", BRAIN, None, with(json!({ "kind": "librarian" }))),
        ("the reserved kind", BRAIN, None, with(json!({ "kind": "ml_project" }))),
        ("a route not built", BRAIN, None, with(json!({ "route": "codex" }))),
        ("arguments that do not parse", BRAIN, None, json!({ "kind": 3 })),
    ];
    for (case, caller, origin, args) in cases {
        let mut h = Harness::new();
        let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
        let origin = origin.unwrap_or_else(|| addressed(turn));
        assert_refused(&h.step(dispatch_from(caller, origin, args)), case);
    }
}

#[test]
fn a_dispatch_from_an_ambient_turn_is_refused_whatever_it_claims() {
    let mut h = Harness::new();
    let cli = Input::User(UserInput::Cli {
        text: Personal::new("create a file called hello dot txt".to_owned()),
    });
    let (turn, _) = brain_turn(&h.step(cli)).expect("an ambient brain turn");
    let text = assert_refused(&h.step(dispatch(turn)), "claims addressed");
    assert_eq!(text, "A job can only start from something the user asked.", "said why");
}

#[test]
fn a_dispatch_after_stop_or_after_the_turn_ended_is_refused() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    h.say("stop");
    assert_refused(&h.step(dispatch(turn)), "the user stopped the turn");
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    h.step(done(turn));
    assert_refused(&h.step(dispatch(turn)), "the turn ended");
}

/// The job a step accepted, with its spoken name.
fn accepted(effects: &[Effect]) -> Option<(JobId, String)> {
    events(effects).into_iter().find_map(|e| {
        if let BusEvent::JobAccepted { job, name, .. } = e {
            Some((*job, name.clone()))
        } else {
            None
        }
    })
}

/// The spec of the session a step started.
fn start_spec(effects: &[Effect]) -> Option<SessionSpec> {
    effects.iter().find_map(|e| {
        if let Effect::Worker(_, WorkerCmd::Start(spec)) = e {
            Some((**spec).clone())
        } else {
            None
        }
    })
}

/// The job rows a step persisted.
fn persisted(effects: &[Effect]) -> Vec<Job> {
    effects
        .iter()
        .filter_map(|e| {
            if let Effect::Persist(Record::JobUpsert(job)) = e {
                Some((**job).clone())
            } else {
                None
            }
        })
        .collect()
}

fn finished() -> WorkerEvent {
    WorkerEvent::TurnDone {
        cost: Cost::default(),
        is_error: false,
        summary: Untrusted::new("Done.".to_owned()),
    }
}

const COLAB: &str = "train a model that finds a person's clothing colour on my Colab";

#[test]
fn an_ml_project_is_refused_before_routing_and_admission_and_research_is_accepted() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, train a model that finds clothing colour on my Colab");
    let ml = json!({ "kind": "ml_project", "task": COLAB });
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), ml));
    let result = effects.iter().find_map(|e| {
        if let Effect::ToolReply { result, .. } = e { Some(result.clone()) } else { None }
    });
    let result = result.expect("a reply");
    assert!(result.is_error, "returned as the call's error result");
    assert!(result.output.is_none(), "no output that does not match `JobsDispatchOutput`");
    let shown = events(&effects)
        .into_iter()
        .find_map(|e| if let BusEvent::UserError(error) = e { Some(error.clone()) } else { None });
    let error = shown.expect("the UserError on the bus, for the HUD");
    assert_eq!(error.kind, UserErrorKind::JobKindUnavailable, "its kind");
    assert!(
        error.sentence.contains("next version") && error.sentence.contains("research job"),
        "names the next version and offers research: {}",
        error.sentence
    );
    assert_eq!(tool_reply(&effects).map(|(_, text)| text), Some(error.sentence), "the same words");
    assert!(
        effects
            .iter()
            .all(|e| matches!(e, Effect::ToolReply { .. } | Effect::Emit(BusEvent::UserError(_)))),
        "no job, no session, no row and no admission: {effects:?}"
    );
    let research = json!({ "kind": "research", "task": COLAB });
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), research));
    let kind = events(&effects)
        .into_iter()
        .find_map(|e| if let BusEvent::JobAccepted { kind, .. } = e { Some(*kind) } else { None });
    assert_eq!(kind, Some(JobKind::Research), "the same task as research: {effects:?}");
    assert!(started_session(&effects).is_some(), "its session starts as usual");
}

#[test]
fn a_restart_marks_the_running_job_interrupted_and_repeats_no_id() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let effects = h.step(dispatch(turn));
    let (job, _) = accepted(&effects).expect("accepted");
    let session = started_session(&effects).expect("a session");
    h.step(Input::Worker(session, init()));
    let (mut h, first) = Harness::resumed(h.core.snapshot());
    assert!(
        events(&first)
            .iter()
            .any(|e| matches!(e, BusEvent::JobInterrupted { job: j } if *j == job)),
        "interrupted on the first step: {first:?}"
    );
    let statuses: Vec<JobStatus> = persisted(&first).iter().map(|row| row.status).collect();
    assert_eq!(statuses, [JobStatus::Interrupted], "its row");
    let kept: Vec<JobStatus> = h.core.snapshot().jobs.iter().map(|job| job.status).collect();
    assert_eq!(kept, [JobStatus::Interrupted], "the snapshot holds it interrupted");
    // The same requests at the same virtual times, with the random numbers starting again.
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let effects = h.step(dispatch(turn));
    let (again, _) = accepted(&effects).expect("a new job: the old one no longer runs");
    assert_ne!(again, job, "no job id repeats");
    assert_ne!(started_session(&effects), Some(session), "no session id repeats");
}

#[test]
fn a_second_job_of_the_same_name_gets_a_slug_of_its_own() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).expect("a session");
    h.step(Input::Worker(session, init()));
    h.step(done(turn));
    h.step(Input::Worker(session, finished()));
    let turn = ask_brain(&mut h, "Eileen, make the hello file again");
    let (_, name) = accepted(&h.step(dispatch(turn))).expect("accepted");
    assert_eq!(name, "the hello file", "the name the user knows");
    let slugs: Vec<String> = h.core.snapshot().jobs.iter().map(|job| job.slug.clone()).collect();
    assert_eq!(slugs, ["hello-file", "hello-file-2"], "slugs never collide");
}

/// FIXME(#5): an absolute repository works in place behind `refs/eileen/<job id>`, or in a
/// worktree, as `lifecycle::Registry::checkout` says, once the harness records that snapshot
/// before the session starts and creates worktrees. Until then every coding job works in its
/// own folder, and a name, a relative path or Eileen's own folder never becomes its folder.
#[test]
fn a_coding_job_works_in_its_own_folder_whatever_repository_the_brain_names() {
    for repo in ["D:\\code\\demo", "demo", "learning", "..\\..\\x"] {
        let mut h = Harness::new();
        let turn = ask_brain(&mut h, "Eileen, fix the parser test in the demo repo");
        let args = json!({
            "kind": "coding",
            "task": "fix the parser test",
            "title_hint": "the parser fix",
            "repo": repo
        });
        let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
        let (job, _) = accepted(&effects).expect("accepted");
        let session = started_session(&effects).expect("a session");
        let spec = start_spec(&effects).expect("its spec");
        assert_eq!(spec.cwd, format!("run/{session}/cwd"), "{repo}: its own folder");
        assert_eq!(spec.worktree, None, "{repo}: no worktree");
        assert_eq!(spec.snapshot_ref, None, "{repo}: no snapshot asked for");
        let row = h.core.snapshot().jobs.into_iter().find(|row| row.id == job).expect("its row");
        assert_eq!(row.snapshot_ref, None, "{repo}: no snapshot claimed");
        assert_eq!((row.worktree, row.branch), (None, None), "{repo}: no worktree, no branch");
    }
}

#[test]
fn speech_going_down_and_up_again_interrupts_no_job() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).expect("a session");
    h.step(Input::Worker(session, init()));
    let down = h.now_ms;
    let mut seen = h.step(speech(SidecarEvent::Exited { code: Some(1) }));
    let restart = seen.iter().find_map(|e| {
        if let Effect::SetTimer { id, at } = e
            && at.0 > down * 1_000_000
        {
            Some((*id, *at))
        } else {
            None
        }
    });
    let (id, at) = restart.expect("a restart after the backoff");
    h.now_ms = at.0 / 1_000_000;
    seen.extend(h.step(Input::Timer(id)));
    seen.extend(h.step(speech(ready())));
    let restarted = Effect::Sidecar(SidecarId::Speech, SidecarCmd::Restart);
    assert!(seen.contains(&restarted), "speech came back: {seen:?}");
    assert!(
        !events(&seen).iter().any(|e| matches!(e, BusEvent::JobInterrupted { .. })),
        "the job this core accepted goes on: {seen:?}"
    );
    let effects = h.step(Input::Worker(session, finished()));
    assert!(
        events(&effects).iter().any(|e| matches!(e, BusEvent::JobCompleted { .. })),
        "and its worker still ends it: {effects:?}"
    );
}

#[test]
fn cancel_takes_a_name_and_an_ordinal_names_only_a_fan_out_child() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, create a file called hello dot txt");
    let session = started_session(&h.step(dispatch(turn))).expect("a session");
    h.step(Input::Worker(session, init()));
    h.step(done(turn));
    for words in ["cancel agent one", "cancel the other file"] {
        let effects = h.say(words);
        assert!(
            !events(&effects).iter().any(|e| matches!(e, BusEvent::JobCancelled { .. })),
            "{words}: names no job: {effects:?}"
        );
    }
    let effects = h.say("cancel the hello file");
    let cancelled = events(&effects)
        .iter()
        .filter(|e| matches!(e, BusEvent::JobCancelled { by: CancelSource::Voice, .. }))
        .count();
    assert_eq!(cancelled, 1, "by its name, in any case: {effects:?}");
    for late in [finished(), WorkerEvent::Exited { code: Some(0) }] {
        let effects = h.step(Input::Worker(session, late));
        assert!(events(&effects).is_empty(), "one terminal state only: {effects:?}");
    }
}

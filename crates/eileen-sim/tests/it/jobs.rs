//! Many jobs in the simulator (BUILD.md section 6.3), the basis of `scenarios/jobs/`'s three
//! parallel jobs: three jobs dispatched in three turns run side by side, a steer reaches the
//! bound session of one, a spoken "cancel" names another, and each ends in exactly one
//! terminal state, with every invariant the world checks holding at each step.

use eileen_contracts::bus::{BusEvent, CancelSource, SpeakCause};
use eileen_contracts::core_io::{Effect, Input, UserInput};
use eileen_contracts::harness::WorkerCmd;
use eileen_contracts::ids::{JobId, Personal, TurnId, WorkerSessionId};
use eileen_contracts::speech::{TurnEndReason, VoiceEvent};
use eileen_sim::parts::{LOAD_MS, Process};
use eileen_sim::{BrainReply, Profile, World};
use serde_json::json;

/// How long each worker turn's work takes, so the three jobs overlap.
const WORK_MS: u64 = 30_000;

/// A world whose core took its first step and whose speech process is ready.
fn world() -> World {
    let mut world = World::new(Profile::default_profile().unwrap(), 1);
    step(&mut world, Input::User(UserInput::SetMute(false)));
    settle(&mut world, LOAD_MS);
    assert_eq!(world.speech().process(), Process::Up, "speech is ready");
    world
}

fn step(world: &mut World, input: Input) {
    let outcome = world.step(input);
    assert_eq!(outcome.violation, None, "{}", outcome.line);
}

/// Lets `ms` of virtual time pass, delivering what the parts answer.
fn settle(world: &mut World, ms: u64) {
    for outcome in world.settle(ms) {
        assert_eq!(outcome.violation, None, "{}", outcome.line);
    }
}

/// The user speaks `text` as turn `n`: speech starts, 400 ms later the turn ends.
fn say(world: &mut World, n: u64, text: &str) {
    let turn = TurnId::from_parts(world.now_ms(), u128::from(n));
    step(world, Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }));
    settle(world, 400);
    step(
        world,
        Input::Voice(VoiceEvent::EndOfTurn {
            turn,
            text: Personal::new(text.to_owned()),
            smart_turn_pct: 92,
            silence_ms: 250,
            asr_confidence_pct: 96,
            reason: TurnEndReason::SmartTurn,
            source: 0,
            level_db: -28,
        }),
    );
}

fn events(world: &World) -> Vec<BusEvent> {
    world.published().iter().map(|event| event.body.clone()).collect()
}

/// The worker session started for `job`.
fn session_of(world: &World, job: JobId) -> Option<WorkerSessionId> {
    world.effects().iter().find_map(|effect| {
        if let Effect::Worker(session, WorkerCmd::Start(spec)) = &effect.body
            && spec.trace.job == Some(job)
        {
            Some(*session)
        } else {
            None
        }
    })
}

/// How many terminal events `job` had.
fn ends(events: &[BusEvent], job: JobId) -> usize {
    let ended = |event: &&BusEvent| {
        matches!(
            event,
            BusEvent::JobCompleted { job: j, .. }
                | BusEvent::JobFailed { job: j, .. }
                | BusEvent::JobCancelled { job: j, .. }
                | BusEvent::JobInterrupted { job: j } if *j == job
        )
    };
    events.iter().filter(ended).count()
}

#[test]
fn three_parallel_jobs_run_side_by_side_and_each_ends_in_one_state() {
    let mut world = world();
    world.set_worker_ms(WORK_MS);
    let titles =
        [("general", "the report"), ("research", "the price check"), ("coding", "the parser fix")];
    for (n, (kind, title)) in [1_u64, 2, 3].into_iter().zip(titles) {
        let args = json!({ "kind": kind, "task": format!("do {title}"), "title_hint": title });
        world.script_brain(BrainReply::text(&["Starting it now. "]).dispatching_args(args));
        say(&mut world, n, &format!("Eileen, start {title}"));
        settle(&mut world, 3_000);
    }
    let all = events(&world);
    let accepted: Vec<(JobId, String)> = all
        .iter()
        .filter_map(|event| {
            if let BusEvent::JobAccepted { job, name, .. } = event {
                Some((*job, name.clone()))
            } else {
                None
            }
        })
        .collect();
    let names: Vec<&str> = accepted.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, ["the report", "the price check", "the parser fix"], "three jobs");
    let started = all.iter().filter(|e| matches!(e, BusEvent::JobStarted { .. })).count();
    assert_eq!(started, 3, "all three run at once");
    assert_eq!(world.eileen_use().workers, 3, "three live worker sessions");
    for (job, name) in &accepted {
        assert_eq!(ends(&all, *job), 0, "{name} still runs");
    }
    let (report, price, parser) = (accepted[0].0, accepted[1].0, accepted[2].0);

    let parser_session = session_of(&world, parser).expect("the parser fix's session");
    world.script_brain(BrainReply::text(&["I will tell it. "]).controlling(json!({
        "job": { "name": "the parser fix" },
        "action": { "steer": { "text": "only look at the parser" } }
    })));
    let from = world.effects().len();
    say(&mut world, 4, "Eileen, tell the parser fix to only look at the parser");
    settle(&mut world, 3_000);
    let sent: Vec<WorkerCmd> = world.effects()[from..]
        .iter()
        .filter_map(|effect| {
            if let Effect::Worker(session, cmd) = &effect.body
                && *session == parser_session
            {
                Some(cmd.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        sent,
        [WorkerCmd::Interrupt, WorkerCmd::Prompt { text: "only look at the parser".to_owned() }],
        "the steer reached the parser fix's bound session"
    );

    say(&mut world, 5, "Eileen, cancel the price check");
    settle(&mut world, 1_000);
    let all = events(&world);
    assert!(
        all.iter().any(|e| matches!(
            e,
            BusEvent::JobCancelled { job, by: CancelSource::Voice } if *job == price
        )),
        "the price check, by its name"
    );
    assert_eq!(ends(&all, report) + ends(&all, parser), 0, "the other two run on");

    settle(&mut world, WORK_MS + 15_000);
    let all = events(&world);
    for (job, name) in &accepted {
        assert_eq!(ends(&all, *job), 1, "{name} ends in exactly one terminal state");
    }
    for job in [report, parser] {
        let completed =
            all.iter().any(|e| matches!(e, BusEvent::JobCompleted { job: j, .. } if *j == job));
        assert!(completed, "{job} completed");
    }
    let announced: Vec<String> = all
        .iter()
        .filter_map(|event| {
            if let BusEvent::Speaking { cause: SpeakCause::Announcement, text, .. } = event {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(announced.len(), 2, "two completions, each once: {announced:?}");
    assert!(announced.iter().any(|t| t.starts_with("The report is done.")), "{announced:?}");
    assert!(
        announced.iter().any(|t| t.starts_with("The parser fix is done.")),
        "the steered job, after its steered turn: {announced:?}"
    );
    assert_eq!(world.eileen_use().workers, 0, "no worker session is left");
}

//! The world of section 4.7 around the real core: the PC's memory readings every 5 s with a
//! heavy app that grows, pages and closes, the CPU and power, a brain that stalls, and each
//! fault point breaking its part on purpose. Every step keeps every invariant the world checks.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::connectors::{SidecarEvent, SidecarId};
use eileen_contracts::core_io::{ActivityEvent, Effect, Input, UserInput};
use eileen_contracts::harness::WorkerEvent;
use eileen_contracts::ids::{Personal, TurnId};
use eileen_contracts::jobs::MemoryPressure;
use eileen_contracts::speech::{SpeechCmd, TurnEndReason, VoiceEvent};
use eileen_sim::faults::{
    self, BRAIN_FAIL, BUS_STALL, Faults, SPEECH_CRASH, SPEECH_SLOW_LOAD, WORKER_CRASH,
};
use eileen_sim::invariants::ChildEvent;
use eileen_sim::parts::{LOAD_MS, Process, SLOW_LOAD_MS};
use eileen_sim::world::StepOutcome;
use eileen_sim::{BrainReply, Profile, World};

/// A world of `profile` whose core took its first step and whose speech process is ready,
/// with `faults` switched on from the start.
fn world_with(profile: &str, faults: Faults) -> World {
    let mut world = World::new(Profile::load(profile).unwrap(), 1);
    world.set_faults(faults);
    step(&mut world, Input::User(UserInput::SetMute(false)));
    world
}

fn ready(profile: &str) -> World {
    let mut world = world_with(profile, Faults::off());
    settle(&mut world, LOAD_MS);
    assert_eq!(world.speech().process(), Process::Up, "speech is ready");
    world
}

fn ok(outcomes: &[StepOutcome]) {
    for outcome in outcomes {
        assert_eq!(outcome.violation, None, "{}", outcome.line);
    }
}

fn step(world: &mut World, input: Input) {
    let outcome = world.step(input);
    assert_eq!(outcome.violation, None, "{}", outcome.line);
}

fn settle(world: &mut World, ms: u64) {
    let outcomes = world.settle(ms);
    ok(&outcomes);
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

/// Every memory reading delivered so far, with the virtual ms it came at.
fn readings(world: &World) -> Vec<(u64, MemoryPressure)> {
    world
        .inputs()
        .iter()
        .filter_map(|record| {
            if let Input::Memory(reading) = &record.input {
                Some((record.mono.0 / 1_000_000, reading.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Every clause the core sent to the speech process.
fn spoken(world: &World) -> Vec<String> {
    world
        .effects()
        .iter()
        .filter_map(|effect| {
            if let Effect::Speech(SpeechCmd::Speak { clauses, .. }) = &effect.body {
                Some(clauses.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

#[test]
fn memory_is_read_every_five_seconds_with_eileens_own_share() {
    let mut world = ready("low-memory");
    settle(&mut world, 18_000);
    let readings = readings(&world);
    let times: Vec<u64> = readings.iter().map(|(at, _)| *at).collect();
    assert_eq!(times, [5_000, 10_000, 15_000, 20_000], "every 5 s");
    let (_, first) = &readings[0];
    // 16,384 less 12,600 for the other apps, 60 core, 400 brain and 1,250 speech.
    assert_eq!(first.available_mb, 2_074, "{first:?}");
    assert_eq!(first.commit_headroom_mb, 3_770, "{first:?}");
    assert_eq!(first.paging_kb_s, 0, "above the floor");
    assert_eq!(first.paging_limit_kb_s, 4_000, "the limit the runtime reports");
    assert_eq!(world.memory(), readings[3].1, "the reading now is the last one delivered");
}

#[test]
fn a_heavy_app_grows_pushes_memory_into_paging_and_closes() {
    let mut world = ready("low-memory");
    settle(&mut world, 128_000);
    let at = |ms: u64| {
        readings(&world)
            .into_iter()
            .find(|(when, _)| *when == ms)
            .map_or_else(|| panic!("a reading at {ms} ms"), |(_, reading)| reading)
    };
    assert_eq!(at(20_000).available_mb, 2_074, "opens with nothing at 20 s");
    assert_eq!(at(30_000).available_mb, 1_574, "500 MB after 10 s at 50 MB/s");
    assert_eq!(at(30_000).paging_kb_s, 0, "still above the 1,000 MB floor");
    let peak = at(60_000);
    assert_eq!(peak.available_mb, 74, "2,000 MB at its peak: {peak:?}");
    assert_eq!(peak.paging_kb_s, 37_040, "926 MB under the floor at 40 KB/s each");
    assert_eq!(peak.commit_headroom_mb, 1_770, "{peak:?}");
    assert_eq!(
        peak.largest_process,
        Some((Personal::new("browser".to_owned()), 3_000)),
        "the browser stays the biggest"
    );
    assert_eq!(at(115_000), peak, "it holds its peak");
    assert_eq!(at(120_000), at(20_000), "and gives it all back when it closes");
}

#[test]
fn the_cpu_and_power_follow_the_profile_and_the_heavy_app() {
    let mut world = ready("low-memory");
    assert_eq!(world.load().cpu_busy_pct, 30, "the base load");
    assert!(!world.load().heavy_app_open, "not yet open");
    settle(&mut world, 30_000);
    assert_eq!(world.load().cpu_busy_pct, 90, "the heavy app adds 60 percent");
    assert!(!world.load().power.has_battery, "a desktop");
    assert!(!world.load().build_slot_held, "no build yet");
    settle(&mut world, 40_000);
    assert!(world.load().build_slot_held, "a build holds a slot from 60 s until 100 s");
    assert!(ready("media-playing").load().power.on_battery, "a laptop on its battery");
}

#[test]
fn media_plays_from_the_start_of_a_media_playing_run() {
    let mut world = World::new(Profile::load("media-playing").unwrap(), 1);
    let first = world.advance(10).expect("an input at the start");
    assert_eq!(
        first,
        Input::Activity(ActivityEvent::OtherAudio { playing: true }),
        "the video is already playing"
    );
    let mut quiet = World::new(Profile::default_profile().unwrap(), 1);
    assert_eq!(quiet.advance(10), None, "the default profile starts in silence");
}

#[test]
fn a_stalled_brains_late_answer_is_never_spoken_after_the_next_turn() {
    let mut world = ready("default");
    world.script_brain(BrainReply::text(&["The stalled answer."]).stalling(20_000));
    world.script_brain(BrainReply::text(&["The fresh answer."]));
    say(&mut world, 1, "Eileen, tell me about rivers");
    settle(&mut world, 5_000);
    assert!(spoken(&world).is_empty(), "nothing yet from the stalled brain");
    say(&mut world, 2, "Eileen, tell me about lakes");
    settle(&mut world, 30_000);
    let late = world.inputs().iter().any(|record| {
        matches!(&record.input, Input::Brain(eileen_contracts::harness::BrainEvent::TextDelta {
            text, ..
        }) if text.get() == "The stalled answer.")
    });
    assert!(late, "the stalled instance did send its whole answer, late");
    let spoken = spoken(&world);
    assert!(spoken.iter().any(|clause| clause == "The fresh answer."), "{spoken:?}");
    assert!(!spoken.iter().any(|clause| clause.contains("stalled")), "{spoken:?}");
}

#[test]
fn a_failed_brain_turn_is_answered_with_an_apology() {
    let mut world = world_with("default", Faults::every(&[BRAIN_FAIL]).unwrap());
    settle(&mut world, LOAD_MS);
    say(&mut world, 1, "Eileen, tell me about rivers");
    settle(&mut world, 5_000);
    assert!(
        world.faults().iter().any(|fault| fault.site == BRAIN_FAIL),
        "the fault fired: {:?}",
        world.faults()
    );
    let spoken = spoken(&world);
    assert!(spoken.iter().any(|clause| clause.starts_with("Sorry")), "{spoken:?}");
}

#[test]
fn a_slow_load_readies_the_speech_process_later() {
    let mut world = world_with("default", Faults::every(&[SPEECH_SLOW_LOAD]).unwrap());
    settle(&mut world, LOAD_MS);
    assert_eq!(world.speech().process(), Process::Loading, "not ready after the usual load");
    settle(&mut world, SLOW_LOAD_MS);
    assert_eq!(world.speech().process(), Process::Up, "ready after the slow one");
    assert_eq!(world.faults().len(), 1, "one load, one fault: {:?}", world.faults());
}

#[test]
fn a_speech_process_that_crashes_at_each_heartbeat_is_restarted_with_backoff() {
    let mut world = world_with("default", Faults::every(&[SPEECH_CRASH]).unwrap());
    settle(&mut world, 180_000);
    let speech = |event: ChildEvent| {
        world
            .children()
            .iter()
            .filter(|c| c.child == SidecarId::Speech && c.event == event)
            .map(|c| c.at_ms)
            .collect::<Vec<u64>>()
    };
    let downs = speech(ChildEvent::Down);
    assert!(downs.len() >= 5, "it crashed again and again: {:?}", world.children());
    let restarts = speech(ChildEvent::Restarted);
    assert!(restarts.len() >= 5, "and was restarted each time: {:?}", world.children());
    // Each gap is a load, a heartbeat and the backoff step of 1 s doubling to 60 s, so the
    // fourth gap's step is at least 8 s longer than the first's.
    let gaps: Vec<u64> = restarts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(gaps.windows(2).all(|pair| pair[1] >= pair[0]), "the restarts space out: {gaps:?}");
    assert!(gaps[3] >= gaps[0] + 8_000, "by a doubling step: {gaps:?}");
    assert!(
        world.inputs().iter().any(|record| matches!(
            record.input,
            Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { code: Some(_) })
        )),
        "each crash is an exit with a code"
    );
}

#[test]
fn a_worker_that_stops_part_way_fails_its_job() {
    let mut world = world_with("default", Faults::every(&[WORKER_CRASH]).unwrap());
    settle(&mut world, LOAD_MS);
    world.script_brain(
        BrainReply::text(&["Sure, I will make it. "])
            .dispatching("the hello file", "create hello.txt containing hi"),
    );
    say(&mut world, 1, "Eileen, create a file called hello dot txt containing hi");
    settle(&mut world, 20_000);
    let exited = world.inputs().iter().any(|record| {
        matches!(record.input, Input::Worker(_, WorkerEvent::Exited { code: Some(_) }))
    });
    assert!(exited, "the session exited part way");
    assert!(
        world.published().iter().any(|event| matches!(event.body, BusEvent::JobFailed { .. })),
        "and its job failed"
    );
    assert_eq!(world.eileen_use().workers, 0, "the session's memory is free again");
}

#[test]
fn a_bus_subscriber_that_stalls_at_every_step_keeps_the_invariants() {
    let mut world = world_with("default", Faults::every(&[BUS_STALL]).unwrap());
    settle(&mut world, LOAD_MS);
    for n in 1..=20 {
        say(&mut world, n, "what time is it");
        settle(&mut world, 3_000);
    }
    let steps = world.steps();
    let stalls = world.faults().iter().filter(|fault| fault.site == BUS_STALL).count();
    assert_eq!(stalls, usize::try_from(steps).unwrap(), "the subscriber stalled at every step");
    assert!(world.faults().iter().all(|fault| faults::is_world_site(&fault.site)), "world sites");
}

//! Slice S1's flows through the minimal core, in the simulator (BUILD.md section 6.3, the
//! first pull request): the fast path, "stop" while Eileen speaks, a backchannel, a brain
//! answer clause by clause, a job's completion, mute, and room talk. Every step keeps every
//! invariant the world checks.

use eileen_contracts::bus::{BusEvent, SpeakCause};
use eileen_contracts::core_io::{AudioCmd, Effect, Input, PlaybackEvent, UserInput};
use eileen_contracts::harness::{BrainCmd, HookDecision, HookReply};
use eileen_contracts::ids::{Personal, SpeakId, TurnId};
use eileen_contracts::speech::{SpeechCmd, TurnEndReason, VoiceEvent};
use eileen_sim::world::StepOutcome;
use eileen_sim::{BrainReply, Profile, World};

fn world() -> World {
    World::new(Profile::default_profile().unwrap(), 1)
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

/// Lets `ms` of virtual time pass, delivering what the parts answer.
fn settle(world: &mut World, ms: u64) {
    let outcomes = world.settle(ms);
    ok(&outcomes);
}

/// The user speaks `text` as turn `n`: speech starts, 400 ms later the turn ends.
fn say(world: &mut World, n: u64, text: &str) -> TurnId {
    let turn = TurnId::from_parts(world.now_ms(), u128::from(n));
    step(world, Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0 }));
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
    turn
}

fn bodies(world: &World, from: usize) -> Vec<Effect> {
    world.effects().iter().skip(from).map(|effect| effect.body.clone()).collect()
}

fn events(world: &World) -> Vec<BusEvent> {
    world.published().iter().map(|event| event.body.clone()).collect()
}

fn speaks(effects: &[Effect]) -> Vec<(SpeakId, Vec<String>)> {
    effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Speech(SpeechCmd::Speak { speak, clauses, .. }) = effect {
                Some((*speak, clauses.clone()))
            } else {
                None
            }
        })
        .collect()
}

fn brain_turns(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::Brain(BrainCmd::Turn { .. }))).count()
}

/// Asks the brain a question whose scripted answer is long, and waits until Eileen speaks it.
fn speaking_a_long_answer(world: &mut World) -> SpeakId {
    world.script_brain(BrainReply::text(&[
        "Rivers carry water from the hills down to the sea. ",
        "Along the way they cut valleys, carry sand and silt, and feed the lakes and marshes \
         that so many animals depend on. ",
        "Some of the longest run for thousands of kilometres.",
    ]));
    let from = world.effects().len();
    say(world, 1, "Eileen, tell me about rivers");
    settle(world, 1_500);
    let spoken = speaks(&bodies(world, from));
    let (speak, _) = spoken.first().cloned().expect("Eileen is speaking the answer");
    assert!(
        world
            .published()
            .iter()
            .any(|e| matches!(&e.body, BusEvent::Speaking { speak: s, .. } if *s == speak)),
        "the answer is on the bus as speaking"
    );
    speak
}

/// The playback reports for `speak` delivered so far, as (clause, samples).
fn played(world: &World, speak: SpeakId) -> Vec<(u16, u32)> {
    world
        .inputs()
        .iter()
        .filter_map(|record| {
            if let Input::Playback(
                PlaybackEvent::Played { speak: s, clause, samples_played }
                | PlaybackEvent::Paused { speak: s, clause, samples_played },
            ) = &record.input
                && *s == speak
            {
                Some((*clause, *samples_played))
            } else {
                None
            }
        })
        .collect()
}

/// Barge-in windows over `speak`, 32 ms apart, numbered from 1.
fn barge_in(world: &mut World, speak: SpeakId, windows: u8) {
    for window in 1..=windows {
        if window > 1 {
            settle(world, 32);
        }
        step(world, Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
}

#[test]
fn what_time_is_answered_by_the_core_with_no_brain_turn() {
    let mut world = world();
    say(&mut world, 1, "Eileen, what time is it?");
    settle(&mut world, 3_000);
    let effects = bodies(&world, 0);
    assert_eq!(brain_turns(&effects), 0, "no brain turn");
    let answer = events(&world).into_iter().find_map(|event| {
        if let BusEvent::Speaking { cause: SpeakCause::Reply, text, .. } = event {
            Some(text)
        } else {
            None
        }
    });
    assert!(answer.as_deref().is_some_and(|text| text.starts_with("It's ")), "{answer:?}");
    assert!(
        events(&world)
            .iter()
            .any(|e| matches!(e, BusEvent::SpeakingStopped { interrupted: false, .. })),
        "spoken to the end"
    );
}

#[test]
fn stop_while_eileen_speaks_ducks_pauses_and_stops_within_the_decision() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let user = TurnId::from_parts(world.now_ms(), 2);
    step(&mut world, Input::Voice(VoiceEvent::SpeechStarted { turn: user, source: 0 }));
    let onset = world.now_ms();
    let from = world.effects().len();
    barge_in(&mut world, speak, 1);
    assert!(
        bodies(&world, from).contains(&Effect::Audio(AudioCmd::Duck { db: -20 })),
        "ducked at the first window"
    );
    barge_in_more(&mut world, speak);
    let paused_at = world.now_ms();
    assert!(
        bodies(&world, from).contains(&Effect::Audio(AudioCmd::Pause { speak })),
        "paused by the third window"
    );
    assert!(paused_at - onset <= 96, "onset to pause within 96 ms: {}", paused_at - onset);
    settle(&mut world, 250);
    let from = world.effects().len();
    step(
        &mut world,
        Input::Voice(VoiceEvent::EndOfTurn {
            turn: user,
            text: Personal::new("stop".to_owned()),
            smart_turn_pct: 95,
            silence_ms: 200,
            asr_confidence_pct: 97,
            reason: TurnEndReason::SmartTurn,
            source: 0,
            level_db: -27,
        }),
    );
    let stopped = bodies(&world, from);
    assert!(stopped.contains(&Effect::Audio(AudioCmd::Stop { speak })), "playback stops");
    assert!(
        stopped.contains(&Effect::Speech(SpeechCmd::Cancel { speak })),
        "synthesis is cancelled"
    );
    assert!(world.now_ms() - paused_at <= 600, "decided within 600 ms of the pause");
    let before = played(&world, speak).len();
    settle(&mut world, 2_000);
    assert_eq!(played(&world, speak).len(), before, "nothing more of it plays");
    assert!(
        events(&world).iter().any(|e| matches!(e, BusEvent::SpeakingStopped { speak: s, interrupted: true, .. } if *s == speak)),
        "stopped, interrupted"
    );
}

/// Windows 2 and 3 after the first.
fn barge_in_more(world: &mut World, speak: SpeakId) {
    for window in 2..=3 {
        settle(world, 32);
        step(world, Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
}

#[test]
fn a_backchannel_resumes_from_the_paused_sample() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let user = TurnId::from_parts(world.now_ms(), 3);
    step(&mut world, Input::Voice(VoiceEvent::SpeechStarted { turn: user, source: 0 }));
    barge_in(&mut world, speak, 3);
    settle(&mut world, 20);
    let at_pause = *played(&world, speak).last().expect("the pause is reported");
    settle(&mut world, 200);
    assert_eq!(played(&world, speak).last(), Some(&at_pause), "nothing plays while paused");
    let from = world.effects().len();
    step(
        &mut world,
        Input::Voice(VoiceEvent::EndOfTurn {
            turn: user,
            text: Personal::new("mm-hm".to_owned()),
            smart_turn_pct: 90,
            silence_ms: 200,
            asr_confidence_pct: 90,
            reason: TurnEndReason::SmartTurn,
            source: 0,
            level_db: -30,
        }),
    );
    let resumed = bodies(&world, from);
    assert!(resumed.contains(&Effect::Audio(AudioCmd::Resume { speak })), "resumed");
    assert!(resumed.contains(&Effect::Audio(AudioCmd::Unduck)), "unducked");
    let count = played(&world, speak).len();
    settle(&mut world, 100);
    let after = played(&world, speak);
    let next = after.get(count).copied().expect("playback reports again");
    assert!(next > at_pause, "it goes on from the paused sample: {at_pause:?} then {next:?}");
    let (clause, samples) = next;
    assert!(
        clause > at_pause.0 || samples - at_pause.1 <= 24 * 100,
        "not from the start of the clause, and no further than 100 ms of audio: {next:?}"
    );
    assert_eq!(
        events(&world).iter().filter(|e| matches!(e, BusEvent::TurnStarted { .. })).count(),
        1,
        "only the question was a turn; the backchannel never reached the bus"
    );
}

#[test]
fn a_brain_answer_is_spoken_clause_by_clause() {
    let mut world = world();
    world.script_brain(BrainReply::text(&[
        "Rivers carry water to the sea. ",
        "They shape valleys, ",
        "and they feed lakes.",
    ]));
    say(&mut world, 1, "Eileen, tell me about rivers");
    settle(&mut world, 10_000);
    let spoken = speaks(&bodies(&world, 0));
    assert_eq!(spoken.len(), 2, "two clauses, sent as they were cut: {spoken:?}");
    let speak = spoken[0].0;
    assert!(spoken.iter().all(|(s, _)| *s == speak), "one utterance");
    assert_eq!(spoken[0].1, ["Rivers carry water to the sea."], "the first clause first");
    assert_eq!(spoken[1].1, ["They shape valleys, and they feed lakes."], "then the rest");
    let heard = events(&world).into_iter().find_map(|event| {
        if let BusEvent::SpeakingStopped { speak: s, heard_chars, interrupted: false, .. } = event
            && s == speak
        {
            Some(heard_chars)
        } else {
            None
        }
    });
    assert_eq!(heard, Some(71), "every character was heard");
}

#[test]
fn a_dispatched_jobs_completion_is_announced_once() {
    let mut world = world();
    world.script_brain(
        BrainReply::text(&["Sure, I will make it. "])
            .dispatching("the hello file", "create hello.txt containing hi"),
    );
    say(&mut world, 1, "Eileen, create a file called hello dot txt containing hi");
    settle(&mut world, 20_000);
    let events = events(&world);
    let job = events.iter().find_map(|event| {
        if let BusEvent::JobAccepted { job, .. } = event { Some(*job) } else { None }
    });
    let job = job.expect("the job was accepted");
    assert!(
        events.iter().any(|e| matches!(e, BusEvent::JobCompleted { job: j, .. } if *j == job)),
        "it completed"
    );
    let effects = bodies(&world, 0);
    assert!(
        effects.iter().any(|e| matches!(e, Effect::ToolReply { result, .. } if !result.is_error)),
        "the brain got the job's id back"
    );
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Hook(_, HookReply::PreToolUse { decision: HookDecision::None, .. })
        )),
        "the write in the job's folder was not blocked"
    );
    let announcements: Vec<&BusEvent> = events
        .iter()
        .filter(|e| matches!(e, BusEvent::Speaking { cause: SpeakCause::Announcement, .. }))
        .collect();
    assert_eq!(announcements.len(), 1, "announced once: {announcements:?}");
    assert!(
        matches!(announcements[0], BusEvent::Speaking { text, .. } if text.contains("Claude says: Created hello.txt")),
        "attributed: {announcements:?}"
    );
}

#[test]
fn mute_stops_capture_and_nothing_is_spoken() {
    let mut world = world();
    step(&mut world, Input::User(UserInput::SetMute(true)));
    assert!(
        bodies(&world, 0).contains(&Effect::Audio(AudioCmd::SetCapture(false))),
        "capture stops"
    );
    assert!(!world.device().capture(), "the device stopped capturing");
    let from = world.effects().len();
    say(&mut world, 1, "Eileen, what time is it?");
    step(
        &mut world,
        Input::User(UserInput::Typed { text: Personal::new("what time is it".to_owned()) }),
    );
    settle(&mut world, 3_000);
    let after = bodies(&world, from);
    assert!(speaks(&after).is_empty(), "nothing spoken while muted: {after:?}");
    assert!(
        !after.iter().any(|e| matches!(e, Effect::Audio(AudioCmd::Earcon(_)))),
        "no earcon either"
    );
}

#[test]
fn an_unnamed_non_command_outside_the_window_produces_no_bus_event() {
    let mut world = world();
    say(&mut world, 1, "want coffee?");
    settle(&mut world, 2_000);
    assert!(world.published().is_empty(), "no bus event: {:?}", events(&world));
    assert!(world.effects().is_empty(), "no effect at all");
}

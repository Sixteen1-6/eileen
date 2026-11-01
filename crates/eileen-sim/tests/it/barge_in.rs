//! Barge-in in simulation (BUILD.md section 6.3 acceptance 4), through the real core with the
//! device's 10 ms callbacks: onset to pause within 96 ms of virtual time plus one callback, the
//! heard prefix equal to the text of the samples played, and a backchannel that resumes from
//! the paused sample. Each test also runs invariants 2, 3 and 9 over the world's audio trace.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{AudioCmd, Effect, Input, UserInput};
use eileen_contracts::harness::BrainCmd;
use eileen_contracts::ids::{Personal, SpeakId, TurnId};
use eileen_contracts::speech::{SpeechCmd, TurnEndReason, VoiceEvent};
use eileen_sim::invariants::{
    AudioEvent, AudioRecord, HEARD_MARKER, WINDOW_MS, barge_in_pauses_in_time,
    heard_prefix_matches_playback, nothing_sounds_while_muted,
};
use eileen_sim::parts::{CALLBACK_MS, LOAD_MS};
use eileen_sim::{BrainReply, Profile, World};

/// A world whose core took its first step and whose speech process is ready.
fn world() -> World {
    let mut world = World::new(Profile::default_profile().unwrap(), 1);
    step(&mut world, Input::User(UserInput::SetMute(false)));
    settle(&mut world, LOAD_MS);
    world
}

fn step(world: &mut World, input: Input) {
    let outcome = world.step(input);
    assert_eq!(outcome.violation, None, "{}", outcome.line);
}

fn settle(world: &mut World, ms: u64) {
    for outcome in world.settle(ms) {
        assert_eq!(outcome.violation, None, "{}", outcome.line);
    }
}

fn end_of_turn(world: &mut World, turn: TurnId, text: &str) {
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

fn speech_started(world: &mut World, n: u128) -> TurnId {
    let turn = TurnId::from_parts(world.now_ms(), n);
    step(world, Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }));
    turn
}

/// Eileen speaks a long scripted answer to a question; returns its utterance.
fn speaking_a_long_answer(world: &mut World) -> SpeakId {
    world.script_brain(BrainReply::text(&[
        "Rivers carry water from the hills down to the sea. ",
        "Along the way they cut valleys and feed the lakes and marshes. ",
        "Some of the longest run for thousands of kilometres.",
    ]));
    let turn = speech_started(world, 1);
    settle(world, 400);
    end_of_turn(world, turn, "Eileen, tell me about rivers");
    settle(world, 1_500);
    world
        .effects()
        .iter()
        .find_map(|effect| {
            if let Effect::Speech(SpeechCmd::Speak { speak, .. }) = &effect.body {
                Some(*speak)
            } else {
                None
            }
        })
        .expect("Eileen is speaking the answer")
}

/// The user speaks over Eileen from now: speech starts, and the speech process sends three
/// consecutive windows at its pace, the first one window after the onset. Returns the turn and
/// the onset, ms.
fn speak_over(world: &mut World, speak: SpeakId) -> (TurnId, u64) {
    let turn = speech_started(world, 2);
    let onset = world.now_ms();
    for window in 1..=3 {
        settle(world, WINDOW_MS);
        step(world, Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
    (turn, onset)
}

/// When the core emitted `cmd`, ms.
fn emitted_at(audio: &[AudioRecord], cmd: &AudioCmd) -> Option<u64> {
    audio.iter().find_map(|record| {
        if let AudioEvent::Effect(Effect::Audio(found)) = &record.event
            && found == cmd
        {
            Some(record.at_ms)
        } else {
            None
        }
    })
}

/// Samples of `speak` handed to the device by the callbacks in `audio`.
fn handed(audio: &[AudioRecord], speak: SpeakId) -> u64 {
    audio
        .iter()
        .filter_map(|record| {
            if let AudioEvent::Callback(parts) = &record.event { Some(parts) } else { None }
        })
        .flatten()
        .filter(|part| part.speak == speak)
        .map(|part| u64::from(part.samples))
        .sum()
}

/// The samples before `(clause, samples)` in `speak`, from its clauses' sizes.
fn through(world: &World, speak: SpeakId, (clause, samples): (u16, u32)) -> u64 {
    let before: u64 = world
        .inputs()
        .iter()
        .filter_map(|record| {
            if let Input::Voice(VoiceEvent::ClauseReady {
                speak: s, clause: c, samples: size, ..
            }) = &record.input
                && *s == speak
                && *c < clause
            {
                Some(u64::from(*size))
            } else {
                None
            }
        })
        .sum();
    before + u64::from(samples)
}

fn invariants_hold(world: &World) {
    let audio = world.audio();
    assert_eq!(barge_in_pauses_in_time(audio, world.now_ms()), Ok(()), "invariant 2");
    assert_eq!(heard_prefix_matches_playback(audio), Ok(()), "invariant 3");
    assert_eq!(nothing_sounds_while_muted(audio), Ok(()), "invariant 9");
}

#[test]
fn onset_to_pause_is_at_most_96_ms_plus_one_callback() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let (_, onset) = speak_over(&mut world, speak);
    settle(&mut world, 2 * CALLBACK_MS);
    let audio = world.audio();
    let paused = emitted_at(audio, &AudioCmd::Pause { speak }).expect("paused");
    assert!(paused - onset <= 3 * WINDOW_MS, "paused by the third window: {} ms", paused - onset);
    let silent = audio
        .iter()
        .filter(|record| record.at_ms > paused)
        .find_map(|record| {
            if let AudioEvent::Callback(parts) = &record.event
                && parts.iter().all(|part| part.speak != speak)
            {
                Some(record.at_ms)
            } else {
                None
            }
        })
        .expect("a callback after the pause plays nothing of it");
    assert!(
        silent - onset <= 3 * WINDOW_MS + CALLBACK_MS,
        "onset to silence {} ms, over 96 ms plus one callback",
        silent - onset
    );
    invariants_hold(&world);
}

#[test]
fn the_heard_prefix_equals_the_text_of_the_samples_played() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let (turn, _) = speak_over(&mut world, speak);
    settle(&mut world, 200);
    let at_pause = world.device().position(speak).expect("the device holds the answer");
    assert_eq!(
        handed(world.audio(), speak),
        through(&world, speak, at_pause),
        "the paused position is every sample handed to the device"
    );
    end_of_turn(&mut world, turn, "Eileen, what about lakes");
    let told = world.effects().iter().rev().find_map(|effect| {
        if let Effect::Brain(BrainCmd::Turn { message, .. }) = &effect.body {
            Some(message.clone())
        } else {
            None
        }
    });
    let told = told.expect("the request went to the brain");
    assert!(told.contains(HEARD_MARKER), "the brain is told what was heard: {told}");
    let stopped = world.published().iter().find_map(|event| {
        if let BusEvent::SpeakingStopped { speak: s, heard_chars, interrupted: true, .. } =
            &event.body
            && *s == speak
        {
            Some(*heard_chars)
        } else {
            None
        }
    });
    let heard_chars = stopped.expect("the answer stopped, interrupted");
    assert!(heard_chars > 0, "some of it was heard");
    invariants_hold(&world);
}

#[test]
fn a_backchannel_resumes_from_the_exact_paused_sample() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let (turn, _) = speak_over(&mut world, speak);
    settle(&mut world, CALLBACK_MS);
    let at_pause = world.device().position(speak).expect("the device holds the answer");
    let handed_at_pause = handed(world.audio(), speak);
    assert_eq!(handed_at_pause, through(&world, speak, at_pause), "paused where it was");
    settle(&mut world, 200);
    assert_eq!(handed(world.audio(), speak), handed_at_pause, "nothing plays while paused");
    end_of_turn(&mut world, turn, "mm-hm");
    assert!(emitted_at(world.audio(), &AudioCmd::Resume { speak }).is_some(), "resumed");
    settle(&mut world, CALLBACK_MS);
    let after = world.device().position(speak).expect("still playing the answer");
    let next = through(&world, speak, after);
    assert_eq!(
        next,
        handed_at_pause + 240,
        "one callback later it is 240 samples past the paused sample: {at_pause:?} then {after:?}"
    );
    assert_eq!(handed(world.audio(), speak), next, "and every sample played was handed once");
    invariants_hold(&world);
}

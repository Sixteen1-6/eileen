//! Barge-in in simulation (BUILD.md section 6.3 acceptance 4), through the real core with the
//! device's 10 ms callbacks: onset to pause within 96 ms of virtual time plus one callback, the
//! heard prefix equal to the text of the samples played, and a backchannel that resumes from
//! the paused sample. Each test also runs invariants 2, 3 and 9 over the world's audio trace.
//! Two known gaps are asserted as they are today: a stop with no pause first can tell a heard
//! prefix a character short (#151), and Eileen keeps speaking while another app uses the
//! microphone (#152).

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{ActivityEvent, AudioCmd, Effect, Input, PlaybackEvent, UserInput};
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
    world.record_audio();
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

/// The first sentence of the scripted answer.
const FIRST_SENTENCE: &str = "Rivers carry water from the hills down to the sea.";

/// The clauses of `speak` as the core sent them to synthesis, in order.
fn clauses_of(world: &World, speak: SpeakId) -> Vec<String> {
    world
        .effects()
        .iter()
        .filter_map(|effect| {
            if let Effect::Speech(SpeechCmd::Speak { speak: s, clauses, .. }) = &effect.body
                && *s == speak
            {
                Some(clauses.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

/// The size of clause `clause` of `speak` as the speech process reported it ready: characters
/// and samples.
fn size_of(world: &World, speak: SpeakId, clause: u16) -> (u64, u64) {
    world
        .inputs()
        .iter()
        .find_map(|record| {
            if let Input::Voice(VoiceEvent::ClauseReady { speak: s, clause: c, chars, samples }) =
                &record.input
                && *s == speak
                && *c == clause
            {
                Some((u64::from(*chars), u64::from(*samples)))
            } else {
                None
            }
        })
        .expect("the clause was synthesized")
}

/// The non-empty pieces, trimmed, joined with single spaces.
fn joined<'a>(pieces: impl IntoIterator<Item = &'a str>) -> String {
    pieces
        .into_iter()
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Section 2.8's heard prefix of `speak` at `(clause, samples)`, worked out here from the
/// scripted text: the clauses before in full, then the playing clause's characters in
/// proportion to its samples played.
fn heard_after(world: &World, speak: SpeakId, (clause, samples): (u16, u32)) -> String {
    let clauses = clauses_of(world, speak);
    assert!(
        joined(clauses.iter().map(String::as_str)).starts_with(FIRST_SENTENCE),
        "the clauses are the scripted answer: {clauses:?}"
    );
    let (chars, total) = size_of(world, speak, clause);
    let cut = usize::try_from(chars * u64::from(samples) / total).unwrap();
    let index = usize::from(clause);
    let playing: String = clauses[index].chars().take(cut).collect();
    joined(clauses[..index].iter().map(String::as_str).chain([playing.as_str()]))
}

/// The last position of `speak` the device reported to the core.
fn last_reported(world: &World, speak: SpeakId) -> Option<(u16, u32)> {
    world.inputs().iter().rev().find_map(|record| {
        if let Input::Playback(PlaybackEvent::Played { speak: s, clause, samples_played }) =
            &record.input
            && *s == speak
        {
            Some((*clause, *samples_played))
        } else {
            None
        }
    })
}

/// Plays on a callback at a time until the device has handed more of the playing clause than
/// the core's last report carries, by a character the heard prefix shows. Returns the prefix
/// of the reported position and of the device's.
fn until_the_report_lags_a_character(world: &mut World, speak: SpeakId) -> (String, String) {
    for _ in 0..100 {
        let at = world.device().position(speak).expect("the device holds the answer");
        if let Some(reported) = last_reported(world, speak).filter(|reported| reported.0 == at.0) {
            let (told, heard) =
                (heard_after(world, speak, reported), heard_after(world, speak, at));
            if told != heard {
                return (told, heard);
            }
        }
        settle(world, CALLBACK_MS);
    }
    panic!("the report never lagged the device by a character");
}

/// The heard prefix a brain turn's message carries.
fn told_in(message: &str) -> Option<&str> {
    let line = message.split(HEARD_MARKER).nth(1)?.lines().next()?;
    line.rsplit_once('"').map(|(heard, _)| heard)
}

/// The last brain turn's message.
fn last_brain_message(world: &World) -> Option<String> {
    world.effects().iter().rev().find_map(|effect| {
        if let Effect::Brain(BrainCmd::Turn { message, .. }) = &effect.body {
            Some(message.clone())
        } else {
            None
        }
    })
}

/// The `heard_chars` of the interrupted stop of `speak`.
fn heard_chars_of(world: &World, speak: SpeakId) -> Option<u32> {
    world.published().iter().find_map(|event| {
        if let BusEvent::SpeakingStopped { speak: s, heard_chars, interrupted: true, .. } =
            &event.body
            && *s == speak
        {
            Some(*heard_chars)
        } else {
            None
        }
    })
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
    let expected = heard_after(&world, speak, at_pause);
    let answer = joined(clauses_of(&world, speak).iter().map(String::as_str));
    assert!(!expected.is_empty() && expected.len() < answer.len(), "{expected:?}");
    assert!(answer.starts_with(&expected), "a prefix of the answer: {expected:?}");
    end_of_turn(&mut world, turn, "Eileen, what about lakes");
    let message = last_brain_message(&world).expect("the request went to the brain");
    assert_eq!(told_in(&message), Some(expected.as_str()), "the brain is told it: {message}");
    let heard_chars = heard_chars_of(&world, speak).expect("the answer stopped, interrupted");
    assert_eq!(
        usize::try_from(heard_chars).unwrap(),
        expected.chars().count(),
        "and the stop counts its characters"
    );
    invariants_hold(&world);
}

#[test]
fn a_mute_mid_sentence_counts_the_heard_prefix_a_character_short() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let (reported, played) = until_the_report_lags_a_character(&mut world, speak);
    step(&mut world, Input::User(UserInput::SetMute(true)));
    let heard_chars = heard_chars_of(&world, speak).expect("muting stopped the answer");
    // FIXME(#151): the stop counts the last report's prefix, not the samples played.
    assert_eq!(usize::try_from(heard_chars).unwrap(), reported.chars().count(), "{reported:?}");
    assert!(played.starts_with(&reported) && played.len() > reported.len(), "{played:?}");
    let error = heard_prefix_matches_playback(world.audio()).unwrap_err();
    assert_eq!(error.number, 3, "invariant 3 sees the shortfall: {error}");
    let audio = world.audio();
    assert_eq!(barge_in_pauses_in_time(audio, world.now_ms()), Ok(()), "invariant 2");
    assert_eq!(nothing_sounds_while_muted(audio), Ok(()), "invariant 9");
}

#[test]
fn a_request_after_one_window_tells_the_brain_a_heard_prefix_a_character_short() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let (reported, played) = until_the_report_lags_a_character(&mut world, speak);
    let turn = speech_started(&mut world, 2);
    step(&mut world, Input::Voice(VoiceEvent::BargeInWindow { speak, window: 1, prob_pct: 90 }));
    assert!(emitted_at(world.audio(), &AudioCmd::Pause { speak }).is_none(), "ducked only");
    end_of_turn(&mut world, turn, "Eileen, what about lakes");
    let message = last_brain_message(&world).expect("the request went to the brain");
    // FIXME(#151): the brain is told the last report's prefix, not the samples played.
    assert_eq!(told_in(&message), Some(reported.as_str()), "{message}");
    assert!(played.starts_with(&reported) && played.len() > reported.len(), "{played:?}");
    let error = heard_prefix_matches_playback(world.audio()).unwrap_err();
    assert_eq!(error.number, 3, "invariant 3 sees the shortfall: {error}");
}

#[test]
fn eileen_keeps_speaking_while_another_app_uses_the_microphone() {
    let mut world = world();
    let speak = speaking_a_long_answer(&mut world);
    let before = handed(world.audio(), speak);
    step(&mut world, Input::Activity(ActivityEvent::MicInUseElsewhere(true)));
    settle(&mut world, 200);
    // FIXME(#152): the core ignores the call; she should fall silent and keep to the HUD.
    assert!(handed(world.audio(), speak) > before, "she plays on");
    let error = nothing_sounds_while_muted(world.audio()).unwrap_err();
    assert_eq!(error.number, 9, "{error}");
    assert!(error.detail.contains("another app used the microphone"), "{error}");
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

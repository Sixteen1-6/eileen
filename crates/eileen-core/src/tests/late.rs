//! Turns heard late through `Core::step` (BUILD.md section 2.8, "Speech heard late"): a turn
//! whose speech began in a replayed frame is addressed only when it begins with the name. The
//! conversation window, a built-in command and a follow-up address the same words heard live,
//! but never a turn heard late, and its partial decides a barge-in only by the name.

use eileen_contracts::bus::{BusEvent, TurnRoute};
use eileen_contracts::core_io::{AudioCmd, Effect, Input, PlaybackEvent};
use eileen_contracts::ids::{Personal, TurnId};
use eileen_contracts::speech::VoiceEvent;

use super::{Harness, ask_brain, brain_turn, delta, end_of_turn, events, spoken, timer_set};
use crate::CoreConfig;

/// The speech process recognises `text` from its ring after a restart: a whole turn whose
/// speech began in a replayed frame. Returns the effects of its end of turn.
fn say_late(h: &mut Harness, text: &str) -> Vec<Effect> {
    h.turns += 1;
    let turn = TurnId::from_parts(h.turns, u128::from(h.turns) + 77);
    h.step(Input::Voice(VoiceEvent::SpeechStarted { turn, source: 0, replayed: true }));
    h.wait(100);
    h.step(Input::Voice(end_of_turn(turn, text)))
}

/// Eileen answers aloud and the answer drains, so the conversation window opens; a second
/// passes.
fn open_window(h: &mut Harness) {
    let effects = h.say("Eileen, what time is it?");
    let (speak, _) = spoken(&effects)[0].clone();
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 13, samples: 1_000 }));
    let effects = h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    assert!(
        events(&effects)
            .iter()
            .any(|e| matches!(e, BusEvent::ConversationWindow { open: true, .. })),
        "the window opens when Eileen stops: {effects:?}"
    );
    h.wait(1_000);
}

#[test]
fn a_replayed_unnamed_command_inside_the_conversation_window_is_greyed() {
    let mut h = Harness::new();
    open_window(&mut h);
    let effects = say_late(&mut h, "what time is it");
    assert!(effects.is_empty(), "greyed: no bus event and nothing said: {effects:?}");
    let effects = h.say("what time is it");
    assert!(
        !spoken(&effects).is_empty(),
        "heard live, the same words are a command inside the window, which is still open"
    );
}

#[test]
fn a_replayed_built_in_command_outside_the_window_is_greyed() {
    let mut h = Harness::new();
    let effects = say_late(&mut h, "what time is it");
    assert!(effects.is_empty(), "rule 4 does not address a turn heard late: {effects:?}");
    let effects = h.say("what time is it");
    assert!(
        !spoken(&effects).is_empty(),
        "the next turn, heard live, is a full built-in command again"
    );
}

#[test]
fn a_replayed_name_led_command_routes() {
    let mut h = Harness::new();
    let effects = say_late(&mut h, "Eileen, what time is it?");
    let bus = events(&effects);
    assert!(matches!(bus.first(), Some(BusEvent::TurnStarted { .. })), "{bus:?}");
    assert!(
        matches!(
            bus.get(1),
            Some(BusEvent::TurnRouted { route: TurnRoute::FastPath, reason }) if reason == "what_time"
        ),
        "routed by the name: {bus:?}"
    );
    assert!(
        matches!(bus.get(2), Some(BusEvent::Heard { text }) if text.get() == "Eileen, what time is it?"),
        "{bus:?}"
    );
    assert_eq!(
        spoken(&effects).first().map(|s| s.1.clone()),
        Some(vec!["It's 3:05 pm.".to_owned()]),
        "answered"
    );
    assert_eq!(brain_turn(&effects), None, "by the core");
}

#[test]
fn a_replayed_command_that_only_ends_with_the_name_is_greyed() {
    let mut h = Harness::new();
    let effects = say_late(&mut h, "What time is it, Eileen?");
    assert!(effects.is_empty(), "a turn heard late must begin with the name: {effects:?}");
    let effects = h.say("What time is it, Eileen?");
    assert!(!spoken(&effects).is_empty(), "heard live, a name at the end addresses her");
}

#[test]
fn a_replayed_follow_up_is_not_taken() {
    let mut h = Harness::with(CoreConfig::default(), vec!["and how long will it take"]);
    open_window(&mut h);
    let effects = say_late(&mut h, "and how long will it take?");
    assert_eq!(brain_turn(&effects), None, "no brain turn for a follow-up heard late");
    assert!(effects.is_empty(), "greyed: {effects:?}");
    let effects = h.say("and how long will it take?");
    assert!(brain_turn(&effects).is_some(), "heard live, the follow-up passes to the brain");
}

#[test]
fn a_replayed_partial_decides_a_barge_in_only_by_the_name() {
    for (partial, stops) in [("stop", false), ("Eileen, stop", true)] {
        let mut h = Harness::new();
        let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
        let speak =
            spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
        // The user began speaking during a restart and went on after it, over Eileen.
        let late = TurnId::from_parts(70, 70);
        h.step(Input::Voice(VoiceEvent::SpeechStarted { turn: late, source: 0, replayed: true }));
        let mut last = Vec::new();
        for window in 1..=3 {
            h.wait(32);
            last = h.step(Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
        }
        assert!(last.contains(&Effect::Audio(AudioCmd::Pause { speak })), "{partial}: paused");
        let (timer, at) = timer_set(&last).expect("the decision timer");
        h.step(Input::Voice(VoiceEvent::Partial {
            turn: late,
            text: Personal::new(partial.to_owned()),
            stable: false,
        }));
        h.now_ms = at.0 / 1_000_000;
        let effects = h.step(Input::Timer(timer));
        assert_eq!(
            effects.contains(&Effect::Audio(AudioCmd::Stop { speak })),
            stops,
            "{partial}: {effects:?}"
        );
        assert_eq!(
            effects.contains(&Effect::Audio(AudioCmd::Resume { speak })),
            !stops,
            "{partial}: {effects:?}"
        );
    }
}

//! Barge-in through `Core::step`: the duck, the pause at the third window and the decision
//! after it (stop, a new addressed turn told what was heard, a backchannel, room talk at the
//! deadline), and "go on" after a stop.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::{AudioCmd, Effect, Input, PlaybackEvent};
use eileen_contracts::harness::{BrainCmd, BrainSlot};
use eileen_contracts::ids::{Personal, TurnId};
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};

use super::{Harness, ask_brain, brain_turn, delta, events, spoken, timer_set};

#[test]
fn stop_while_eileen_speaks_ducks_pauses_and_stops() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let speak = spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 30, samples: 1_000 }));
    h.step(Input::Playback(PlaybackEvent::Played { speak, clause: 0, samples_played: 500 }));
    let barge = |window| Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 });
    let effects = h.step(barge(1));
    assert!(
        effects.contains(&Effect::Audio(AudioCmd::Duck { db: -20 })),
        "ducked at the first window"
    );
    h.wait(32);
    h.step(barge(2));
    h.wait(32);
    let effects = h.step(barge(3));
    assert!(effects.contains(&Effect::Audio(AudioCmd::Pause { speak })), "paused at the third");
    h.wait(200);
    let effects = h.say("stop");
    assert!(effects.contains(&Effect::Audio(AudioCmd::Stop { speak })), "playback stops");
    assert!(
        effects.contains(&Effect::Speech(SpeechCmd::Cancel { speak })),
        "synthesis is cancelled"
    );
    assert!(
        effects.contains(&Effect::Brain(BrainCmd::Interrupt { slot: BrainSlot::Primary })),
        "the brain turn is interrupted"
    );
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SpeakingStopped { interrupted: true, heard_chars: 15, .. }
        )),
        "half of the first clause was heard: {effects:?}"
    );
}

#[test]
fn an_addressed_barge_in_is_taken_and_the_brain_is_told_what_was_heard() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let speak = spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 30, samples: 1_000 }));
    h.step(Input::Playback(PlaybackEvent::Played { speak, clause: 0, samples_played: 500 }));
    for window in 1..=3 {
        h.step(Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
    let effects = h.say("Eileen, what about lakes?");
    let (_, message) = brain_turn(&effects).expect("a new brain turn");
    assert!(message.contains("[interrupted] they heard: \"Rivers carry wa\""), "{message}");
}

#[test]
fn a_backchannel_resumes_from_the_paused_sample() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let speak = spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
    for window in 1..=3 {
        h.step(Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
    let effects = h.say("mm-hm");
    assert_eq!(
        effects,
        [
            Effect::Audio(AudioCmd::Resume { speak }),
            Effect::Speech(SpeechCmd::Resume { speak }),
            Effect::Audio(AudioCmd::Unduck),
        ],
        "resumed and unducked, and nothing on the bus"
    );
}

#[test]
fn a_pause_with_nothing_addressed_resumes_at_the_deadline() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let speak = spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
    let mut last = Vec::new();
    for window in 1..=3 {
        // Speech windows come 32 ms apart, so the pause re-arms the duck's timer for later.
        h.wait(32);
        last = h.step(Input::Voice(VoiceEvent::BargeInWindow { speak, window, prob_pct: 90 }));
    }
    let (timer, at) = timer_set(&last).expect("the decision timer");
    h.step(Input::Voice(VoiceEvent::Partial {
        turn: TurnId::from_parts(5, 5),
        text: Personal::new("so anyway I was".to_owned()),
        stable: false,
    }));
    assert!(at.0 - h.now_ms * 1_000_000 <= 600_000_000, "within 600 ms of the pause");
    h.now_ms = at.0 / 1_000_000;
    let effects = h.step(Input::Timer(timer));
    assert!(
        effects.contains(&Effect::Audio(AudioCmd::Resume { speak })),
        "room talk resumes: {effects:?}"
    );
}

#[test]
fn go_on_plays_the_rest_after_a_stop() {
    let mut h = Harness::new();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let speak = spoken(&h.step(delta(turn, "Rivers carry water to the sea. They "))).remove(0).0;
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 30, samples: 1_000 }));
    h.step(Input::Playback(PlaybackEvent::Played { speak, clause: 0, samples_played: 500 }));
    h.say("stop");
    let effects = h.say("go on");
    assert_eq!(
        spoken(&effects).first().map(|s| s.1.clone()),
        Some(vec!["water to the sea.".to_owned()]),
        "from the start of the word that was cut"
    );
}

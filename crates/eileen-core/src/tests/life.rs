//! The speech process's life through `Core::step`: the start on the first step, the 120 s load
//! deadline, the 5 s heartbeat deadline, a crash, the backoff and its reset, the wait for a
//! killed process's exit, the turn open when speech went down, and replies on the HUD while
//! speech is down or the microphone is muted, with the HUD line's limit.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use eileen_contracts::core_io::{Effect, Input, PlaybackEvent, TimerId, UserInput};
use eileen_contracts::hud::{HudStateLine, MAX_LINE_BYTES, MicState};
use eileen_contracts::ids::{MonoTime, Personal, TurnId};
use eileen_contracts::speech::VoiceEvent;

use super::{
    Harness, ask_brain, brain_turn, delta, done, end_of_turn, events, ports, ready, speech, spoken,
};
use crate::hud::LINE_MAX_BYTES;
use crate::turn::Phase;
use crate::{Core, CoreConfig};

const START: Effect = Effect::Sidecar(SidecarId::Speech, SidecarCmd::Start);
const RESTART: Effect = Effect::Sidecar(SidecarId::Speech, SidecarCmd::Restart);
const KILL: Effect = Effect::Sidecar(SidecarId::Speech, SidecarCmd::HangDumpThenKill);

/// The timer set in `effects` for `at_ms` of virtual time.
fn timer_at(effects: &[Effect], at_ms: u64) -> Option<TimerId> {
    effects.iter().find_map(|effect| {
        if let Effect::SetTimer { id, at } = effect
            && *at == MonoTime(at_ms * 1_000_000)
        {
            Some(*id)
        } else {
            None
        }
    })
}

/// Lets time pass to `at_ms` and fires the timer `id`.
fn fire(h: &mut Harness, id: TimerId, at_ms: u64) -> Vec<Effect> {
    h.now_ms = at_ms;
    h.step(Input::Timer(id))
}

fn typed(text: &str) -> Input {
    Input::User(UserInput::Typed { text: Personal::new(text.to_owned()) })
}

const fn exited() -> Input {
    speech(SidecarEvent::Exited { code: Some(1) })
}

/// A harness whose speech process went `Ready` at 1 s; returns the heartbeat check's timer.
fn up() -> (Harness, TimerId) {
    let mut h = Harness::booting(CoreConfig::default(), Vec::new());
    h.step(Input::User(UserInput::SetMute(false)));
    let effects = h.step(speech(ready()));
    let check = timer_at(&effects, 6_000).expect("the 5 s heartbeat check is set at `Ready`");
    (h, check)
}

#[test]
fn the_first_step_starts_speech_and_the_hud_says_loading() {
    let mut h = Harness::booting(CoreConfig::default(), Vec::new());
    let effects = h.step(Input::User(UserInput::SetMute(false)));
    assert_eq!(effects.first(), Some(&START), "`Start` first, whatever the input: {effects:?}");
    assert!(timer_at(&effects, 121_000).is_some(), "120 s to be ready: {effects:?}");
    assert_eq!(h.mic(), Some(&MicState::LoadingSpeech), "the HUD says loading");
    let effects = h.step(typed("what time is it"));
    assert!(!effects.contains(&START), "started once");
    assert!(spoken(&effects).is_empty(), "nothing is spoken before `Ready`: {effects:?}");
    assert!(h.hud_line().is_some_and(|line| line.starts_with("It's ")), "{:?}", h.hud_line());
    h.step(speech(SidecarEvent::Heartbeat));
    assert_eq!(h.mic(), Some(&MicState::LoadingSpeech), "a heartbeat is no `Ready`");
    h.step(speech(ready()));
    assert_eq!(h.mic(), Some(&MicState::Listening), "listening once ready");
}

#[test]
fn no_ready_within_120_s_dumps_kills_and_restarts_after_a_second() {
    let mut h = Harness::booting(CoreConfig::default(), Vec::new());
    let effects = h.step(Input::User(UserInput::SetMute(false)));
    let deadline = timer_at(&effects, 121_000).unwrap();
    let effects = fire(&mut h, deadline, 121_000);
    assert_eq!(effects.first(), Some(&KILL), "hang-dumped and killed: {effects:?}");
    assert!(!effects.contains(&RESTART), "not restarted at once");
    let restart = timer_at(&effects, 122_000).expect("the first backoff step is 1 s");
    assert_eq!(h.mic(), Some(&MicState::SpeechRestarting), "the HUD says restarting");
    // The killed process's exit is the core's own doing.
    let effects = h.step(exited());
    assert!(effects.is_empty(), "nothing more for the exit of a killed process: {effects:?}");
    let effects = fire(&mut h, restart, 122_000);
    assert_eq!(effects.first(), Some(&RESTART), "{effects:?}");
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SidecarRestarted { which: SidecarId::Speech, reason } if reason == "not ready within 120 s"
        )),
        "{effects:?}"
    );
    assert!(timer_at(&effects, 242_000).is_some(), "120 s again: {effects:?}");
}

#[test]
fn a_heartbeat_five_seconds_late_dumps_kills_and_restarts() {
    let (mut h, check) = up();
    for second in 2..=4 {
        h.now_ms = second * 1_000;
        h.step(speech(SidecarEvent::Heartbeat));
    }
    let effects = fire(&mut h, check, 6_000);
    assert!(!effects.contains(&KILL), "the last heartbeat was 2 s ago: {effects:?}");
    let check = timer_at(&effects, 9_000).expect("checked again 5 s after the last heartbeat");
    let effects = fire(&mut h, check, 9_000);
    assert_eq!(effects.first(), Some(&KILL), "5 s with no heartbeat: {effects:?}");
    assert!(timer_at(&effects, 10_000).is_some(), "restarted after 1 s: {effects:?}");
    assert_eq!(h.mic(), Some(&MicState::SpeechRestarting));
}

#[test]
fn an_exit_restarts_without_a_dump() {
    let (mut h, _) = up();
    h.now_ms = 3_000;
    let effects = h.step(exited());
    assert!(!effects.contains(&KILL), "no dump for a process that exited: {effects:?}");
    let restart = timer_at(&effects, 4_000).expect("restarted after 1 s");
    let effects = fire(&mut h, restart, 4_000);
    assert_eq!(effects.first(), Some(&RESTART), "{effects:?}");
    h.step(speech(ready()));
    assert_eq!(h.mic(), Some(&MicState::Listening), "back");
}

#[test]
fn the_backoff_doubles_and_starts_over_after_a_minute_up() {
    let (mut h, _) = up();
    let mut steps = Vec::new();
    for _ in 0..4 {
        let down = h.now_ms;
        let effects = h.step(exited());
        let (id, at) = effects
            .iter()
            .find_map(|e| {
                if let Effect::SetTimer { id, at } = e
                    && at.0 > down * 1_000_000
                {
                    Some((*id, *at))
                } else {
                    None
                }
            })
            .expect("a restart timer");
        let at_ms = at.0 / 1_000_000;
        steps.push((at_ms - down) / 1_000);
        fire(&mut h, id, at_ms);
        h.step(speech(ready()));
    }
    assert_eq!(steps, [1, 2, 4, 8], "1 s, doubling");
    h.now_ms += 60_000;
    let down = h.now_ms;
    let effects = h.step(exited());
    assert!(timer_at(&effects, down + 1_000).is_some(), "a minute up resets it: {effects:?}");
}

#[test]
fn a_reply_while_speech_is_down_goes_to_the_hud() {
    let (mut h, _) = up();
    h.step(exited());
    let effects = h.step(typed("what time is it"));
    assert!(spoken(&effects).is_empty(), "not spoken: {effects:?}");
    assert!(h.hud_line().is_some_and(|line| line.starts_with("It's ")), "{:?}", h.hud_line());
    let effects = h.step(typed("tell me about rivers"));
    let (turn, _) = brain_turn(&effects).expect("the brain is asked");
    let effects = h.step(delta(turn, "Rivers carry water to the sea. They shape "));
    assert!(spoken(&effects).is_empty(), "not spoken: {effects:?}");
    assert_eq!(h.hud_line(), Some("Rivers carry water to the sea."), "a new line");
    h.step(delta(turn, "valleys."));
    h.step(done(turn));
    assert_eq!(
        h.hud_line(),
        Some("Rivers carry water to the sea. They shape valleys."),
        "the rest is added to it"
    );
}

#[test]
fn a_typed_request_while_muted_is_answered_on_the_hud() {
    let (mut h, _) = up();
    h.step(Input::User(UserInput::SetMute(true)));
    assert_eq!(h.mic(), Some(&MicState::Muted));
    let effects = h.step(typed("what time is it"));
    assert!(spoken(&effects).is_empty(), "nothing spoken while muted: {effects:?}");
    assert!(h.hud_line().is_some_and(|line| line.starts_with("It's ")), "{:?}", h.hud_line());
    h.step(Input::User(UserInput::SetMute(false)));
    let effects = h.say("Eileen, what time is it?");
    assert_eq!(spoken(&effects).len(), 1, "spoken again once unmuted: {effects:?}");
    assert_eq!(h.hud_line(), None, "and the line is cleared");
}

#[test]
fn what_speech_had_not_synthesized_goes_to_the_hud_and_the_open_turn_is_dropped() {
    let (mut h, _) = up();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let effects = h.step(delta(turn, "Rivers carry water to the sea. They shape valleys. "));
    let (speak, _) = spoken(&effects).remove(0);
    h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause: 0, chars: 30, samples: 900 }));
    let open = TurnId::from_parts(5, 5);
    h.step(Input::Voice(VoiceEvent::SpeechStarted { turn: open, source: 0, replayed: false }));
    let effects = h.step(exited());
    assert!(
        !events(&effects).iter().any(|e| matches!(e, BusEvent::SpeakingStopped { .. })),
        "the synthesized clause still plays: {effects:?}"
    );
    assert_eq!(h.hud_line(), Some("They shape valleys."), "the rest is on the HUD");
    h.step(delta(turn, "And they feed lakes."));
    h.step(done(turn));
    assert_eq!(
        h.hud_line(),
        Some("They shape valleys. And they feed lakes."),
        "the rest of the reply follows it"
    );
    let effects = h.step(Input::Playback(PlaybackEvent::Drained { speak }));
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SpeakingStopped { speak: s, interrupted: false, .. } if *s == speak
        )),
        "it ends once played: {effects:?}"
    );
    assert_eq!(h.core.turns().phase(), Phase::Idle, "the open turn was dropped, not left open");
}

#[test]
fn a_reply_the_speech_process_made_no_clause_of_goes_to_the_hud() {
    let (mut h, _) = up();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let effects = h.step(delta(turn, "Rivers carry water to the sea. They "));
    let (speak, _) = spoken(&effects).remove(0);
    let effects = h.step(Input::Voice(VoiceEvent::SpeakDone { speak }));
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SpeakingStopped { speak: s, heard_chars: 0, .. } if *s == speak
        )),
        "it ends unheard: {effects:?}"
    );
    assert_eq!(h.hud_line(), Some("Rivers carry water to the sea."), "{:?}", h.hud_line());
    let effects = h.step(delta(turn, "shape valleys."));
    assert!(spoken(&effects).is_empty(), "the rest is not spoken: {effects:?}");
    h.step(done(turn));
    assert_eq!(h.hud_line(), Some("Rivers carry water to the sea. They shape valleys."));
}

#[test]
fn an_exit_that_arrives_after_the_backoff_step_restarts_once() {
    let (mut h, check) = up();
    let effects = fire(&mut h, check, 6_000);
    assert_eq!(effects.first(), Some(&KILL), "{effects:?}");
    let step = timer_at(&effects, 7_000).expect("the first backoff step is 1 s");
    // The hang dump outlasts the step: the killed process has not been reported gone.
    let effects = fire(&mut h, step, 7_000);
    assert!(!effects.contains(&RESTART), "no restart before the kill's exit: {effects:?}");
    assert!(timer_at(&effects, 16_000).is_some(), "waited for at most 10 s: {effects:?}");
    h.now_ms = 9_000;
    let effects = h.step(exited());
    assert_eq!(effects.first(), Some(&RESTART), "restarted with the exit: {effects:?}");
    let effects = h.step(speech(ready()));
    assert!(!effects.contains(&RESTART), "{effects:?}");
    assert_eq!(h.mic(), Some(&MicState::Listening), "the new process is kept");
}

#[test]
fn a_kill_whose_exit_never_comes_restarts_after_10_s() {
    let (mut h, check) = up();
    let effects = fire(&mut h, check, 6_000);
    let step = timer_at(&effects, 7_000).unwrap();
    let effects = fire(&mut h, step, 7_000);
    let wait = timer_at(&effects, 16_000).unwrap();
    let effects = fire(&mut h, wait, 16_000);
    assert_eq!(effects.first(), Some(&RESTART), "{effects:?}");
}

#[test]
fn an_exit_within_the_backoff_step_restarts_at_the_step() {
    let (mut h, check) = up();
    let effects = fire(&mut h, check, 6_000);
    let step = timer_at(&effects, 7_000).unwrap();
    h.now_ms = 6_050;
    let effects = h.step(exited());
    assert!(effects.is_empty(), "the kill's exit changes nothing yet: {effects:?}");
    let effects = fire(&mut h, step, 7_000);
    assert_eq!(effects.first(), Some(&RESTART), "{effects:?}");
}

#[test]
fn a_turn_open_when_speech_went_down_is_never_routed() {
    let (mut h, _) = up();
    let open = TurnId::from_parts(5, 5);
    h.step(Input::Voice(VoiceEvent::SpeechStarted { turn: open, source: 0, replayed: false }));
    h.step(exited());
    let late = [
        Input::Voice(VoiceEvent::SpeechStarted { turn: open, source: 0, replayed: false }),
        Input::Voice(end_of_turn(open, "Eileen, what time is it?")),
    ];
    for input in late {
        let effects = h.step(input);
        assert!(
            !events(&effects).iter().any(|e| matches!(e, BusEvent::TurnRouted { .. })),
            "the dropped turn is not routed: {effects:?}"
        );
        assert!(brain_turn(&effects).is_none(), "{effects:?}");
    }
    assert_eq!(h.hud_line(), None, "and not answered on the HUD");
    assert_eq!(h.core.turns().phase(), Phase::Idle, "nor heard");
}

#[test]
fn muting_mid_reply_shows_the_unheard_rest_and_the_rest_of_the_reply_on_the_hud() {
    let (mut h, _) = up();
    let turn = ask_brain(&mut h, "Eileen, tell me about rivers");
    let effects = h.step(delta(turn, "Rivers carry water to the sea. They shape valleys. "));
    let (speak, _) = spoken(&effects).remove(0);
    for clause in 0..2 {
        h.step(Input::Voice(VoiceEvent::ClauseReady { speak, clause, chars: 20, samples: 900 }));
    }
    // The first clause has played; the second is about to.
    h.step(Input::Playback(PlaybackEvent::Played { speak, clause: 1, samples_played: 0 }));
    let effects = h.step(Input::User(UserInput::SetMute(true)));
    assert!(
        events(&effects).iter().any(|e| matches!(
            e,
            BusEvent::SpeakingStopped { speak: s, interrupted: true, .. } if *s == speak
        )),
        "muting stops it: {effects:?}"
    );
    assert_eq!(h.hud_line(), Some("They shape valleys."), "what was not heard is on the HUD");
    let effects = h.step(delta(turn, "And they feed lakes."));
    assert!(spoken(&effects).is_empty(), "nothing spoken while muted: {effects:?}");
    h.step(done(turn));
    assert_eq!(
        h.hud_line(),
        Some("They shape valleys. And they feed lakes."),
        "the rest of the reply follows it"
    );
}

#[test]
fn a_reply_longer_than_the_hud_line_is_cut_and_sent_no_more_once_full() {
    let (mut h, _) = up();
    h.step(Input::User(UserInput::SetMute(true)));
    let effects = h.step(typed("tell me about rivers"));
    let (turn, _) = brain_turn(&effects).expect("the brain is asked");
    let clause = "Rivers carry water from the hills down to the sea, through every valley. ";
    let mut streamed = 0;
    let mut sent_when_full = None;
    while streamed <= MAX_LINE_BYTES + clause.len() {
        h.step(delta(turn, clause));
        streamed += clause.len();
        let state = h.hud.clone().expect("a HUD state");
        let line = state.speaking.clone().unwrap_or_default();
        assert!(line.len() <= LINE_MAX_BYTES, "{} bytes after {streamed}", line.len());
        let wire = serde_json::to_string(&HudStateLine { v: 1, state }).unwrap();
        assert!(wire.len() < MAX_LINE_BYTES, "{} bytes on the wire", wire.len());
        if line.ends_with(" ...") && sent_when_full.is_none() {
            sent_when_full = Some(h.hud_sent);
        }
    }
    h.step(done(turn));
    assert!(sent_when_full.is_some(), "the line was cut");
    assert_eq!(Some(h.hud_sent), sent_when_full, "a full line is not sent again");
}

#[test]
fn a_core_resumed_from_a_snapshot_starts_speech_again() {
    let (h, _) = up();
    let snapshot = h.core.snapshot();
    let mut resumed = Harness::booting(CoreConfig::default(), Vec::new());
    resumed.core = Core::new(CoreConfig::default(), ports(Vec::new()), Some(snapshot));
    let effects = resumed.step(speech(SidecarEvent::Heartbeat));
    assert_eq!(effects.first(), Some(&START), "the life is not in the snapshot: {effects:?}");
    assert_eq!(resumed.mic(), Some(&MicState::LoadingSpeech));
}

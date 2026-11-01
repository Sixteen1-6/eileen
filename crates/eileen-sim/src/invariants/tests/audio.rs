//! Invariants 2, 3 and 9 on hand-built audio traces that break them, and on ones that keep them.

use eileen_contracts::bus::{BusEvent, SpeakCause};
use eileen_contracts::core_io::{ActivityEvent, AudioCmd, Earcon, Effect, Input};
use eileen_contracts::harness::{BrainCmd, BrainSlot};
use eileen_contracts::ids::{SpeakId, TurnId};
use eileen_contracts::jobs::TurnAuthority;
use eileen_contracts::speech::{SpeechCmd, VoiceEvent};

use super::super::{
    AudioEvent, AudioRecord, HEARD_MARKER, Handed, barge_in_pauses_in_time,
    heard_prefix_matches_playback, nothing_sounds_while_muted,
};

const S: SpeakId = SpeakId::from_parts(1, 1);

fn effect(at_ms: u64, effect: Effect) -> AudioRecord {
    AudioRecord { at_ms, event: AudioEvent::Effect(effect) }
}

fn input(at_ms: u64, input: Input) -> AudioRecord {
    AudioRecord { at_ms, event: AudioEvent::Input(input) }
}

fn audio(at_ms: u64, cmd: AudioCmd) -> AudioRecord {
    effect(at_ms, Effect::Audio(cmd))
}

fn speaking(at_ms: u64) -> AudioRecord {
    let text = "One two three. Four five.".to_owned();
    effect(at_ms, Effect::Emit(BusEvent::Speaking { speak: S, cause: SpeakCause::Reply, text }))
}

fn stopped(at_ms: u64, heard_chars: u32, interrupted: bool) -> AudioRecord {
    let event = BusEvent::SpeakingStopped { speak: S, heard_chars, interrupted, resumed: false };
    effect(at_ms, Effect::Emit(event))
}

fn window(at_ms: u64, window: u8) -> AudioRecord {
    input(at_ms, Input::Voice(VoiceEvent::BargeInWindow { speak: S, window, prob_pct: 90 }))
}

/// A callback at `at_ms` that hands `samples` of clause `clause`, or nothing for 0.
fn callback(at_ms: u64, clause: u16, samples: u32) -> AudioRecord {
    let handed = if samples == 0 { Vec::new() } else { vec![Handed { speak: S, clause, samples }] };
    AudioRecord { at_ms, event: AudioEvent::Callback(handed) }
}

/// Whether `record` is the effect `cmd`.
fn is(record: &AudioRecord, cmd: &AudioCmd) -> bool {
    matches!(&record.event, AudioEvent::Effect(Effect::Audio(found)) if found == cmd)
}

/// Eileen speaks; three windows at the speech process's pace duck at the first and pause at
/// the third, and the callback after the pause plays nothing.
fn paused_in_time() -> Vec<AudioRecord> {
    vec![
        speaking(0),
        callback(10, 0, 240),
        window(32, 1),
        audio(32, AudioCmd::Duck { db: -20 }),
        callback(40, 0, 240),
        window(64, 2),
        callback(70, 0, 240),
        window(96, 3),
        audio(96, AudioCmd::Pause { speak: S }),
        callback(100, 0, 0),
    ]
}

#[test]
fn invariant_2_holds_for_a_duck_a_pause_in_time_and_a_resume() {
    let mut trace = paused_in_time();
    trace.extend([
        audio(400, AudioCmd::Resume { speak: S }),
        audio(400, AudioCmd::Unduck),
        callback(410, 0, 240),
    ]);
    assert_eq!(barge_in_pauses_in_time(&trace, 410), Ok(()), "{trace:?}");
}

#[test]
fn invariant_2_fails_on_a_window_not_ducked_by_the_next_callback() {
    let trace = vec![speaking(0), window(32, 1), callback(40, 0, 240)];
    let error = barge_in_pauses_in_time(&trace, 40).unwrap_err();
    assert_eq!(error.number, 2, "{error}");
    assert!(error.detail.contains("not ducked"), "{error}");
}

#[test]
fn invariant_2_fails_on_a_third_window_not_paused_by_the_next_callback() {
    let mut trace = paused_in_time();
    trace.retain(|record| !is(record, &AudioCmd::Pause { speak: S }));
    let error = barge_in_pauses_in_time(&trace, 100).unwrap_err();
    assert!(error.detail.contains("not paused"), "{error}");
}

#[test]
fn invariant_2_fails_when_silence_comes_more_than_150_ms_after_the_onset() {
    let trace = vec![
        speaking(0),
        window(32, 1),
        audio(32, AudioCmd::Duck { db: -20 }),
        window(64, 2),
        window(96, 3),
        audio(96, AudioCmd::Pause { speak: S }),
        callback(200, 0, 0),
    ];
    let error = barge_in_pauses_in_time(&trace, 200).unwrap_err();
    assert!(error.detail.contains("fell silent at 200 ms, 200 ms later"), "{error}");
}

#[test]
fn invariant_2_fails_on_playback_while_paused() {
    let mut trace = paused_in_time();
    trace.push(callback(110, 0, 240));
    let error = barge_in_pauses_in_time(&trace, 110).unwrap_err();
    assert!(error.detail.contains("still played"), "{error}");
}

#[test]
fn invariant_2_fails_on_a_pause_left_undecided_past_600_ms() {
    let trace = paused_in_time();
    assert_eq!(barge_in_pauses_in_time(&trace, 696), Ok(()), "600 ms after the pause: in time");
    let error = barge_in_pauses_in_time(&trace, 697).unwrap_err();
    assert!(error.detail.contains("neither resumed nor stopped"), "{error}");
}

/// Two clauses of 1,000 samples each: the first played in full, then half of the second.
fn interrupted_halfway() -> Vec<AudioRecord> {
    let clauses = vec!["One two three.".to_owned(), "Four five.".to_owned()];
    let whole_line = clauses.join(" ");
    let speak = SpeechCmd::Speak { speak: S, voice: String::new(), clauses, whole_line };
    let ready = |clause: u16, chars: u32| {
        input(0, Input::Voice(VoiceEvent::ClauseReady { speak: S, clause, chars, samples: 1_000 }))
    };
    vec![
        speaking(0),
        effect(0, Effect::Speech(speak)),
        ready(0, 14),
        ready(1, 10),
        callback(10, 0, 1_000),
        callback(20, 1, 500),
    ]
}

fn brain_turn(at_ms: u64, heard: Option<&str>) -> AudioRecord {
    let message = match heard {
        Some(heard) => format!("[heard] what about rivers\n        {HEARD_MARKER}{heard}\""),
        None => "[heard] what about rivers".to_owned(),
    };
    let turn = TurnId::from_parts(2, 2);
    let (slot, authority) = (BrainSlot::Primary, TurnAuthority::Addressed);
    effect(at_ms, Effect::Brain(BrainCmd::Turn { slot, turn, message, authority }))
}

#[test]
fn invariant_3_holds_when_the_brain_is_told_the_played_prefix() {
    let mut trace = interrupted_halfway();
    trace.extend([stopped(30, 19, true), brain_turn(30, Some("One two three. Four"))]);
    assert_eq!(heard_prefix_matches_playback(&trace), Ok(()), "{trace:?}");
}

#[test]
fn invariant_3_fails_when_the_brain_is_told_more_than_was_played() {
    let mut trace = interrupted_halfway();
    trace.extend([stopped(30, 19, true), brain_turn(30, Some("One two three. Four five."))]);
    let error = heard_prefix_matches_playback(&trace).unwrap_err();
    assert_eq!(error.number, 3, "{error}");
    assert!(error.detail.contains("One two three. Four five."), "{error}");
}

#[test]
fn invariant_3_fails_when_the_stop_counts_other_characters() {
    let mut trace = interrupted_halfway();
    trace.push(stopped(30, 24, true));
    let error = heard_prefix_matches_playback(&trace).unwrap_err();
    assert!(error.detail.contains("24 characters heard"), "{error}");
}

#[test]
fn invariant_3_fails_when_the_brain_is_told_of_an_interruption_that_did_not_happen() {
    let mut trace = interrupted_halfway();
    trace.extend([stopped(30, 25, false), brain_turn(30, Some("One two three."))]);
    let error = heard_prefix_matches_playback(&trace).unwrap_err();
    assert!(error.detail.contains("carry None"), "{error}");
}

/// Muting the way the turn-taking machine does it: the earcon, capture off, then the change.
fn muted() -> Vec<AudioRecord> {
    vec![
        audio(0, AudioCmd::Earcon(Earcon::Muted)),
        audio(0, AudioCmd::SetCapture(false)),
        effect(0, Effect::Emit(BusEvent::MuteChanged { muted: true })),
        callback(10, 0, 0),
    ]
}

#[test]
fn invariant_9_holds_for_a_mute_and_an_unmute_with_their_earcons() {
    let mut trace = muted();
    trace.extend([
        audio(20, AudioCmd::SetCapture(true)),
        audio(20, AudioCmd::Earcon(Earcon::Unmuted)),
        effect(20, Effect::Emit(BusEvent::MuteChanged { muted: false })),
        speaking(20),
        callback(30, 0, 240),
    ]);
    assert_eq!(nothing_sounds_while_muted(&trace), Ok(()), "{trace:?}");
}

#[test]
fn invariant_9_fails_when_capture_is_left_on_past_the_next_callback() {
    let mut trace = muted();
    trace.retain(|record| !is(record, &AudioCmd::SetCapture(false)));
    let error = nothing_sounds_while_muted(&trace).unwrap_err();
    assert_eq!(error.number, 9, "{error}");
    assert!(error.detail.contains("still captured"), "{error}");
}

#[test]
fn invariant_9_fails_on_speech_or_an_earcon_while_muted() {
    let mut speech = muted();
    speech.push(speaking(20));
    let error = nothing_sounds_while_muted(&speech).unwrap_err();
    assert!(error.detail.contains("began at 20 ms while muted"), "{error}");
    let mut earcon = muted();
    earcon.push(audio(20, AudioCmd::Earcon(Earcon::Routed)));
    let error = nothing_sounds_while_muted(&earcon).unwrap_err();
    assert!(error.detail.contains("Routed earcon"), "{error}");
}

#[test]
fn invariant_9_fails_on_samples_played_while_another_app_uses_the_microphone() {
    let trace = vec![
        input(0, Input::Activity(ActivityEvent::MicInUseElsewhere(true))),
        callback(10, 0, 240),
    ];
    let error = nothing_sounds_while_muted(&trace).unwrap_err();
    assert!(error.detail.contains("another app used the microphone"), "{error}");
}

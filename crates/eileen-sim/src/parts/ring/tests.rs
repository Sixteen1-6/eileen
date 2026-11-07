use eileen_contracts::connectors::{SidecarCmd, SidecarEvent, SidecarId};
use eileen_contracts::core_io::Input;
use eileen_contracts::ids::{Personal, SpeakId, TurnId};
use eileen_contracts::speech::{TurnEndReason, VoiceEvent};

use super::{RING_MS, Ring, turn_of};
use crate::parts::{Due, LOAD_MS, Process, Speech};

fn turn(n: u64) -> TurnId {
    TurnId::from_parts(n, u128::from(n))
}

const fn started(turn: TurnId) -> VoiceEvent {
    VoiceEvent::SpeechStarted { turn, source: 0, replayed: false }
}

fn partial(turn: TurnId, text: &str) -> VoiceEvent {
    VoiceEvent::Partial { turn, text: Personal::new(text.to_owned()), stable: false }
}

fn ended(turn: TurnId, text: &str) -> VoiceEvent {
    VoiceEvent::EndOfTurn {
        turn,
        text: Personal::new(text.to_owned()),
        smart_turn_pct: 90,
        silence_ms: 300,
        asr_confidence_pct: 95,
        reason: TurnEndReason::SmartTurn,
        source: 0,
        level_db: -30,
    }
}

/// Each event's turn and kind, with whether a start was replayed.
fn shape(events: &[VoiceEvent]) -> Vec<(TurnId, &'static str)> {
    events
        .iter()
        .map(|event| {
            let kind = match event {
                VoiceEvent::SpeechStarted { replayed: true, .. } => "replayed start",
                VoiceEvent::SpeechStarted { replayed: false, .. } => "live start",
                VoiceEvent::Partial { .. } => "partial",
                VoiceEvent::SpeechEnded { .. } => "speech ended",
                VoiceEvent::EndOfTurn { .. } => "end",
                VoiceEvent::Calibration(_)
                | VoiceEvent::BargeInWindow { .. }
                | VoiceEvent::ClauseReady { .. }
                | VoiceEvent::SpeakDone { .. }
                | VoiceEvent::Embedding { .. }
                | VoiceEvent::CheckResult { .. }
                | VoiceEvent::SpeakerMatch { .. }
                | VoiceEvent::Health { .. } => "other",
            };
            (turn_of(event).unwrap_or_else(|| turn(0)), kind)
        })
        .collect()
}

#[test]
fn a_turn_held_while_the_process_is_not_ready_is_replayed_under_a_new_turn() {
    let mut ring = Ring::default();
    let said = turn(1);
    ring.hold(100, started(said));
    ring.hold(300, partial(said, "Eileen, what"));
    ring.hold(500, ended(said, "Eileen, what time is it?"));
    let replayed = ring.replay(2_000);
    let name = turn_of(&replayed[0]).unwrap();
    assert_ne!(name, said, "the process mints a turn of its own");
    assert_eq!(
        shape(&replayed),
        [(name, "replayed start"), (name, "partial"), (name, "end")],
        "the turn began in a replayed frame"
    );
    assert!(
        matches!(&replayed[2], VoiceEvent::EndOfTurn { text, .. } if text.get() == "Eileen, what time is it?"),
        "the words are kept: {replayed:?}"
    );
    assert!(ring.replay(2_000).is_empty(), "replayed once");
    let next = turn(2);
    assert_eq!(ring.live(started(next)), started(next), "speech heard live is as it was");
}

#[test]
fn a_turn_open_when_the_process_went_down_comes_back_whole_under_a_new_turn() {
    let mut ring = Ring::default();
    let said = turn(1);
    assert_eq!(ring.live(started(said)), started(said), "heard live before the crash");
    ring.process_lost(200);
    ring.hold(600, ended(said, "Eileen, what time is it?"));
    let replayed = ring.replay(3_000);
    let name = turn_of(&replayed[0]).unwrap();
    assert_ne!(name, said, "the open turn is dropped by the core, so it comes back renamed");
    assert_eq!(shape(&replayed), [(name, "replayed start"), (name, "end")], "{replayed:?}");
}

#[test]
fn a_renamed_turn_finished_after_the_process_is_back_keeps_its_new_name() {
    let mut ring = Ring::default();
    let said = turn(1);
    ring.live(started(said));
    ring.process_lost(200);
    let replayed = ring.replay(2_200);
    let name = turn_of(&replayed[0]).unwrap();
    assert_eq!(shape(&replayed), [(name, "replayed start")], "its start, from the ring");
    let rest = ring.live(ended(said, "Eileen, stop"));
    assert_eq!(turn_of(&rest), Some(name), "the rest, heard live, is the same turn");
    let after = turn(2);
    assert_eq!(turn_of(&ring.live(ended(said, "again"))), Some(said), "the name ended with it");
    assert_eq!(ring.live(started(after)), started(after), "a new turn is heard as it is");
}

#[test]
fn a_held_turn_heard_on_live_keeps_its_name_until_it_ends() {
    let mut ring = Ring::default();
    let said = turn(1);
    ring.hold(1_000, started(said));
    let replayed = ring.replay(2_000);
    let name = turn_of(&replayed[0]).unwrap();
    assert_eq!(turn_of(&ring.live(partial(said, "Eileen"))), Some(name), "a partial");
    assert_eq!(turn_of(&ring.live(ended(said, "Eileen, stop"))), Some(name), "its end");
    ring.process_lost(3_000);
    ring.hold(3_500, started(turn(2)));
    let again = ring.replay(6_000);
    assert_eq!(again.len(), 1, "only the new turn, nothing of the finished one: {again:?}");
}

#[test]
fn the_ring_keeps_180_seconds_and_a_mute_empties_it() {
    let mut ring = Ring::default();
    ring.hold(0, started(turn(1)));
    ring.hold(1_000, ended(turn(1), "old"));
    ring.hold(150_000, started(turn(2)));
    ring.hold(151_000, ended(turn(2), "newer"));
    let replayed = ring.replay(RING_MS + 1_500);
    assert_eq!(replayed.len(), 2, "the first turn left the ring: {replayed:?}");
    assert!(
        matches!(&replayed[1], VoiceEvent::EndOfTurn { text, .. } if text.get() == "newer"),
        "{replayed:?}"
    );
    ring.hold(200_000, started(turn(3)));
    ring.clear();
    assert!(ring.replay(201_000).is_empty(), "a mute empties the ring");
}

/// The voice events due by `now_ms`, in order; anything else due then is a failure.
fn voice_due(due: &mut Due, now_ms: u64) -> Vec<VoiceEvent> {
    let inputs: Vec<Input> = std::iter::from_fn(|| due.pop(now_ms)).collect();
    inputs
        .into_iter()
        .map(|input| {
            let Input::Voice(event) = input else { panic!("only voice events are due") };
            event
        })
        .collect()
}

#[test]
fn the_speech_process_hears_live_once_ready_and_replays_its_ring_at_ready() {
    let mut speech = Speech::default();
    let mut due = Due::default();
    let said = turn(1);
    assert_eq!(speech.hear(0, started(said), true), None, "not started: the ring keeps it");
    speech.sidecar(0, SidecarCmd::Start, &mut due);
    assert_eq!(speech.hear(400, ended(said, "Eileen, stop"), true), None, "loading");
    assert_eq!(speech.hear(500, started(turn(2)), false), None, "capture is off: nothing kept");
    let ready = due.pop(LOAD_MS).expect("the load finishes");
    assert!(matches!(ready, Input::Sidecar(SidecarId::Speech, SidecarEvent::Ready { .. })));
    speech.delivered(LOAD_MS, &ready, &mut due);
    assert_eq!(speech.process(), Process::Up);
    let replayed = voice_due(&mut due, LOAD_MS);
    let name = turn_of(&replayed[0]).unwrap();
    assert_eq!(
        shape(&replayed),
        [(name, "replayed start"), (name, "end")],
        "recognised from the ring at `Ready`, before the next heartbeat"
    );
    let live = started(turn(3));
    assert_eq!(speech.hear(LOAD_MS + 10, live.clone(), true), Some(live), "heard live");
    let window =
        VoiceEvent::BargeInWindow { speak: SpeakId::from_parts(1, 1), window: 1, prob_pct: 90 };
    assert_eq!(speech.hear(LOAD_MS + 20, window.clone(), true), Some(window), "not a turn event");
    speech.crash(5_000, &mut due);
    assert_eq!(speech.hear(5_100, partial(turn(3), "and then"), true), None, "down: kept");
    speech.mute();
    speech.sidecar(6_000, SidecarCmd::Restart, &mut due);
    due.drop_where(|input| {
        matches!(input, Input::Sidecar(SidecarId::Speech, SidecarEvent::Exited { .. }))
    });
    let ready = due.pop(6_000 + LOAD_MS).expect("the load finishes");
    speech.delivered(6_000 + LOAD_MS, &ready, &mut due);
    assert!(voice_due(&mut due, 6_000 + LOAD_MS).is_empty(), "a mute emptied the ring");
}

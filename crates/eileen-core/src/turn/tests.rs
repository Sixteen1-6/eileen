use eileen_contracts::bus::SpeakCause;
use eileen_contracts::core_io::{AudioCmd, Earcon};
use eileen_contracts::ids::{MonoTime, SpeakId, TurnId};
use eileen_contracts::speech::SpeechCmd;

use super::{DECIDE_NS, DUCK_DB, Out, Phase, Turns, Verdict};

fn speak(n: u64) -> SpeakId {
    SpeakId::from_parts(n, u128::from(n))
}

fn turn(n: u64) -> TurnId {
    TurnId::from_parts(n, u128::from(n))
}

const fn ms(n: u64) -> MonoTime {
    MonoTime(n * 1_000_000)
}

/// A machine speaking `clauses` as utterance 1, every clause ready at 1,000 samples.
fn speaking(clauses: &[&str]) -> Turns {
    let mut turns = Turns::new(false);
    let mut out = Vec::new();
    let clauses: Vec<String> = clauses.iter().map(|c| (*c).to_owned()).collect();
    let count = clauses.len();
    assert!(turns.speak(speak(1), SpeakCause::Reply, "", clauses, &mut out), "it starts");
    for clause in 0..count {
        turns.clause_ready(speak(1), u16::try_from(clause).unwrap(), 1_000);
    }
    turns
}

fn stopped(out: &[Out]) -> Option<(u32, bool, bool)> {
    out.iter().find_map(|o| {
        if let Out::Stopped { heard_chars, interrupted, resumed, .. } = o {
            Some((*heard_chars, *interrupted, *resumed))
        } else {
            None
        }
    })
}

#[test]
fn the_phase_follows_the_floor() {
    let mut turns = Turns::new(false);
    assert_eq!(turns.phase(), Phase::Idle, "nobody speaks");
    turns.speech_started(turn(1));
    assert_eq!(turns.phase(), Phase::Hearing, "the user speaks");
    turns.end_of_turn();
    turns.think(turn(1));
    assert_eq!(turns.phase(), Phase::Thinking, "a brain turn is pending");
    let mut out = Vec::new();
    assert!(turns.speak(speak(1), SpeakCause::Reply, "", vec!["Hi.".to_owned()], &mut out), "ok");
    assert_eq!(turns.phase(), Phase::Speaking, "Eileen speaks");
    turns.thought(turn(1));
    turns.close(speak(1), &mut out);
    turns.clause_ready(speak(1), 0, 500);
    turns.drained(speak(1), &mut out);
    assert_eq!(turns.phase(), Phase::Idle, "drained and closed: the floor is free");
    assert!(turns.quiet(), "quiet");
}

#[test]
fn a_clone_shares_its_prefix_and_then_diverges() {
    let mut base = speaking(&["One two three.", "Four five six."]);
    let mut out = Vec::new();
    base.barge_in_window(ms(0), speak(1), 1, &mut out);
    let mut resumed = base.clone();
    let mut stopped_here = base.clone();
    assert_eq!(resumed, stopped_here, "clones start equal");
    resumed.resolve_barge_in(Verdict::Resume, &mut out);
    stopped_here.resolve_barge_in(Verdict::Stop, &mut out);
    assert_eq!(resumed.speaking(), Some(speak(1)), "the resumed clone still speaks");
    assert_eq!(stopped_here.speaking(), None, "the stopped clone is silent");
    assert!(base.barge_in(), "the original is untouched");
}

#[test]
fn barge_in_ducks_at_the_first_window_and_pauses_at_the_third_consecutive() {
    let mut turns = speaking(&["A long answer that goes on."]);
    let mut out = Vec::new();
    turns.barge_in_window(ms(1_000), speak(1), 4, &mut out);
    assert_eq!(
        out,
        [
            Out::BargeIn { speak: speak(1) },
            Out::Audio(AudioCmd::Duck { db: DUCK_DB }),
            Out::DecideBy(MonoTime(ms(1_000).0 + DECIDE_NS)),
        ],
        "duck at once and set the decision timer"
    );
    out.clear();
    turns.barge_in_window(ms(1_032), speak(1), 5, &mut out);
    assert!(out.is_empty(), "the second window changes nothing: {out:?}");
    turns.barge_in_window(ms(1_064), speak(1), 6, &mut out);
    assert_eq!(
        out,
        [
            Out::Audio(AudioCmd::Pause { speak: speak(1) }),
            Out::DecideBy(MonoTime(ms(1_064).0 + DECIDE_NS)),
        ],
        "the third consecutive window pauses and re-arms the decision"
    );
    assert!(turns.paused(), "paused");
}

#[test]
fn a_gap_in_the_windows_starts_the_count_again() {
    let mut turns = speaking(&["A long answer that goes on."]);
    let mut out = Vec::new();
    for window in [1, 2, 4, 5] {
        turns.barge_in_window(ms(0), speak(1), window, &mut out);
    }
    assert!(!turns.paused(), "1, 2, then 4, 5: never three in a row");
    turns.barge_in_window(ms(0), speak(1), 6, &mut out);
    assert!(turns.paused(), "4, 5, 6 pauses");
}

#[test]
fn a_duck_that_never_paused_is_undone_at_the_deadline() {
    let mut turns = speaking(&["A long answer."]);
    let mut out = Vec::new();
    turns.barge_in_window(ms(0), speak(1), 1, &mut out);
    out.clear();
    assert!(!turns.barge_in_deadline(&mut out), "nothing for the core to decide");
    assert_eq!(
        out,
        [Out::Audio(AudioCmd::Unduck), Out::BargeInEnded { speak: speak(1) }],
        "unducked"
    );
    assert!(!turns.barge_in(), "the barge-in is over");
}

#[test]
fn a_resume_continues_from_the_paused_sample_and_unducks() {
    let mut turns = speaking(&["One two three four.", "Five six."]);
    let mut out = Vec::new();
    turns.played(speak(1), 0, 600);
    for window in 1..=3 {
        turns.barge_in_window(ms(0), speak(1), window, &mut out);
    }
    turns.played(speak(1), 0, 640);
    assert!(turns.barge_in_deadline(&mut out), "a pause needs a verdict");
    out.clear();
    turns.resolve_barge_in(Verdict::Resume, &mut out);
    assert_eq!(
        out,
        [
            Out::Audio(AudioCmd::Resume { speak: speak(1) }),
            Out::Speech(SpeechCmd::Resume { speak: speak(1) }),
            Out::Audio(AudioCmd::Unduck),
            Out::BargeInEnded { speak: speak(1) },
        ],
        "resume playback and synthesis, then unduck"
    );
    out.clear();
    turns.played(speak(1), 1, 1_000);
    turns.close(speak(1), &mut out);
    turns.drained(speak(1), &mut out);
    assert_eq!(stopped(&out), Some((29, false, true)), "every character heard, resumed");
}

#[test]
fn a_stop_maps_the_samples_played_to_the_heard_prefix() {
    let mut turns = speaking(&["The weather today", "is sunny and warm."]);
    let mut out = Vec::new();
    turns.played(speak(1), 1, 500);
    turns.stop(true, &mut out);
    assert_eq!(
        out.first(),
        Some(&Out::Audio(AudioCmd::Stop { speak: speak(1) })),
        "playback stops first"
    );
    assert_eq!(
        out.get(1),
        Some(&Out::Speech(SpeechCmd::Cancel { speak: speak(1) })),
        "then synthesis is cancelled"
    );
    // "The weather today" (17) plus half of "is sunny and warm." (18 chars, 9 heard).
    assert_eq!(turns.take_heard().as_deref(), Some("The weather today is sunny"), "prefix");
    assert_eq!(stopped(&out), Some((26, true, false)), "26 characters heard, interrupted");
    assert_eq!(turns.take_rest().as_deref(), Some("and warm."), "the rest starts at the cut word");
    assert_eq!(turns.take_heard(), None, "the heard prefix is given once");
}

#[test]
fn an_utterance_waits_while_synthesis_is_behind() {
    let mut turns = Turns::new(false);
    let mut out = Vec::new();
    assert!(
        turns.speak(speak(1), SpeakCause::Reply, "", vec!["First.".to_owned()], &mut out),
        "ok"
    );
    turns.clause_ready(speak(1), 0, 100);
    turns.drained(speak(1), &mut out);
    assert_eq!(turns.speaking(), Some(speak(1)), "open: more clauses may come");
    turns.append(speak(1), "", vec!["Second.".to_owned()], &mut out);
    turns.close(speak(1), &mut out);
    turns.drained(speak(1), &mut out);
    assert_eq!(turns.speaking(), Some(speak(1)), "the second clause is not ready yet");
    turns.clause_ready(speak(1), 1, 100);
    turns.drained(speak(1), &mut out);
    assert_eq!(turns.speaking(), None, "closed, ready and drained: done");
}

#[test]
fn a_second_utterance_is_refused_while_one_holds_the_floor() {
    let mut turns = speaking(&["Busy."]);
    let mut out = Vec::new();
    assert!(
        !turns.speak(speak(2), SpeakCause::Announcement, "", vec!["Done.".to_owned()], &mut out),
        "two utterances never overlap"
    );
    assert!(out.is_empty(), "nothing emitted: {out:?}");
}

#[test]
fn muting_stops_speech_stops_capture_and_silences_everything() {
    let mut turns = speaking(&["Something long."]);
    let mut out = Vec::new();
    assert!(turns.set_mute(true, &mut out), "muted");
    assert!(out.contains(&Out::Audio(AudioCmd::SetCapture(false))), "capture stops: {out:?}");
    assert!(out.contains(&Out::Muted(true)), "announced");
    assert_eq!(turns.speaking(), None, "speech stopped");
    out.clear();
    assert!(
        !turns.speak(speak(2), SpeakCause::Reply, "", vec!["Hi.".to_owned()], &mut out),
        "nothing is spoken while muted"
    );
    turns.earcon(Earcon::Routed, &mut out);
    assert!(out.is_empty(), "no earcon while muted: {out:?}");
    assert!(!turns.set_mute(true, &mut out), "muting twice changes nothing");
    assert!(turns.set_mute(false, &mut out), "unmuted");
    assert_eq!(
        out,
        [
            Out::Audio(AudioCmd::SetCapture(true)),
            Out::Audio(AudioCmd::Earcon(Earcon::Unmuted)),
            Out::Muted(false),
        ],
        "capture first, then the earcon"
    );
}

#[test]
fn events_for_another_utterance_change_nothing() {
    let mut turns = speaking(&["Mine."]);
    let before = turns.clone();
    let mut out = Vec::new();
    turns.barge_in_window(ms(0), speak(9), 1, &mut out);
    turns.played(speak(9), 0, 10);
    turns.drained(speak(9), &mut out);
    turns.device_stopped(speak(9), 0, 10, &mut out);
    assert_eq!(turns, before, "only the utterance holding the floor counts");
    assert!(out.is_empty(), "nothing emitted: {out:?}");
}

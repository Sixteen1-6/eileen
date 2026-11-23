//! The bench and the search: each start is the state it names, a state that differs only in
//! time is pruned, the decision timer fires at its instant, and a broken machine is caught by
//! the invariant it breaks, with the sequence that broke it.

use eileen_core::turn::Phase;

use super::{Bench, Fact, Fault, PR_DEPTH, Start, Step, explore};

fn bench_at(start: Start) -> Bench {
    let mut bench = Bench::new(Fault::Off);
    for step in start.prefix() {
        bench.apply(step).unwrap();
    }
    bench
}

#[test]
fn each_start_is_the_state_it_names() {
    assert_eq!(bench_at(Start::Idle).turns().phase(), Phase::Idle, "idle");
    let speaking = bench_at(Start::Speaking);
    assert_eq!(speaking.turns().phase(), Phase::Speaking, "speaking");
    assert!(!speaking.turns().barge_in(), "no barge-in yet");
    let paused = bench_at(Start::Paused);
    assert!(paused.turns().paused(), "paused after three windows");
    assert_eq!(paused.now_ms(), 96, "three ticks of 32 ms");
    let due = bench_at(Start::PausedDecisionDue);
    assert!(due.turns().paused(), "still paused");
    assert_eq!(due.now_ms(), 672, "the decision, due at 696 ms, falls in the next tick");
    let lapsing = bench_at(Start::DuckedDecisionDue);
    assert!(lapsing.turns().barge_in() && !lapsing.turns().paused(), "ducked, not paused");
    assert_eq!(lapsing.now_ms(), 608, "the duck, due to lapse at 632 ms, does so in the next tick");
}

#[test]
fn inputs_the_core_does_not_act_on_and_time_alone_lead_to_a_state_already_seen() {
    let idle = Bench::new(Fault::Off);
    for step in [Step::OtherAudio, Step::PhysicalInput, Step::PlaybackTick, Step::SpeechEnd] {
        let mut next = idle.clone();
        next.apply(step).unwrap();
        assert_eq!(next.key(), idle.key(), "{step:?} from idle");
    }
    let mut speaking = Bench::new(Fault::Off);
    speaking.apply(Step::SpeechStart).unwrap();
    assert_ne!(speaking.key(), idle.key(), "the user speaking is a state of its own");
}

#[test]
fn the_backchannel_resumes_the_pause_and_playback_goes_on() {
    let mut bench = bench_at(Start::Paused);
    bench.apply(Step::Backchannel).unwrap();
    assert!(!bench.turns().barge_in(), "resumed");
    assert_eq!(bench.turns().phase(), Phase::Speaking, "still speaking");
    bench.apply(Step::PlaybackTick).unwrap();
    assert_eq!(bench.turns().phase(), Phase::Speaking, "the clause plays on");
}

#[test]
fn reaching_the_deadline_of_a_pause_resumes_it_at_600_ms() {
    let mut bench = bench_at(Start::PausedDecisionDue);
    assert!(!bench.events().contains(&Fact::DeadlineResumed), "not before the deadline");
    bench.apply(Step::PlaybackTick).unwrap();
    assert!(bench.events().contains(&Fact::DeadlineResumed), "{:?}", bench.events());
    assert!(!bench.turns().barge_in(), "resumed");
    assert_eq!(bench.turns().phase(), Phase::Speaking, "still speaking");
}

#[test]
fn reaching_the_deadline_of_a_duck_that_never_paused_undoes_it() {
    let mut bench = bench_at(Start::DuckedDecisionDue);
    assert!(!bench.events().contains(&Fact::DeadlineUnducked), "not before the deadline");
    bench.apply(Step::PlaybackTick).unwrap();
    assert!(bench.events().contains(&Fact::DeadlineUnducked), "{:?}", bench.events());
    assert!(!bench.turns().barge_in(), "unducked");
    assert_eq!(bench.turns().phase(), Phase::Speaking, "still speaking");
}

#[test]
fn a_short_search_covers_every_input_and_prunes() {
    let explored = explore(Start::Speaking, 3, Fault::Off).unwrap();
    assert_eq!(explored.depth, 3, "{explored:?}");
    assert!(explored.transitions >= 11, "every input from the start at least: {explored:?}");
    assert!(explored.pruned > 0, "no-op inputs are pruned: {explored:?}");
    assert!(explored.transitions <= 11 + 11 * 11 + 11 * 11 * 11, "{explored:?}");
}

/// The fault is caught within the PR depth from `start`, by invariant `number`.
fn caught(start: Start, fault: Fault, number: u8) -> Vec<Step> {
    let found = explore(start, PR_DEPTH, fault).unwrap_err();
    assert_eq!(found.violation.number, number, "{found}");
    assert!(found.steps.starts_with(&start.prefix()), "{found}");
    found.steps
}

#[test]
fn a_machine_that_never_ducks_breaks_invariant_2() {
    let steps = caught(Start::Speaking, Fault::NoDuck, 2);
    assert_eq!(
        steps.get(Start::Speaking.prefix().len()..),
        Some([Step::SpeechStart, Step::PlaybackTick, Step::PlaybackTick].as_slice()),
        "the shortest: speech over Eileen, its first window, then the next callback"
    );
}

#[test]
fn a_machine_that_never_pauses_breaks_invariant_2() {
    caught(Start::Speaking, Fault::NoPause, 2);
}

#[test]
fn a_machine_that_tells_the_brain_the_whole_reply_breaks_invariant_3() {
    caught(Start::Paused, Fault::ToldWholeReply, 3);
}

#[test]
fn a_mute_that_leaves_the_microphone_on_breaks_invariant_9() {
    let steps = caught(Start::Idle, Fault::CaptureLeftOn, 9);
    assert_eq!(steps, [Step::Mute, Step::PlaybackTick], "the shortest is found");
}

#[test]
fn a_machine_that_ignores_a_mute_breaks_invariant_9() {
    let steps = caught(Start::Speaking, Fault::IgnoresMute, 9);
    assert_eq!(
        steps.get(Start::Speaking.prefix().len()..),
        Some([Step::Mute, Step::PlaybackTick].as_slice()),
        "the shortest: the mute, then the next tick finds capture still on"
    );
}

#[test]
fn an_utterance_over_another_breaks_invariant_10() {
    caught(Start::Idle, Fault::PhantomUtterance, 10);
}

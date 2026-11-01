//! `turn::exhaustive` (section 6.3 acceptance 2): every sequence of the eleven turn-taking
//! inputs to depth 6 from each start, with invariants 2, 3, 9 and 10 checked after every
//! input. The nightly lane sets `EILEEN_EXHAUSTIVE_DEPTH=8`.

use eileen_sim::exhaustive::{DEPTH_ENV, Fault, NIGHTLY_DEPTH, PR_DEPTH, STEPS, Start, explore};
use eileen_sim::invariants::EXHAUSTIVE;

/// The depth to run: `EILEEN_EXHAUSTIVE_DEPTH` when it is set, else the pull request depth.
fn depth() -> u8 {
    match std::env::var(DEPTH_ENV) {
        Ok(value) => value.trim().parse().unwrap_or_else(|_| {
            panic!("{DEPTH_ENV}={value:?} is not a depth; the nightly lane sets {NIGHTLY_DEPTH}")
        }),
        Err(_) => PR_DEPTH,
    }
}

#[test]
fn every_sequence_to_the_depth_keeps_invariants_2_3_9_and_10() {
    assert_eq!(STEPS.len(), 11, "section 4.7 names eleven turn-taking inputs");
    let numbers: Vec<u8> = EXHAUSTIVE.iter().map(|(number, _)| *number).collect();
    assert_eq!(numbers, [2, 3, 9, 10], "section 4.7 names invariants 2, 3, 9 and 10");
    let depth = depth();
    assert!(depth >= PR_DEPTH, "never shallower than the pull request depth: {depth}");
    for start in Start::ALL {
        let explored = explore(start, depth, Fault::Off).unwrap_or_else(|found| panic!("{found}"));
        println!(
            "turn::exhaustive from {start:?} to depth {depth}: {} states, {} inputs applied, {} pruned",
            explored.states, explored.transitions, explored.pruned
        );
        assert!(explored.states > 1, "the search went somewhere: {explored:?}");
    }
}

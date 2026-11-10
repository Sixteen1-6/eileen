//! `turn::exhaustive` (section 6.3 acceptance 2): every sequence of the eleven turn-taking
//! inputs to depth 6 from each start, with invariants 2, 3, 9 and 10 checked after every
//! input. The nightly lane sets `EILEEN_EXHAUSTIVE_DEPTH=8`.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use eileen_sim::exhaustive::{
    DEPTH_ENV, Fact, Fault, NIGHTLY_DEPTH, PR_DEPTH, STEPS, Start, explore,
};
use eileen_sim::invariants::EXHAUSTIVE;

/// What the search reaches from each start at the pull request depth. A change here is a
/// change in the search's reach, to be explained in review.
const REACH_AT_PR_DEPTH: &str = "Idle: 158 states, 1045 inputs, 823 pruned, 0 deadlines
Speaking: 827 states, 5258 inputs, 4088 pruned, 0 deadlines
Paused: 1111 states, 8448 inputs, 6651 pruned, 0 deadlines
PausedDecisionDue: 1219 states, 9174 inputs, 7276 pruned, 11 deadlines
DuckedDecisionDue: 1073 states, 8030 inputs, 6355 pruned, 11 deadlines
";

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
    let mut reach = String::new();
    let mut covered = BTreeSet::new();
    for start in Start::ALL {
        let explored = explore(start, depth, Fault::Off).unwrap_or_else(|found| panic!("{found}"));
        writeln!(
            reach,
            "{start:?}: {} states, {} inputs, {} pruned, {} deadlines",
            explored.states, explored.transitions, explored.pruned, explored.deadlines
        )
        .unwrap();
        if matches!(start, Start::PausedDecisionDue | Start::DuckedDecisionDue) {
            assert!(explored.deadlines > 0, "the decision timer fired: {explored:?}");
        }
        covered.extend(explored.coverage);
    }
    println!("turn::exhaustive to depth {depth}:\n{reach}");
    let missed: Vec<Fact> = Fact::ALL.into_iter().filter(|fact| !covered.contains(fact)).collect();
    assert!(missed.is_empty(), "the search never reached {missed:?}");
    assert!(covered.contains(&Fact::DeadlineResumed), "a pause resumed at its deadline");
    assert!(covered.contains(&Fact::DeadlineUnducked), "a duck lapsed at its deadline");
    if depth == PR_DEPTH {
        assert_eq!(reach, REACH_AT_PR_DEPTH, "the search's reach at depth {PR_DEPTH} changed");
    }
}

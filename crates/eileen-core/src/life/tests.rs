//! The backoff steps on their own; the deadlines through `Core::step` are in `tests::life`.

use super::{BACKOFF_MAX_NS, backoff_ns};

const S: u64 = 1_000_000_000;

#[test]
fn the_backoff_doubles_from_one_second_and_never_passes_a_minute() {
    let steps: Vec<u64> = (1..=9).map(|failures| backoff_ns(failures) / S).collect();
    assert_eq!(steps, [1, 2, 4, 8, 16, 32, 60, 60, 60], "1 s doubling, held at 60 s");
    assert_eq!(backoff_ns(0), S, "before any failure, the first step");
    assert_eq!(backoff_ns(u32::MAX), BACKOFF_MAX_NS, "never more than a minute");
}

#[test]
fn every_step_is_at_least_the_one_before() {
    for failures in 1..200 {
        assert!(
            backoff_ns(failures + 1) >= backoff_ns(failures),
            "step {} is shorter than step {failures}",
            failures + 1
        );
    }
}

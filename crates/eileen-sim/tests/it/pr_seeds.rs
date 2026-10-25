//! The pull request seed set, one test per seed (BUILD.md section 4.7), each run with the
//! default profile and doublechecked: run twice, one trace hash.

use eileen_sim::Profile;
use eileen_sim::run::{PR_SEEDS, doublecheck};

/// Runs `seed` twice with the default profile; fails on a broken invariant or two traces.
fn check(seed: u64) {
    assert!(PR_SEEDS.contains(&seed), "seed {seed} is in the pull request set");
    let profile = Profile::default_profile().unwrap();
    match doublecheck(&profile, seed, None).unwrap() {
        Ok(report) => assert!(
            report.passed(),
            "{}\nseed {seed} failed: {:?}\nreplay: {}",
            report.log,
            report.failure,
            report.replay_command()
        ),
        Err((first, second)) => panic!("seed {seed}: trace {first:#018x} then {second:#018x}"),
    }
}

macro_rules! seeds {
    ($($name:ident = $seed:expr;)*) => {
        $(
            #[test]
            fn $name() {
                check($seed);
            }
        )*

        #[test]
        fn every_pull_request_seed_has_a_test() {
            let tested = [$($seed),*];
            assert_eq!(tested.as_slice(), PR_SEEDS, "one test per seed, in order");
        }
    };
}

seeds! {
    seed_1 = 1;
    seed_2 = 2;
    seed_3 = 3;
    seed_7 = 7;
    seed_42 = 42;
    seed_1000 = 1_000;
    seed_65537 = 65_537;
    seed_c0ffee = 0x00C0_FFEE;
    seed_deadbeef = 0xDEAD_BEEF;
    seed_5eed = 0x5EED_0000_0000_0001;
    seed_max_minus_one = u64::MAX - 1;
    seed_max = u64::MAX;
}

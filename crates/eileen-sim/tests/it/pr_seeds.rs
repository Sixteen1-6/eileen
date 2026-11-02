//! The pull request seed set, one test per seed and profile (BUILD.md section 4.7), each run
//! with every invariant the world checks so far and doublechecked: run twice, one trace hash.

use eileen_sim::Profile;
use eileen_sim::profile::names;
use eileen_sim::run::{PR_SEEDS, doublecheck};

/// Runs `seed` of `profile` twice; fails on a broken invariant or two traces.
fn check(profile: &str, seed: u64) {
    assert!(PR_SEEDS.contains(&seed), "seed {seed} is in the pull request set");
    let profile = Profile::load(profile).unwrap();
    match doublecheck(&profile, seed, None).unwrap() {
        Ok(report) => assert!(
            report.passed(),
            "{}\nseed {seed} of `{}` failed: {:?}\nfaults: {:?}\nreplay: {}",
            report.log,
            profile.name,
            report.failure,
            report.faults,
            report.replay_command()
        ),
        Err((first, second)) => {
            panic!("seed {seed} of `{}`: trace {first:#018x} then {second:#018x}", profile.name)
        }
    }
}

/// One module of tests for a profile: one test per pull request seed.
macro_rules! profile {
    ($module:ident = $profile:literal) => {
        mod $module {
            use super::check;

            #[test]
            fn seed_1() {
                check($profile, 1);
            }

            #[test]
            fn seed_2() {
                check($profile, 2);
            }

            #[test]
            fn seed_3() {
                check($profile, 3);
            }

            #[test]
            fn seed_7() {
                check($profile, 7);
            }

            #[test]
            fn seed_42() {
                check($profile, 42);
            }

            #[test]
            fn seed_1000() {
                check($profile, 1_000);
            }

            #[test]
            fn seed_65537() {
                check($profile, 65_537);
            }

            #[test]
            fn seed_c0ffee() {
                check($profile, 0x00C0_FFEE);
            }

            #[test]
            fn seed_deadbeef() {
                check($profile, 0xDEAD_BEEF);
            }

            #[test]
            fn seed_5eed() {
                check($profile, 0x5EED_0000_0000_0001);
            }

            #[test]
            fn seed_max_minus_one() {
                check($profile, u64::MAX - 1);
            }

            #[test]
            fn seed_max() {
                check($profile, u64::MAX);
            }
        }
    };
}

profile!(default = "default");
profile!(barge_in_heavy = "barge-in-heavy");
profile!(many_jobs = "many-jobs");
profile!(flaky_sidecars = "flaky-sidecars");
profile!(media_playing = "media-playing");
profile!(low_memory = "low-memory");

#[test]
fn every_pull_request_seed_and_profile_has_a_test() {
    let tested = [
        1,
        2,
        3,
        7,
        42,
        1_000,
        65_537,
        0x00C0_FFEE,
        0xDEAD_BEEF,
        0x5EED_0000_0000_0001,
        u64::MAX - 1,
        u64::MAX,
    ];
    assert_eq!(tested.as_slice(), PR_SEEDS, "one test per seed, in order");
    assert_eq!(
        names(),
        ["default", "barge-in-heavy", "many-jobs", "flaky-sidecars", "media-playing", "low-memory"],
        "a module of seeds per profile"
    );
}

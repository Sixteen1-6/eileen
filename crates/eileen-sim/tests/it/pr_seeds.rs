//! The pull request seed set, one test per seed and profile (BUILD.md section 4.7), each run
//! with every invariant the world checks so far and doublechecked: run twice, one trace hash.
//! The tests and the list the coverage test compares with `PR_SEEDS` and the profile names come
//! from one list of seeds (`with_seeds!`) and one of profiles (`profiles!`), so a seed or a
//! profile cannot be listed without its tests, or the other way round.

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

/// Calls the macro `$m` with `$args`, then every pull request seed as `test_name = seed;`, in
/// `PR_SEEDS`'s order.
macro_rules! with_seeds {
    ($m:ident! $($args:tt)*) => {
        $m! {
            $($args)*
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
    };
}

/// One module of tests for a profile: one test per seed.
macro_rules! seed_tests {
    ($module:ident = $profile:literal; $($name:ident = $seed:expr;)*) => {
        mod $module {
            use super::check;

            $(
                #[test]
                fn $name() {
                    check($profile, $seed);
                }
            )*
        }
    };
}

/// The seeds, as an array.
macro_rules! seed_list {
    ($($name:ident = $seed:expr;)*) => {
        [$($seed),*]
    };
}

/// A module of seed tests per profile, and the names of the profiles tested.
macro_rules! profiles {
    ($($module:ident = $profile:literal;)*) => {
        $(with_seeds!(seed_tests! $module = $profile;);)*

        /// Every profile with a module of seed tests, in order.
        const TESTED_PROFILES: &[&str] = &[$($profile),*];
    };
}

profiles! {
    default = "default";
    barge_in_heavy = "barge-in-heavy";
    many_jobs = "many-jobs";
    flaky_sidecars = "flaky-sidecars";
    media_playing = "media-playing";
    low_memory = "low-memory";
}

#[test]
fn every_pull_request_seed_and_profile_has_a_test() {
    let tested: &[u64] = &with_seeds!(seed_list!);
    assert_eq!(tested, PR_SEEDS, "one test per seed, in order");
    assert_eq!(TESTED_PROFILES, names().as_slice(), "a module of seed tests per profile, in order");
}

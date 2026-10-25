//! Runs: one seed, raw bytes (the fuzz target), a stored regression, the pull request seed set
//! and `--doublecheck`, each on a thread with a large explicit stack.

use std::fmt;
use std::io;

use arbitrary::Unstructured;
use eileen_contracts::core_io::{Input, TimerId};
use eileen_contracts::record::{InputRecord, SimRegression};
use eileen_stdx::rng::CoreRng;

use crate::inputs::Generator;
use crate::invariants::Violation;
use crate::profile::{Profile, ProfileError};
use crate::world::World;

/// The pull request seed set: fixed, so every pull request runs the same simulations, and
/// one test each (`eileen-sim::it::pr_seeds`). `main` and nightly use fresh seeds.
pub const PR_SEEDS: &[u64] = &[
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

/// The simulator thread's stack. Windows gives a program's main thread 1 MiB, and the core
/// matches on large enums in one deep `step`, so a run never relies on the default: 64 MiB is
/// reserved address space, committed only as far as the run goes.
pub const SIM_STACK_BYTES: usize = 64 * 1024 * 1024;

/// Bytes of input buffer per step: more than one input draws, so a seeded run never runs dry.
const BYTES_PER_STEP: usize = 32;

/// What a run did.
#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    /// The profile's name.
    pub profile: String,
    /// The seed, for a seeded run.
    pub seed: Option<u64>,
    /// Steps taken.
    pub steps: u32,
    /// The trace hash.
    pub hash: u64,
    /// The step log.
    pub log: String,
    /// The decoded inputs, as a `SimRegression` records them.
    pub inputs: Vec<InputRecord>,
    /// The first broken invariant and the step it broke at.
    pub failure: Option<(u32, Violation)>,
}

impl RunReport {
    /// Whether every invariant held.
    #[must_use]
    pub const fn passed(&self) -> bool {
        self.failure.is_none()
    }

    /// The command that runs this seed again, with the step count it ran.
    #[must_use]
    pub fn replay_command(&self) -> String {
        let (profile, steps) = (&self.profile, self.steps);
        match self.seed {
            Some(seed) => {
                format!("cargo xtask sim --seed {seed} --profile {profile} --steps {steps}")
            }
            None => {
                format!("cargo xtask sim --profile {profile} --steps {steps} (from the same bytes)")
            }
        }
    }
}

/// A run that could not happen.
#[derive(Debug)]
pub enum RunError {
    /// The profile did not load.
    Profile(ProfileError),
    /// The simulator thread could not start, or a regression file could not be read.
    Io(io::Error),
    /// The simulator thread panicked.
    Panicked(String),
    /// A regression names something this build cannot replay.
    Unreplayable(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Panicked(message) => write!(f, "the simulator thread panicked: {message}"),
            Self::Unreplayable(why) => write!(f, "cannot replay: {why}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<ProfileError> for RunError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

impl From<io::Error> for RunError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Runs `f` on a thread named `eileen-sim` with [`SIM_STACK_BYTES`] of stack.
pub fn on_sim_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, RunError> {
    let handle =
        eileen_stdx::thread::spawn_named_with_stack("eileen-sim", Some(SIM_STACK_BYTES), f)?;
    handle.join().map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        RunError::Panicked(message)
    })
}

/// Runs `profile` from `seed`: the seed fills the input buffer through `CoreRng` and seeds the
/// world. Runs on the calling thread; see [`run_seed`] for the large stack.
#[must_use]
pub fn run_seed_here(profile: &Profile, seed: u64, steps: Option<u32>) -> RunReport {
    let steps = steps.unwrap_or(profile.steps);
    let mut buffer = vec![0_u8; usize::try_from(steps).unwrap_or(0).saturating_mul(BYTES_PER_STEP)];
    // The world takes the seed's first two streams (the generator's IDs and the core's), so
    // the buffer comes from a third, two jumps on.
    let mut stream = CoreRng::from_seed(seed);
    stream.jump();
    stream.jump();
    stream.fill_bytes(&mut buffer);
    let mut report = run_bytes(profile, seed, &buffer, steps);
    report.seed = Some(seed);
    report
}

/// [`run_seed_here`] on the simulator thread.
pub fn run_seed(profile: &Profile, seed: u64, steps: Option<u32>) -> Result<RunReport, RunError> {
    let profile = profile.clone();
    on_sim_thread(move || run_seed_here(&profile, seed, steps))
}

/// Runs `steps` steps of `profile` with inputs drawn from `data`, the world seeded from
/// `seed`. When `data` runs out, the draws fall back to their smallest values, so the run
/// still takes every step.
#[must_use]
pub fn run_bytes(profile: &Profile, seed: u64, data: &[u8], steps: u32) -> RunReport {
    let mut world = World::new(profile.clone(), seed);
    let mut bytes = Unstructured::new(data);
    let mut generator = Generator::default();
    let max_step_ms = u64::from(profile.max_step_ms);
    let mut failure = None;
    while world.steps() < steps {
        let elapsed = bytes.int_in_range(1..=max_step_ms).unwrap_or(1);
        let input = if let Some(timer) = world.advance(elapsed) {
            Input::Timer(timer)
        } else {
            let now_ms = world.now_ms();
            let speaking = world.speaking().to_vec();
            generator
                .next(&mut bytes, &profile.input_mix, now_ms, world.id_rng(), &speaking)
                .map_or(Input::Timer(TimerId(0)), |(_, input)| input)
        };
        let outcome = world.step(input);
        if let Some(violation) = outcome.violation {
            failure = Some((world.steps(), violation));
            break;
        }
    }
    report_of(&world, None, failure)
}

/// Replays a stored regression: its inputs at their recorded times, against the current core.
/// The skeleton has no fault sites yet, so a regression that records faults cannot replay.
pub fn replay_here(regression: &SimRegression) -> Result<RunReport, RunError> {
    let profile = Profile::load(&regression.profile)?;
    if let Some(fault) = regression.faults.first() {
        return Err(RunError::Unreplayable(format!(
            "fault site `{}` at seq {}: the skeleton has no fault sites yet (WP1 adds them)",
            fault.site, fault.seq
        )));
    }
    let mut world = World::new(profile, regression.seed.unwrap_or(0));
    let mut failure = None;
    for record in &regression.inputs {
        world.set_time(record.mono);
        let outcome = world.step(record.input.clone());
        if let Some(violation) = outcome.violation {
            failure = Some((world.steps(), violation));
            break;
        }
    }
    Ok(report_of(&world, regression.seed, failure))
}

/// [`replay_here`] on the simulator thread.
pub fn replay(regression: &SimRegression) -> Result<RunReport, RunError> {
    let regression = regression.clone();
    on_sim_thread(move || replay_here(&regression)).and_then(|result| result)
}

/// Runs `seed` twice and compares the trace hashes. `Ok` holds the first run's report; a
/// mismatch is a failure of determinism, reported as `Err` with both hashes.
pub fn doublecheck(
    profile: &Profile,
    seed: u64,
    steps: Option<u32>,
) -> Result<Result<RunReport, (u64, u64)>, RunError> {
    let first = run_seed(profile, seed, steps)?;
    let second = run_seed(profile, seed, steps)?;
    Ok(if first.hash == second.hash { Ok(first) } else { Err((first.hash, second.hash)) })
}

fn report_of(world: &World, seed: Option<u64>, failure: Option<(u32, Violation)>) -> RunReport {
    RunReport {
        profile: world.profile().name.clone(),
        seed,
        steps: world.steps(),
        hash: world.hash(),
        log: world.log().to_owned(),
        inputs: world.inputs().to_vec(),
        failure,
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::record::{FaultRecord, SimRegression};

    use super::{doublecheck, replay, run_bytes, run_seed};
    use crate::profile::Profile;

    fn profile() -> Profile {
        Profile::default_profile().unwrap()
    }

    #[test]
    fn a_seed_runs_every_step_and_passes_against_the_stub_core() {
        let report = run_seed(&profile(), 5, Some(300)).unwrap();
        assert!(report.passed(), "{:?}", report.failure);
        assert_eq!(report.steps, 300, "every step taken");
        assert_eq!(report.log.lines().count(), 300, "one log line per step");
        assert_eq!(report.inputs.len(), 300, "one input per step");
    }

    #[test]
    fn one_seed_gives_one_hash_and_two_seeds_two() {
        let first = doublecheck(&profile(), 9, Some(200)).unwrap();
        assert!(first.is_ok(), "the same seed twice gives one hash: {first:?}");
        let a = run_seed(&profile(), 9, Some(200)).unwrap().hash;
        let b = run_seed(&profile(), 10, Some(200)).unwrap().hash;
        assert_ne!(a, b, "another seed, another trace");
    }

    #[test]
    fn a_seeded_runs_inputs_replay_to_the_same_trace() {
        let report = run_seed(&profile(), 11, Some(150)).unwrap();
        let regression = SimRegression {
            profile: report.profile.clone(),
            seed: report.seed,
            found_at_commit: "0000000".to_owned(),
            inputs: report.inputs.clone(),
            faults: vec![],
            invariant: "none".to_owned(),
        };
        let replayed = replay(&regression).unwrap();
        assert!(replayed.passed(), "{:?}", replayed.failure);
        assert_eq!(replayed.log, report.log, "the replay takes the same steps");
    }

    #[test]
    fn a_regression_with_faults_cannot_replay_on_the_skeleton() {
        let regression = SimRegression {
            profile: "default".to_owned(),
            seed: Some(1),
            found_at_commit: "0000000".to_owned(),
            inputs: vec![],
            faults: vec![FaultRecord {
                seq: 1,
                site: "store.write".to_owned(),
                fault: "error".to_owned(),
            }],
            invariant: "6".to_owned(),
        };
        let error = replay(&regression).unwrap_err();
        assert!(error.to_string().contains("store.write"), "{error}");
    }

    #[test]
    fn empty_bytes_still_take_every_step() {
        let report = run_bytes(&profile(), 0, &[], 50);
        assert_eq!(report.steps, 50, "the draws fall back to their smallest values");
        assert!(report.passed(), "{:?}", report.failure);
    }
}

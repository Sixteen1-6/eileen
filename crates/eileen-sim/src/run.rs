//! Runs: one seed, raw bytes (the fuzz target), a stored regression, the pull request seed set
//! and `--doublecheck`, each on a thread with a large explicit stack.

use std::fmt;
use std::io;

use arbitrary::Unstructured;
use eileen_contracts::core_io::{Input, PlaybackEvent};
use eileen_contracts::record::{FaultRecord, InputRecord, SimRegression};
use eileen_stdx::rng::CoreRng;

use crate::faults::{FaultError, Faults};
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
    /// Every fault that fired, each with the step it fired in, as a `SimRegression` records
    /// them.
    pub faults: Vec<FaultRecord>,
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
    /// A regression names a fault the world cannot fire.
    Fault(FaultError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Panicked(message) => write!(f, "the simulator thread panicked: {message}"),
            Self::Fault(e) => write!(f, "cannot replay: {e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<FaultError> for RunError {
    fn from(error: FaultError) -> Self {
        Self::Fault(error)
    }
}

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

/// Runs `steps` steps of `profile` with inputs drawn from `data`, the world and its fault
/// points seeded from `seed`. When `data` runs out, the draws fall back to their smallest
/// values, so the run still takes every step.
#[must_use]
pub fn run_bytes(profile: &Profile, seed: u64, data: &[u8], steps: u32) -> RunReport {
    let mut world = World::new(profile.clone(), seed);
    world.set_faults(Faults::live(seed, profile.faults));
    let mut bytes = Unstructured::new(data);
    let mut generator = Generator::new(profile.barge_in_pct, profile.media_at_start);
    let max_step_ms = u64::from(profile.max_step_ms);
    let mut failure = None;
    while world.steps() < steps {
        let elapsed = bytes.int_in_range(1..=max_step_ms).unwrap_or(1);
        let input = world
            .advance(elapsed)
            .unwrap_or_else(|| draw(&mut world, &mut generator, &mut bytes, profile));
        let outcome = world.step(input);
        if let Some(violation) = outcome.violation {
            failure = Some((world.steps(), violation));
            break;
        }
    }
    report_of(&world, None, failure)
}

/// The generator's next input. A draw of time passing goes to the core's next timer, or to
/// whatever the world's parts answer before it; with no timer set, or when the bytes cannot
/// draw, the device reports a change, which the core ignores.
fn draw(
    world: &mut World,
    generator: &mut Generator,
    bytes: &mut Unstructured<'_>,
    profile: &Profile,
) -> Input {
    let now_ms = world.now_ms();
    let speaking = world.speaking().to_vec();
    let drawn = generator.next(bytes, &profile.input_mix, now_ms, world.id_rng(), &speaking);
    match drawn {
        Ok((_, Some(input))) => input,
        Ok((_, None)) => world
            .until_next_timer()
            .and_then(|wait| world.advance(wait.max(1)))
            .unwrap_or(Input::Playback(PlaybackEvent::DeviceChanged)),
        Err(_) => Input::Playback(PlaybackEvent::DeviceChanged),
    }
}

/// Replays a stored regression: its inputs at their recorded times, against the current core,
/// with exactly the faults it records fired at their steps. A regression that names a fault
/// site the world cannot fire does not replay.
pub fn replay_here(regression: &SimRegression) -> Result<RunReport, RunError> {
    let profile = Profile::load(&regression.profile)?;
    let faults = Faults::scripted(&regression.faults)?;
    let mut world = World::new(profile, regression.seed.unwrap_or(0));
    world.set_faults(faults);
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
        faults: world.faults().to_vec(),
        failure,
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::record::{FaultRecord, SimRegression};

    use super::{RunError, RunReport, doublecheck, replay, run_bytes, run_seed};
    use crate::faults::FaultError;
    use crate::profile::Profile;

    fn profile() -> Profile {
        Profile::default_profile().unwrap()
    }

    #[test]
    fn a_seed_runs_every_step_and_keeps_every_invariant() {
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

    /// The `SimRegression` a run's report would be stored as.
    fn regression_of(report: &RunReport) -> SimRegression {
        SimRegression {
            profile: report.profile.clone(),
            seed: report.seed,
            found_at_commit: "0000000".to_owned(),
            inputs: report.inputs.clone(),
            faults: report.faults.clone(),
            invariant: "none".to_owned(),
        }
    }

    #[test]
    fn a_seeded_runs_inputs_replay_to_the_same_trace() {
        let report = run_seed(&profile(), 11, Some(150)).unwrap();
        let replayed = replay(&regression_of(&report)).unwrap();
        assert!(replayed.passed(), "{:?}", replayed.failure);
        assert_eq!(replayed.log, report.log, "the replay takes the same steps");
    }

    #[test]
    fn a_run_with_faults_replays_with_exactly_its_fault_records() {
        // Every site is on in flaky-sidecars, so a few hundred steps fire some.
        let flaky = Profile::load("flaky-sidecars").unwrap();
        let report = (1..=8)
            .map(|seed| run_seed(&flaky, seed, Some(800)).unwrap())
            .find(|report| !report.faults.is_empty())
            .expect("a seed whose run fired a fault");
        assert!(report.passed(), "{}\n{:?}", report.log, report.failure);
        let steps = report.inputs.iter().map(|input| input.seq).collect::<Vec<_>>();
        for fault in &report.faults {
            assert!(steps.contains(&fault.seq), "each fault names a step of the run: {fault:?}");
            assert!(crate::faults::is_world_site(&fault.site), "a world site: {fault:?}");
        }
        let replayed = replay(&regression_of(&report)).unwrap();
        assert!(replayed.passed(), "{:?}", replayed.failure);
        assert_eq!(replayed.log, report.log, "the replay takes the same steps");
        assert_eq!(replayed.faults, report.faults, "and fires exactly the recorded faults");
    }

    #[test]
    fn a_regression_naming_a_site_the_world_cannot_fire_does_not_replay() {
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
        let site = |error: &RunError| match error {
            RunError::Fault(FaultError::UnknownSite { site, seq }) => Some((site.clone(), *seq)),
            RunError::Profile(_) | RunError::Io(_) | RunError::Panicked(_) => None,
        };
        assert_eq!(
            site(&error),
            Some(("store.write".to_owned(), Some(1))),
            "a typed fault error naming the site and its step: {error:?}"
        );
        assert!(error.to_string().contains("store.write"), "{error}");
    }

    #[test]
    fn a_seeded_runs_fault_points_end_with_the_run() {
        let report = run_bytes(&Profile::load("flaky-sidecars").unwrap(), 3, &[0x5a; 4_000], 400);
        assert!(report.passed(), "{:?}", report.failure);
        assert!(!eileen_stdx::buggify::is_enabled(), "the run's fault points end with it");
    }

    #[test]
    fn empty_bytes_still_take_every_step() {
        let report = run_bytes(&profile(), 0, &[], 50);
        assert_eq!(report.steps, 50, "the draws fall back to their smallest values");
        assert!(report.passed(), "{:?}", report.failure);
    }
}

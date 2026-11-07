//! Every `SimRegression` in `crates/eileen-sim/regressions/` replays clean: a regression is
//! stored with the fix of the failure it records. A run with faults, written as such a file and
//! read back, replays to the same steps with the same faults.

use std::path::Path;

use eileen_contracts::record::SimRegression;
use eileen_sim::Profile;
use eileen_sim::run::{replay, run_seed};

#[test]
fn every_regression_replays_clean() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("regressions");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let regression: SimRegression = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not a SimRegression: {e}", path.display()));
        let report = replay(&regression).unwrap();
        assert!(
            report.passed(),
            "{}\n{} fails again: {:?}",
            report.log,
            path.display(),
            report.failure
        );
    }
}

#[test]
fn a_run_with_faults_written_as_a_regression_file_replays_from_it() {
    // Every site is on in flaky-sidecars, so a few hundred steps fire some.
    let flaky = Profile::load("flaky-sidecars").unwrap();
    let report = (1..=8)
        .map(|seed| run_seed(&flaky, seed, Some(800)).unwrap())
        .find(|report| !report.faults.is_empty())
        .expect("a seed whose run fired a fault");
    let regression = SimRegression {
        profile: report.profile.clone(),
        seed: report.seed,
        found_at_commit: "0000000".to_owned(),
        inputs: report.inputs.clone(),
        faults: report.faults.clone(),
        invariant: "none".to_owned(),
    };
    let text = toml::to_string(&regression).unwrap();
    assert!(text.contains("[[faults]]"), "each fault is an explicit record in the file");
    let back: SimRegression = toml::from_str(&text).unwrap();
    assert_eq!(back.faults, report.faults, "the faults read back as written");
    assert_eq!(back.inputs.len(), report.inputs.len(), "every decoded input read back");
    let replayed = replay(&back).unwrap();
    assert!(
        replayed.passed(),
        "{}
{:?}",
        replayed.log,
        replayed.failure
    );
    assert_eq!(replayed.log, report.log, "the file replays the run's steps");
    assert_eq!(replayed.faults, report.faults, "with exactly its faults");
}

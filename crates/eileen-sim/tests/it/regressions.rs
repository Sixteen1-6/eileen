//! Every `SimRegression` in `crates/eileen-sim/regressions/` replays clean: a regression is
//! stored with the fix of the failure it records.

use std::path::Path;

use eileen_contracts::record::SimRegression;
use eileen_sim::run::replay;

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

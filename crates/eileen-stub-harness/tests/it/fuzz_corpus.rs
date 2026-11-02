//! The seed corpus of the `stub_script` fuzz target replays as an ordinary test on every OS
//! (BUILD.md section 4.7), with `fuzz/regressions/stub_script/` when it exists, and so does
//! every whole script under `tests/harness-scripts/`, whose cut pieces are among the seeds.

use std::path::{Path, PathBuf};

use eileen_stub_harness::fuzz_entry;

/// The largest seed a corpus may hold.
const MAX_SEED_BYTES: u64 = 4096;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file of a folder, sorted; none when the folder does not exist.
fn files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut files: Vec<_> = entries.map(|entry| entry.unwrap().path()).collect();
    files.sort();
    files
}

#[test]
fn stub_script_corpus_replays() {
    let corpus = files(&root().join("fuzz/corpus/stub_script"));
    assert!(!corpus.is_empty(), "fuzz/corpus/stub_script/ has no seeds");
    for seed in &corpus {
        let size = std::fs::metadata(seed).unwrap().len();
        assert!(size <= MAX_SEED_BYTES, "{} is {size} bytes, over the cap", seed.display());
    }
    let regressions = files(&root().join("fuzz/regressions/stub_script"));
    let scripts = files(&root().join("tests/harness-scripts"))
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "ndjson"));
    for input in corpus.iter().chain(&regressions).cloned().chain(scripts) {
        if let Err(broken) = fuzz_entry::stub_script(&std::fs::read(&input).unwrap()) {
            panic!("{}: {broken}", input.display());
        }
    }
}

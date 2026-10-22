//! The seed corpora of this crate's fuzz targets replay as ordinary tests (BUILD.md section
//! 4.7), with `fuzz/regressions/<target>/` when it exists. The crate builds only on Linux, so
//! they replay only there.

use std::path::{Path, PathBuf};

use eileen_shell_linux::fuzz_entry;

/// The largest seed a corpus may hold.
const MAX_SEED_BYTES: u64 = 4096;

fn fuzz_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz")
}

/// Every file of a folder, sorted; none when the folder does not exist.
fn files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut files: Vec<_> = entries.map(|entry| entry.unwrap().path()).collect();
    files.sort();
    files
}

fn replay(target: &str, entry: fn(&[u8]) -> Result<(), &'static str>) {
    let corpus = files(&fuzz_dir().join("corpus").join(target));
    assert!(!corpus.is_empty(), "fuzz/corpus/{target}/ has no seeds");
    for seed in &corpus {
        let size = std::fs::metadata(seed).unwrap().len();
        assert!(size <= MAX_SEED_BYTES, "{} is {size} bytes, over the cap", seed.display());
    }
    for seed in corpus.iter().chain(&files(&fuzz_dir().join("regressions").join(target))) {
        if let Err(broken) = entry(&std::fs::read(seed).unwrap()) {
            panic!("{}: {broken}", seed.display());
        }
    }
}

#[test]
fn linux_mountinfo_corpus_replays() {
    replay("linux_mountinfo", fuzz_entry::linux_mountinfo);
}

#[test]
fn linux_proc_status_corpus_replays() {
    replay("linux_proc_status", fuzz_entry::linux_proc_status);
}

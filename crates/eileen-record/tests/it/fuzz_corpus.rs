//! The seed corpus of this crate's fuzz target, `recording`, replays as an ordinary test on
//! every OS (BUILD.md section 4.7), with `fuzz/regressions/recording/` when it exists. Each
//! `line-*` seed is also read as a recording, so a seed that stops parsing (a contract changed
//! shape) fails here rather than silently fuzzing nothing.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use eileen_record::blob::MemoryBlobs;
use eileen_record::fuzz_entry;
use eileen_record::reader::Reader;

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

#[test]
fn recording_corpus_replays() {
    let corpus = files(&fuzz_dir().join("corpus").join("recording"));
    assert!(!corpus.is_empty(), "fuzz/corpus/recording/ has no seeds");
    for seed in &corpus {
        let size = std::fs::metadata(seed).unwrap().len();
        assert!(size <= MAX_SEED_BYTES, "{} is {size} bytes, over the cap", seed.display());
    }
    for seed in corpus.iter().chain(&files(&fuzz_dir().join("regressions").join("recording"))) {
        if let Err(broken) = fuzz_entry::recording(&std::fs::read(seed).unwrap()) {
            panic!("{}: {broken}", seed.display());
        }
    }
}

#[test]
fn every_line_seed_reads_as_a_recording() {
    let corpus = files(&fuzz_dir().join("corpus").join("recording"));
    let lines: Vec<_> = corpus
        .iter()
        .filter(|seed| seed.file_name().unwrap().to_string_lossy().starts_with("line-"))
        .collect();
    assert!(lines.len() >= 5, "the corpus holds line seeds: {lines:?}");
    for seed in lines {
        let bytes = std::fs::read(seed).unwrap();
        let read = Reader::new(Cursor::new(bytes)).read_all(&MemoryBlobs::default());
        let whole = matches!(&read, Ok(lines) if !lines.is_empty());
        assert!(whole, "{}: {read:?}", seed.display());
    }
}

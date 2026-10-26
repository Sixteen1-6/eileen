//! The `dictation_cleanup` fuzz target of BUILD.md section 4.7.
//! Its input: transcripts into the dictation cleaner.
//! It fails when `eileen_dictation::fuzz_entry::dictation_cleanup` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_dictation::fuzz_entry::dictation_cleanup(data) {
        panic!("{broken}");
    }
});

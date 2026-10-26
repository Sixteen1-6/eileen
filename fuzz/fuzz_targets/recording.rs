//! The `recording` fuzz target of BUILD.md section 4.7. Its input: recording input lines.
//! It fails when `eileen_record::fuzz_entry::recording` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_record::fuzz_entry::recording(data) {
        panic!("{broken}");
    }
});

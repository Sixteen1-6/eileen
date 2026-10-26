//! The `expected_trace` fuzz target of BUILD.md section 4.7. Its input: expected-trace files.
//! It fails when `eileen_replay::fuzz_entry::expected_trace` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_replay::fuzz_entry::expected_trace(data) {
        panic!("{broken}");
    }
});

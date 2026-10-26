//! The `sim` fuzz target of BUILD.md section 4.7. Its input: simulator runs from bytes.
//! It fails when `eileen_sim::fuzz_entry::sim` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_sim::fuzz_entry::sim(data) {
        panic!("{broken}");
    }
});

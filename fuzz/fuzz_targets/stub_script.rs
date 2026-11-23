//! The `stub_script` fuzz target: scripts for the stub harness and the stdin lines a driver
//! writes to it, both parsed by `eileen-stub-harness` (BUILD.md section 3.3.16).
//! It fails when `eileen_stub_harness::fuzz_entry::stub_script` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_stub_harness::fuzz_entry::stub_script(data) {
        panic!("{broken}");
    }
});

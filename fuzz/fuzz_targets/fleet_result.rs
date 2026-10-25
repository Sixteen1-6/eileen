//! The `fleet_result` fuzz target of BUILD.md section 4.7.
//! Its input: fleet result lines and fleet issue comments.
//! It fails when `eileen_fleet::fuzz_entry::fleet_result` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_fleet::fuzz_entry::fleet_result(data) {
        panic!("{broken}");
    }
});

//! The `control_message` fuzz target of BUILD.md section 4.7.
//! Its input: control messages from `claude -p`.
//! It fails when `eileen_harness::fuzz_entry::control_message` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_harness::fuzz_entry::control_message(data) {
        panic!("{broken}");
    }
});

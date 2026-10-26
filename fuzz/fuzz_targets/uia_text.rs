//! The `uia_text` fuzz target of BUILD.md section 4.7.
//! Its input: UI Automation strings through `sanitize`.
//! It fails when `eileen_contracts::fuzz_entry::uia_text` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_contracts::fuzz_entry::uia_text(data) {
        panic!("{broken}");
    }
});

//! The `scenario_file` fuzz target of BUILD.md section 4.7.
//! Its input: scenarios and their `inputs.jsonl` lines.
//! It fails when `eileen_scenarios::fuzz_entry::scenario_file` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_scenarios::fuzz_entry::scenario_file(data) {
        panic!("{broken}");
    }
});

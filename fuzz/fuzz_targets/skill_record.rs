//! The `skill_record` fuzz target of BUILD.md section 4.7.
//! Its input: learned skill files and `recipe.json`.
//! It fails when `eileen_skills::fuzz_entry::skill_record` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_skills::fuzz_entry::skill_record(data) {
        panic!("{broken}");
    }
});

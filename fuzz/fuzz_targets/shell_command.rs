//! The `shell_command` fuzz target of BUILD.md section 4.7.
//! Its input: worker shell commands into the tokenizer.
//! It fails when `eileen_core::fuzz_entry::shell_command` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_core::fuzz_entry::shell_command(data) {
        panic!("{broken}");
    }
});

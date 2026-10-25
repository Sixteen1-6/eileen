//! The `command_grammar` fuzz target of BUILD.md section 4.7.
//! Its input: transcripts into the fast path.
//! It fails when `eileen_intent::fuzz_entry::command_grammar` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_intent::fuzz_entry::command_grammar(data) {
        panic!("{broken}");
    }
});

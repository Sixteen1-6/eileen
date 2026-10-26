//! The `resolve_reply` fuzz target of BUILD.md section 4.7. Its input: Resolve bridge replies.
//! It fails when `eileen_apps::fuzz_entry::resolve_reply` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_apps::fuzz_entry::resolve_reply(data) {
        panic!("{broken}");
    }
});

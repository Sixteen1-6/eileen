//! The `stream_json_line` fuzz target of BUILD.md section 4.7.
//! Its input: stream-json lines from `claude -p`.
//! It fails when `eileen_harness::fuzz_entry::stream_json_line` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_harness::fuzz_entry::stream_json_line(data) {
        panic!("{broken}");
    }
});

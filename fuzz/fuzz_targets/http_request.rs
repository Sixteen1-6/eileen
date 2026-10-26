//! The `http_request` fuzz target of BUILD.md section 4.7.
//! Its input: requests to the loopback HTTP parser.
//! It fails when `eileen_runtime::fuzz_entry::http_request` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_runtime::fuzz_entry::http_request(data) {
        panic!("{broken}");
    }
});

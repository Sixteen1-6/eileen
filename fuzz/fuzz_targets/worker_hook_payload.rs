//! The `worker_hook_payload` fuzz target of BUILD.md section 4.7.
//! Its input: hook bodies from workers.
//! It fails when `eileen_runtime::fuzz_entry::worker_hook_payload` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_runtime::fuzz_entry::worker_hook_payload(data) {
        panic!("{broken}");
    }
});

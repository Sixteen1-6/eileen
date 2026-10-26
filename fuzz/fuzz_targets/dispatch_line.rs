//! The `dispatch_line` fuzz target of BUILD.md section 4.7. Its input: the shim's pipe lines.
//! It fails when `eileen_mcp::fuzz_entry::dispatch_line` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_mcp::fuzz_entry::dispatch_line(data) {
        panic!("{broken}");
    }
});

//! The `mcp_tool_arguments` fuzz target of BUILD.md section 4.7.
//! Its input: tool arguments from workers.
//! It fails when `eileen_mcp::fuzz_entry::mcp_tool_arguments` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_mcp::fuzz_entry::mcp_tool_arguments(data) {
        panic!("{broken}");
    }
});

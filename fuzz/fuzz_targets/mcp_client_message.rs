//! The `mcp_client_message` fuzz target of BUILD.md section 4.7. Its input: JSON-RPC from sidecars.
//! It fails when `eileen_mcp::fuzz_entry::mcp_client_message` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_mcp::fuzz_entry::mcp_client_message(data) {
        panic!("{broken}");
    }
});

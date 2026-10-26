//! The `desktop_state` fuzz target of BUILD.md section 4.7. Its input: `DesktopState` snapshots.
//! It fails when `eileen_contracts::fuzz_entry::desktop_state` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_contracts::fuzz_entry::desktop_state(data) {
        panic!("{broken}");
    }
});

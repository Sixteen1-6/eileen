//! The `linux_proc_status` fuzz target of BUILD.md section 4.7.
//! Its input: `/proc/<pid>/status` into the largest-process scan.
//! It fails when `eileen_shell_linux::fuzz_entry::linux_proc_status` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_shell_linux::fuzz_entry::linux_proc_status(data) {
        panic!("{broken}");
    }
});

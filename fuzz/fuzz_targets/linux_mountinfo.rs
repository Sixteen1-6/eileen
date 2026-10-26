//! The `linux_mountinfo` fuzz target of BUILD.md section 4.7.
//! Its input: `/proc/self/mountinfo` into the mount parser.
//! It fails when `eileen_shell_linux::fuzz_entry::linux_mountinfo` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_shell_linux::fuzz_entry::linux_mountinfo(data) {
        panic!("{broken}");
    }
});

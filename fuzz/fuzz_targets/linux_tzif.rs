//! The `linux_tzif` fuzz target of BUILD.md section 4.7.
//! Its input: `/etc/localtime`, a TZif file, into the local time zone reader.
//! It fails when `eileen_shell_linux::fuzz_entry::linux_tzif` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_shell_linux::fuzz_entry::linux_tzif(data) {
        panic!("{broken}");
    }
});

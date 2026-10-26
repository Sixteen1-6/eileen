//! The `desktop_entry` fuzz target of BUILD.md section 4.7.
//! Its input: desktop entry files into the parser and the interpreter.
//! It fails when `eileen_apps_linux::fuzz_entry::desktop_entry` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_apps_linux::fuzz_entry::desktop_entry(data) {
        panic!("{broken}");
    }
});

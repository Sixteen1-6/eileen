//! The `speech_frame` fuzz target of BUILD.md section 4.7. Its input: speech pipe frames.
//! It fails when `eileen_speech::fuzz_entry::speech_frame` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_speech::fuzz_entry::speech_frame(data) {
        panic!("{broken}");
    }
});

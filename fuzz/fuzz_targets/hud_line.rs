//! The `hud_line` fuzz target of BUILD.md section 4.7. Its input: HUD protocol lines.
//! It fails when `eileen_hud::fuzz_entry::hud_line` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_hud::fuzz_entry::hud_line(data) {
        panic!("{broken}");
    }
});

//! The `config_toml` fuzz target of BUILD.md section 4.7.
//! Its input: the user's configuration files.
//! It fails when `eileen_config::fuzz_entry::config_toml` reports a broken invariant.

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Err(broken) = eileen_config::fuzz_entry::config_toml(data) {
        panic!("{broken}");
    }
});

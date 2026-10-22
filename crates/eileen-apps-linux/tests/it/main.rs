//! Integration tests of `eileen-apps-linux` through its public API: the app index over the
//! fixture session in `tests/fixtures/session/`, and once over the session the tests run in.
//! Linux only, as the crate is. Each module is `cfg(test)`, so clippy holds it to the test rules
//! (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod index;

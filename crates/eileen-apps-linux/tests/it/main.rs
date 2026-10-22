//! Integration tests of `eileen-apps-linux` through its public API: the app index over the
//! fixture session in `tests/fixtures/session/`, and once over the session the tests run in; the
//! fuzz seed corpus; and the opener against a fake `OpenURI` portal on `eileen-dbus-linux`'s
//! private bus. Linux only, as the crate is. Each module is `cfg(test)`, so clippy holds it to
//! the test rules (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod index;
#[cfg(test)]
mod open;

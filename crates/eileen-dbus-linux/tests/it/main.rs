//! Integration tests of `eileen-dbus-linux` through its public API, against fake services on a
//! private bus: the buses' method timeout, the private bus itself, name owners and their
//! changes, and the round-trip barrier. Linux only, as the crate is. Each module is `cfg(test)`,
//! so clippy holds it to the test rules (`clippy.toml`'s test allowances and
//! `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod bus;
#[cfg(test)]
mod names;

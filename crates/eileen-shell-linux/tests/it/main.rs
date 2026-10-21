//! Integration tests of `eileen-shell-linux` through its public API: the probe over each
//! fixture machine in `tests/fixtures/`, and once over the machine the tests run on. Linux
//! only, as the crate is. Each module is `cfg(test)`, so clippy holds it to the test rules
//! (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod live;
#[cfg(test)]
mod machines;
#[cfg(test)]
mod tree;

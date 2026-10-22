//! Integration tests of `eileen-shell-linux` through its public API: the probe and the memory
//! sampler over the fixture machines in `tests/fixtures/`, and once over the machine the tests
//! run on. Linux only, as the crate is. Each module is `cfg(test)`, so clippy holds it to the
//! test rules (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#![cfg(target_os = "linux")]

#[cfg(test)]
mod bus;
#[cfg(test)]
mod live;
#[cfg(test)]
mod machines;
#[cfg(test)]
mod memory;
#[cfg(test)]
mod tree;

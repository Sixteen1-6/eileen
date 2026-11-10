//! Integration tests of `eileen-record` through its public API: recordings written and read back
//! through the disk store, trim, blob reference counts and the fuzz seed corpus. Each module is
//! `cfg(test)`, so clippy holds it to the test rules (`clippy.toml`'s test allowances and
//! `tests_outside_test_module`).

#[cfg(test)]
mod blobs;
#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod round_trip;
#[cfg(test)]
mod support;
#[cfg(test)]
mod trim;

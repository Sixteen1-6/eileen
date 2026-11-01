//! Integration tests of `eileen-harness` through its public API: the fuzz seed corpora. Each
//! module is `cfg(test)`, so clippy holds it to the test rules (`clippy.toml`'s test allowances
//! and `tests_outside_test_module`).

#[cfg(test)]
mod fuzz_corpus;

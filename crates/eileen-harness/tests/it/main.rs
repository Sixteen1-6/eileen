//! Integration tests of `eileen-harness` through its public API: the fuzz seed corpora, and the
//! worker's start against the signed-in CLI on the owner's PC (`real_claude`, which the default
//! profiles skip). Each module is `cfg(test)`, so clippy holds it to the test rules
//! (`clippy.toml`'s test allowances and `tests_outside_test_module`).

#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod real_claude;

//! Integration tests of `eileen-contracts` through its public API: one snapshot per module,
//! the text walk, `modules.toml`, the documented wire forms and the fuzz seed corpora. Each
//! module is `cfg(test)`, so clippy holds it to the test rules (`clippy.toml`'s test
//! allowances and `tests_outside_test_module`).

#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod modules;
#[cfg(test)]
mod schemas;
#[cfg(test)]
mod text;
#[cfg(test)]
mod wire;

//! Integration tests of `eileen-cli`: every command of section 3.5 through the library, and the
//! binary itself. Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod commands;
#[cfg(test)]
mod process;

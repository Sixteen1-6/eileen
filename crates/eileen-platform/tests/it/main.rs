//! Integration tests of `eileen-platform` through its public API: the fleet's machine hash and
//! the Linux machine-id reader (acceptance 12 of BUILD.md section 6.2, run on Windows and Linux
//! alike). Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod machine;

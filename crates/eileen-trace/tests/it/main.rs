//! Integration tests of `eileen-trace` through its public API: the redaction canary
//! (acceptance 6 of BUILD.md section 6.2) and the whole subscriber stack. Each module is
//! `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod canary;
#[cfg(test)]
mod stack;

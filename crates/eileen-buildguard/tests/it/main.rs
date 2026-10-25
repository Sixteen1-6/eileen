//! Integration tests of `eileen-buildguard` through its public API (acceptance 4 of BUILD.md
//! section 6.2): admission with fake memory and a manual clock, the cross-process files, the
//! stale holder and, on Windows, the kill-on-close job. Tests that need a second process run
//! this test binary again as a helper (`helper::entry`). Each module is `cfg(test)`, so clippy
//! holds it to the test rules.

#[cfg(test)]
mod admission;
#[cfg(test)]
mod helper;
#[cfg(test)]
mod processes;
#[cfg(test)]
mod support;

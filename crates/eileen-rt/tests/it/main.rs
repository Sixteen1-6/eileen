//! Integration tests of `eileen-rt` through its public API: tasks keep their span, show in the
//! await tree while they run, and log their panics. Each module is `cfg(test)`, so clippy holds
//! it to the test rules.

#[cfg(test)]
mod tasks;

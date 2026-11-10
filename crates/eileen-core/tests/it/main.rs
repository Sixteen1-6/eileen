//! Integration tests of `eileen-core` through its public API: the job registry against a
//! reference model of the job lifecycle. Each module is `cfg(test)`, so clippy holds it to the
//! test rules.

#[cfg(test)]
mod job_model;

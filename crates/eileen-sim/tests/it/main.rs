//! Integration tests of `eileen-sim`: one test per pull request seed, every stored regression,
//! and slice S1's flows through the minimal core. Each module is `cfg(test)`, so clippy holds it
//! to the test rules.

#[cfg(test)]
mod pr_seeds;
#[cfg(test)]
mod regressions;
#[cfg(test)]
mod slice;

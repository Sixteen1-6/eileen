//! Integration tests of `eileen-sim`: one test per pull request seed, and every stored
//! regression. Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod pr_seeds;
#[cfg(test)]
mod regressions;

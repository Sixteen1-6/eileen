//! Integration tests of `eileen-sim`: one test per pull request seed and profile, every stored
//! regression, the world's PC, fault points and stalling brain, slice S1's flows through the
//! minimal core, barge-in's timing and heard prefix, and the exhaustive enumeration of
//! turn-taking. Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod barge_in;
#[cfg(test)]
mod pr_seeds;
#[cfg(test)]
mod regressions;
#[cfg(test)]
mod slice;
#[cfg(test)]
mod turn;
#[cfg(test)]
mod world;

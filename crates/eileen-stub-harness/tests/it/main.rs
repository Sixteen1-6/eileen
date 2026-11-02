//! Integration tests of `eileen-stub-harness`: the binary started through the supervisor, as
//! `ClaudeWorker` will start it, and the script corpus under `tests/harness-scripts/`. Each module
//! is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod play;

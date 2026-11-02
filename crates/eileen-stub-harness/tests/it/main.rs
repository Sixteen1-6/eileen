//! Integration tests of `eileen-stub-harness`: the binary started through the supervisor, as
//! `ClaudeWorker` will start it, the script corpus under `tests/harness-scripts/`, and the
//! `stub_script` fuzz corpus. Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod corpus;
#[cfg(test)]
mod fuzz_corpus;
#[cfg(test)]
mod play;

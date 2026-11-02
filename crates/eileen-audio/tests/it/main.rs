//! Integration tests of `eileen-audio` through its public API: the audio component's end of the
//! speech pipe over the in-memory transport, with a scripted stand-in for the speech process
//! (WP4 acceptance 15). Each module is `cfg(test)`, so clippy holds it to the test rules.

#[cfg(test)]
mod speech_pipe;

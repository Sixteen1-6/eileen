//! The `eileen-speech` binary, the speech process; its guidance is the crate doc of the library.

use clap::Parser as _;
use eileen_speech::cli::{SpeechCli, run};

fn main() -> anyhow::Result<()> {
    run(SpeechCli::parse())
}

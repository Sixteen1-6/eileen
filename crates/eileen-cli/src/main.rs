//! The `eileen` binary; its guidance is the crate doc of the `eileen_cli` library.

use clap::Parser as _;
use eileen_cli::{Cli, run};

fn main() -> anyhow::Result<()> {
    run(Cli::parse())
}

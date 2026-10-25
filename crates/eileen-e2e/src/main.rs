//! The `eileen-e2e` binary, the end-to-end tools; its guidance is the crate doc of the library.

use clap::Parser as _;
use eileen_e2e::cli::{E2eCli, run};

fn main() -> anyhow::Result<()> {
    run(E2eCli::parse())
}

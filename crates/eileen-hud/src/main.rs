//! The `eileen-hud` binary, the on-screen layer; its guidance is the crate doc of the library.

use clap::Parser as _;
use eileen_hud::cli::{HudCli, run};

fn main() -> anyhow::Result<()> {
    run(HudCli::parse())
}

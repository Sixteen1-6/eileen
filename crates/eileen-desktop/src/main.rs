//! The `eileen-desktop` binary, the desktop sidecar; its guidance is the crate doc of the library.

use clap::Parser as _;
use eileen_desktop::cli::{DesktopCli, run};

fn main() -> anyhow::Result<()> {
    run(DesktopCli::parse())
}

//! The `eileen-apps` binary, the app adapters sidecar; its guidance is the crate doc of the
//! library.

use clap::Parser as _;
use eileen_apps::cli::{AppsCli, run};

fn main() -> anyhow::Result<()> {
    run(AppsCli::parse())
}

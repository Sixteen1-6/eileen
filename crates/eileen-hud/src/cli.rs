//! The `eileen-hud` command line: it runs the HUD process on the pipe the core names. A stub
//! until WP8 builds it: it says it is not built yet.

use clap::Parser;

/// `eileen-hud`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Parser)]
#[command(name = "eileen-hud", version, about = "Eileen's on-screen layer")]
pub struct HudCli;

/// Runs `eileen-hud`.
pub fn run(_cli: HudCli) -> anyhow::Result<()> {
    anyhow::bail!("`eileen-hud` is not built yet (WP8 builds it)")
}

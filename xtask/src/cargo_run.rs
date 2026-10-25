//! The guarded cargo commands: `check`, `test`, `clippy`, `build`, `insta`, `ci`, `pre-push` and
//! `sim`, each run through the build guard (BUILD.md sections 4b.5 and 6.1).

use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::{CargoCommand, GuardOptions, PrePushOptions, SimOptions};

/// One guarded cargo command.
pub(crate) fn cargo(_command: &CargoCommand, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// What the pull request lane runs, locally.
pub(crate) fn ci(_guard: GuardOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// What `.githooks/pre-push` runs.
pub(crate) fn pre_push(_options: &PrePushOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// The simulator, through the guard.
pub(crate) fn sim(_options: &SimOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

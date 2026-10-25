//! `cargo xtask lint-repo`: the fast repository checks of BUILD.md section 4b.3.

use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// Runs every `lint-repo` check over the tree at `root`.
pub(crate) fn main(root: &Path, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "lint-repo: no checks yet under {}", root.display())?;
    Ok(ExitCode::SUCCESS)
}

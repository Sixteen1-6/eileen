//! The commands about this machine: `setup`, `slots`, `desktop-lock`, `fetch-native` and
//! `lock`.

use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::{DesktopLockOptions, FetchNativeOptions, SlotsCommand};

/// The toolchain, the prebuilt tools and the hooks.
pub(crate) fn setup(out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// The build slots.
pub(crate) fn slots(_command: SlotsCommand, out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// Holds `desktop.lock`.
pub(crate) fn desktop_lock(
    _options: DesktopLockOptions,
    out: &mut dyn Write,
) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// The pinned native libraries.
pub(crate) fn fetch_native(
    _options: FetchNativeOptions,
    out: &mut dyn Write,
) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

/// The lockfile, updated within the pins.
pub(crate) fn lock(out: &mut dyn Write) -> io::Result<ExitCode> {
    writeln!(out, "xtask: not built yet")?;
    Ok(ExitCode::from(2))
}

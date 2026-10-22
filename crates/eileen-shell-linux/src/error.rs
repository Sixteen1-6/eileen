//! The crate's own errors, for the files it reads and writes itself: [`ReadError`], a file every
//! Linux kernel provides is unreadable or incomplete, and [`AutostartError`], Eileen's autostart
//! entry could not be written or removed. D-Bus failures are [`crate::DbusError`].
//!
//! Each keeps only an I/O error's kind, never its text, which can carry a path.

use std::io;

/// A file every Linux kernel provides is unreadable, or lacks the line Eileen needs, so no
/// facts or no memory sample could be read at all. Everything else that cannot be read is a
/// `degraded` entry, not an error. It becomes `PlatformError::Io` once the probe and the
/// sampler implement the `eileen-platform` traits.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// The file could not be read.
    #[error("{what} is unreadable: {detail}")]
    Io {
        /// The file, such as `/proc/meminfo`.
        what: &'static str,
        /// What went wrong, as the I/O error's kind.
        detail: String,
    },
    /// The file lacks a line Eileen needs.
    #[error("{what} has no {field} line")]
    Missing {
        /// The file.
        what: &'static str,
        /// The line, such as `MemTotal`.
        field: &'static str,
    },
}

impl ReadError {
    /// Maps an error reading `what`. Only the error's kind is kept: an I/O error's text can
    /// carry a path.
    pub(crate) fn io(what: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |err| Self::Io { what, detail: err.kind().to_string() }
    }
}

/// Eileen's autostart entry could not be written or removed. The variants are
/// `eileen-platform`'s `PlatformError::Invalid` and `PlatformError::Io`, field for field and
/// with the same messages, so `ShellHost::set_run_at_sign_in` maps each one to one when the
/// trait lands.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AutostartError {
    /// The desktop file ID or the program's path cannot make an entry that starts Eileen.
    #[error("invalid {what}: {detail}")]
    Invalid {
        /// The argument, as a bare noun: `autostart ID` or `program path`.
        what: &'static str,
        /// What is wrong with it.
        detail: String,
    },
    /// Writing or removing the entry failed.
    #[error("{what}: {detail}")]
    Io {
        /// The file, as a bare noun: `autostart entry`.
        what: &'static str,
        /// What went wrong, as the I/O error's kind.
        detail: String,
    },
}

impl AutostartError {
    /// Maps an error writing or removing `what`. Only the error's kind is kept, as
    /// [`ReadError::io`] keeps it.
    pub(crate) fn io(what: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |err| Self::Io { what, detail: err.kind().to_string() }
    }
}

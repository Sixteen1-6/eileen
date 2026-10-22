//! [`ReadError`]: a file every Linux kernel provides is unreadable or incomplete.

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

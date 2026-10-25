//! The crate's own errors, for the files it reads and writes itself: [`ReadError`], a file every
//! Linux kernel provides is unreadable or incomplete, and [`AutostartError`], Eileen's autostart
//! entry could not be written or removed. D-Bus failures are [`crate::DbusError`].
//!
//! Each keeps only an I/O error's kind, never its text, which can carry a path, and each becomes
//! a `PlatformError` where the traits are served: [`LinuxSystemMemory`](crate::LinuxSystemMemory)'s
//! `SystemMemory` and [`LinuxShell`](crate::LinuxShell)'s `ShellHost`.

use std::io;

use eileen_platform::PlatformError;

/// A file every Linux kernel provides is unreadable, or lacks the line Eileen needs, so no
/// facts or no memory sample could be read at all. Everything else that cannot be read is a
/// `degraded` entry, not an error. It becomes `PlatformError::Io`, with the same file.
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
/// `eileen-platform`'s `PlatformError::NotFound`, `Refused`, `Invalid` and `Io`, field for field
/// and with the same messages, and `ShellHost::set_run_at_sign_in` maps each to its twin. An I/O
/// error becomes the variant that `eileen-apps-linux`'s `FileError` gives the same cause. None
/// holds a path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AutostartError {
    /// A folder or file the write needs went away, such as the autostart folder removed while the
    /// entry was written. An entry that is already absent when it is turned off is no error: it
    /// is off.
    #[error("{what} was not found")]
    NotFound {
        /// The file, as a bare noun: `autostart entry`.
        what: &'static str,
    },
    /// The system refused the write or the removal: the autostart folder, or the folder it is
    /// made in, may not be changed by this user.
    #[error("{what} was refused: {reason}")]
    Refused {
        /// The file, as a bare noun: `autostart entry`.
        what: &'static str,
        /// Why, in a few words: `not writable`.
        reason: &'static str,
    },
    /// The desktop file ID or the program's path cannot make an entry that starts Eileen.
    #[error("invalid {what}: {detail}")]
    Invalid {
        /// The argument, as a bare noun: `autostart ID` or `program path`.
        what: &'static str,
        /// What is wrong with it.
        detail: String,
    },
    /// Writing or removing the entry failed for another reason.
    #[error("{what}: {detail}")]
    Io {
        /// The file, as a bare noun: `autostart entry`.
        what: &'static str,
        /// What went wrong, as the I/O error's kind.
        detail: String,
    },
}

impl AutostartError {
    /// Maps an error writing or removing `what` as `eileen-apps-linux`'s `FileError::io` maps a
    /// read: a missing file or folder is [`Self::NotFound`], a permission the system refuses
    /// [`Self::Refused`], and anything else [`Self::Io`]. Only the error's kind is kept, as
    /// [`ReadError::io`] keeps it.
    pub(crate) fn io(what: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |err| {
            let kind = err.kind();
            if kind == io::ErrorKind::NotFound {
                Self::NotFound { what }
            } else if kind == io::ErrorKind::PermissionDenied {
                Self::Refused { what, reason: "not writable" }
            } else {
                Self::Io { what, detail: kind.to_string() }
            }
        }
    }
}

impl From<ReadError> for PlatformError {
    /// [`PlatformError::Io`] naming the same file; a missing line is its detail.
    fn from(error: ReadError) -> Self {
        match error {
            ReadError::Io { what, detail } => Self::Io { what, detail },
            ReadError::Missing { what, field } => {
                Self::Io { what, detail: format!("no {field} line") }
            }
        }
    }
}

impl From<AutostartError> for PlatformError {
    /// Each variant to its twin, field for field.
    fn from(error: AutostartError) -> Self {
        match error {
            AutostartError::NotFound { what } => Self::NotFound { what },
            AutostartError::Refused { what, reason } => Self::Refused { what, reason },
            AutostartError::Invalid { what, detail } => Self::Invalid { what, detail },
            AutostartError::Io { what, detail } => Self::Io { what, detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use eileen_platform::PlatformError;

    use super::{AutostartError, ReadError};

    #[test]
    fn an_autostart_io_error_maps_as_a_file_error_does() {
        let of = |kind| AutostartError::io("the entry")(io::Error::new(kind, "/home/sam/secret"));
        assert_eq!(
            of(io::ErrorKind::NotFound),
            AutostartError::NotFound { what: "the entry" },
            "gone"
        );
        assert_eq!(
            of(io::ErrorKind::PermissionDenied),
            AutostartError::Refused { what: "the entry", reason: "not writable" },
            "a folder that may not be changed"
        );
        let other = of(io::ErrorKind::AlreadyExists);
        assert_eq!(
            other,
            AutostartError::Io { what: "the entry", detail: "entity already exists".to_owned() },
            "any other kind"
        );
        assert!(!other.to_string().contains("sam"), "no text of the error's: {other}");
    }

    #[test]
    fn each_autostart_error_reads_as_the_platform_error_it_becomes() {
        let texts = [
            (AutostartError::NotFound { what: "autostart entry" }, "autostart entry was not found"),
            (
                AutostartError::Refused { what: "autostart entry", reason: "not writable" },
                "autostart entry was refused: not writable",
            ),
            (
                AutostartError::Invalid { what: "program path", detail: "not absolute".to_owned() },
                "invalid program path: not absolute",
            ),
            (
                AutostartError::Io { what: "autostart entry", detail: "is a directory".to_owned() },
                "autostart entry: is a directory",
            ),
        ];
        for (error, text) in texts {
            assert_eq!(error.to_string(), text, "{error:?}");
            assert_eq!(PlatformError::from(error).to_string(), text, "the same text as its twin");
        }
    }

    #[test]
    fn each_autostart_error_becomes_its_platform_twin() {
        let twins = [
            (AutostartError::NotFound { what: "e" }, PlatformError::NotFound { what: "e" }),
            (
                AutostartError::Refused { what: "e", reason: "r" },
                PlatformError::Refused { what: "e", reason: "r" },
            ),
            (
                AutostartError::Invalid { what: "e", detail: "d".to_owned() },
                PlatformError::Invalid { what: "e", detail: "d".to_owned() },
            ),
            (
                AutostartError::Io { what: "e", detail: "d".to_owned() },
                PlatformError::Io { what: "e", detail: "d".to_owned() },
            ),
        ];
        for (error, twin) in twins {
            assert_eq!(PlatformError::from(error), twin, "field for field");
        }
    }

    #[test]
    fn a_read_error_becomes_an_io_error_naming_the_same_file() {
        let unreadable = ReadError::Io { what: "/proc/vmstat", detail: "permission denied".into() };
        assert_eq!(
            PlatformError::from(unreadable),
            PlatformError::Io { what: "/proc/vmstat", detail: "permission denied".to_owned() },
            "unreadable"
        );
        let missing = ReadError::Missing { what: "/proc/meminfo", field: "MemAvailable" };
        let error = PlatformError::from(missing);
        assert_eq!(
            error,
            PlatformError::Io { what: "/proc/meminfo", detail: "no MemAvailable line".to_owned() },
            "a missing line"
        );
        assert_eq!(error.to_string(), "/proc/meminfo: no MemAvailable line", "reads well");
    }
}

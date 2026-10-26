//! [`FileError`]: what went wrong with a local file this crate reads, as `eileen-platform`'s
//! `PlatformError` says it. A failed D-Bus call is a [`DbusError`](crate::DbusError) instead.

use std::fmt;
use std::io;

use eileen_platform::PlatformError;

/// What went wrong with a local file: the one error type for every file this crate reads or
/// opens, such as `recently-used.xbel`. The variants are `eileen-platform`'s `PlatformError`
/// for files, field for field, and each becomes its twin through `From`. None holds a path or
/// the file's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// The file is not there.
    NotFound {
        /// The file, by its name or its role, such as `recently-used.xbel`.
        what: &'static str,
    },
    /// The file may not be read.
    Refused {
        /// The file.
        what: &'static str,
        /// Why, in a few words.
        reason: &'static str,
    },
    /// The file is not what its format says.
    Invalid {
        /// The file.
        what: &'static str,
        /// What is wrong with it, and about where.
        detail: String,
    },
    /// Reading the file failed.
    Io {
        /// The file.
        what: &'static str,
        /// The I/O error's kind.
        detail: String,
    },
}

impl FileError {
    /// What a failed open or read of `what` means. Only the error's kind is kept: an I/O
    /// error's text can carry a path.
    pub(crate) fn io(what: &'static str, error: &io::Error) -> Self {
        let kind = error.kind();
        if kind == io::ErrorKind::NotFound {
            Self::NotFound { what }
        } else if kind == io::ErrorKind::PermissionDenied {
            Self::Refused { what, reason: "not readable" }
        } else {
            Self::Io { what, detail: kind.to_string() }
        }
    }
}

/// The same texts as `PlatformError`'s.
impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { what } => write!(f, "{what} was not found"),
            Self::Refused { what, reason } => write!(f, "{what} was refused: {reason}"),
            Self::Invalid { what, detail } => write!(f, "invalid {what}: {detail}"),
            Self::Io { what, detail } => write!(f, "{what}: {detail}"),
        }
    }
}

impl std::error::Error for FileError {}

/// Each variant becomes the `PlatformError` variant of its name, with the same fields.
impl From<FileError> for PlatformError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::NotFound { what } => Self::NotFound { what },
            FileError::Refused { what, reason } => Self::Refused { what, reason },
            FileError::Invalid { what, detail } => Self::Invalid { what, detail },
            FileError::Io { what, detail } => Self::Io { what, detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use eileen_platform::PlatformError;

    use super::FileError;

    #[test]
    fn an_io_error_keeps_its_kind_and_drops_its_text() {
        let of = |kind| FileError::io("the file", &io::Error::new(kind, "/home/sam/secret"));
        assert_eq!(of(io::ErrorKind::NotFound), FileError::NotFound { what: "the file" }, "gone");
        assert_eq!(
            of(io::ErrorKind::PermissionDenied),
            FileError::Refused { what: "the file", reason: "not readable" },
            "not readable"
        );
        let other = of(io::ErrorKind::IsADirectory);
        assert_eq!(
            other,
            FileError::Io { what: "the file", detail: "is a directory".to_owned() },
            "any other kind"
        );
        assert!(!other.to_string().contains("sam"), "no text of the error's: {other}");
    }

    #[test]
    fn each_error_reads_as_the_platform_error_it_becomes() {
        let texts = [
            (FileError::NotFound { what: "the file" }, "the file was not found"),
            (
                FileError::Refused { what: "the file", reason: "not readable" },
                "the file was refused: not readable",
            ),
            (
                FileError::Invalid { what: "the file", detail: "not XML".to_owned() },
                "invalid the file: not XML",
            ),
            (
                FileError::Io { what: "the file", detail: "is a directory".to_owned() },
                "the file: is a directory",
            ),
        ];
        for (error, text) in texts {
            assert_eq!(error.to_string(), text, "{error:?}");
            assert_eq!(PlatformError::from(error).to_string(), text, "and as its twin");
        }
    }

    #[test]
    fn each_error_becomes_its_platform_twin() {
        let pairs = [
            (FileError::NotFound { what: "a" }, PlatformError::NotFound { what: "a" }),
            (
                FileError::Refused { what: "a", reason: "b" },
                PlatformError::Refused { what: "a", reason: "b" },
            ),
            (
                FileError::Invalid { what: "a", detail: "b".to_owned() },
                PlatformError::Invalid { what: "a", detail: "b".to_owned() },
            ),
            (
                FileError::Io { what: "a", detail: "b".to_owned() },
                PlatformError::Io { what: "a", detail: "b".to_owned() },
            ),
        ];
        for (error, twin) in pairs {
            assert_eq!(PlatformError::from(error.clone()), twin, "{error:?}");
        }
    }
}

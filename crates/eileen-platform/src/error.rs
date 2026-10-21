//! The one error type every platform trait returns.

/// What went wrong in a platform call. Implementations convert every `HRESULT`, `BOOL` and OS
/// error code into one of these inside their `ffi` module.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    /// Not on this OS, or not built yet (the stubs answer this).
    #[error("{what} is not available here")]
    Unavailable {
        /// What was asked for.
        what: &'static str,
    },
    /// The OS refused the call.
    #[error("{call} failed with OS error {code}")]
    Os {
        /// The OS function that failed.
        call: &'static str,
        /// Its error code (`HRESULT`, `GetLastError` or `errno`).
        code: i64,
    },
    /// The call passed its deadline; the implementation abandoned it.
    #[error("{call} passed its deadline")]
    DeadlineExceeded {
        /// The call that ran too long.
        call: &'static str,
    },
    /// The OS or another app refused, for a reason the implementation knows.
    #[error("{what} was refused: {reason}")]
    Refused {
        /// What was refused.
        what: &'static str,
        /// Why, in a few words.
        reason: &'static str,
    },
    /// The window, element, app or file is gone or never existed.
    #[error("{what} was not found")]
    NotFound {
        /// What was looked for.
        what: &'static str,
    },
    /// An argument the implementation cannot use.
    #[error("invalid {what}: {detail}")]
    Invalid {
        /// The argument.
        what: &'static str,
        /// What is wrong with it.
        detail: String,
    },
    /// Reading or writing a file failed.
    #[error("{what}: {detail}")]
    Io {
        /// The file or operation.
        what: &'static str,
        /// The OS's message.
        detail: String,
    },
}

impl PlatformError {
    /// An [`Self::Io`] from a `std::io::Error`.
    #[must_use]
    pub fn io(what: &'static str, error: &std::io::Error) -> Self {
        Self::Io { what, detail: error.to_string() }
    }
}

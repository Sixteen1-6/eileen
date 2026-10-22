//! D-Bus plumbing shared by this crate's services: [`DbusError`], what a failed call means, and
//! connections to the real session and system buses. Every service module takes a
//! `zbus::Connection` from its caller, so the tests hand it a private bus instead.

/// What went wrong talking to a D-Bus service. The variants are `eileen-platform`'s
/// `PlatformError` (on the unmerged base crates) field for field, so each maps one to one when
/// the traits land.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DbusError {
    /// The bus or the service is not there: no session bus, or nothing owns the name.
    #[error("{what} is not available here")]
    Unavailable {
        /// What was asked for.
        what: &'static str,
    },
    /// The call did not finish in time.
    #[error("{call} passed its deadline")]
    DeadlineExceeded {
        /// The call that ran too long.
        call: &'static str,
    },
    /// The service refused, for a reason its error name gives.
    #[error("{what} was refused: {reason}")]
    Refused {
        /// What was refused.
        what: &'static str,
        /// Why, in a few words.
        reason: &'static str,
    },
    /// The service runs but has no such object, interface, method or property: an older or
    /// different implementation.
    #[error("{what} was not found")]
    NotFound {
        /// What was looked for.
        what: &'static str,
    },
    /// The service sent something this client cannot use, or refused the arguments.
    #[error("invalid {what}: {detail}")]
    Invalid {
        /// The call or the value.
        what: &'static str,
        /// What is wrong with it.
        detail: String,
    },
    /// The connection itself failed.
    #[error("{what}: {detail}")]
    Io {
        /// The call or the bus.
        what: &'static str,
        /// The error's kind.
        detail: String,
    },
}

impl DbusError {
    /// What a failed call to `what` means. Only error names and kinds are kept: a service's
    /// message can carry anything.
    #[must_use]
    pub fn from_zbus(what: &'static str, error: &zbus::Error) -> Self {
        match error {
            zbus::Error::MethodError(name, _, _) => Self::from_error_name(what, name.as_str()),
            zbus::Error::FDO(fdo) => {
                Self::from_error_name(what, zbus::DBusError::name(fdo.as_ref()).as_str())
            }
            zbus::Error::InputOutput(io) => Self::Io { what, detail: io.kind().to_string() },
            zbus::Error::Address(_) | zbus::Error::Connection(_, _) => Self::Unavailable { what },
            zbus::Error::Handshake(_) => Self::Io { what, detail: "handshake failed".to_owned() },
            zbus::Error::InterfaceNotFound
            | zbus::Error::InvalidField
            | zbus::Error::ExcessData
            | zbus::Error::Variant(_)
            | zbus::Error::Names(_)
            | zbus::Error::IncorrectEndian
            | zbus::Error::InvalidReply
            | zbus::Error::MissingField
            | zbus::Error::InvalidGUID
            | zbus::Error::Unsupported
            | zbus::Error::NameTaken
            | zbus::Error::InvalidMatchRule
            | zbus::Error::Failure(_)
            | zbus::Error::MissingParameter(_)
            | zbus::Error::InvalidSerial
            | zbus::Error::InterfaceExists(..) => Self::Invalid { what, detail: error.to_string() },
            // A kind of failure a later zbus adds: reported as a failed connection.
            _ => Self::Io { what, detail: error.to_string() },
        }
    }

    /// What a D-Bus error name means, such as `org.freedesktop.DBus.Error.ServiceUnknown`.
    #[must_use]
    pub fn from_error_name(what: &'static str, name: &str) -> Self {
        let short = name.rsplit('.').next().unwrap_or(name);
        match short {
            "ServiceUnknown" | "NameHasNoOwner" | "Disconnected" => Self::Unavailable { what },
            "NoReply" | "Timeout" | "TimedOut" => Self::DeadlineExceeded { call: what },
            "AccessDenied" | "AuthFailed" | "InteractiveAuthorizationRequired" => {
                Self::Refused { what, reason: "not authorized" }
            }
            "NotSupported" => Self::Refused { what, reason: "not supported" },
            "UnknownMethod" | "UnknownObject" | "UnknownInterface" | "UnknownProperty" => {
                Self::NotFound { what }
            }
            _ => Self::Invalid { what, detail: name.to_owned() },
        }
    }
}

/// The user's session bus.
pub async fn session_bus() -> Result<zbus::Connection, DbusError> {
    zbus::Connection::session()
        .await
        .map_err(|error| DbusError::from_zbus("the session bus", &error))
}

/// The system bus.
pub async fn system_bus() -> Result<zbus::Connection, DbusError> {
    zbus::Connection::system().await.map_err(|error| DbusError::from_zbus("the system bus", &error))
}

#[cfg(test)]
mod tests {
    use super::DbusError;

    #[test]
    fn error_names_map_to_what_they_mean() {
        let of = |name: &str| DbusError::from_error_name("the call", name);
        assert_eq!(
            of("org.freedesktop.DBus.Error.ServiceUnknown"),
            DbusError::Unavailable { what: "the call" },
            "nothing owns the name"
        );
        assert_eq!(
            of("org.freedesktop.DBus.Error.NoReply"),
            DbusError::DeadlineExceeded { call: "the call" },
            "no answer"
        );
        assert_eq!(
            of("org.freedesktop.DBus.Error.AccessDenied"),
            DbusError::Refused { what: "the call", reason: "not authorized" },
            "refused"
        );
        assert_eq!(
            of("org.freedesktop.DBus.Error.UnknownMethod"),
            DbusError::NotFound { what: "the call" },
            "an older service"
        );
        assert_eq!(
            of("org.example.Custom.Failed"),
            DbusError::Invalid { what: "the call", detail: "org.example.Custom.Failed".to_owned() },
            "anything else keeps its name"
        );
    }
}

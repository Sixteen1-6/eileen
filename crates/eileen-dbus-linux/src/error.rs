//! [`DbusError`]: what a failed D-Bus call means, as `eileen-platform`'s `PlatformError` says it.
//! The mappings follow the D-Bus specification's error names
//! ([the specification](https://dbus.freedesktop.org/doc/dbus-specification.html#message-protocol-names-error)),
//! systemd's and dbus-daemon's activation errors, the portal's `org.freedesktop.portal.Error`
//! names and logind's `NoSessionForPID`.

use eileen_platform::PlatformError;

/// How the error names begin that the bus answers a call with when it could not start the
/// service for it: systemd's, when it would not or could not start the service's unit (such as
/// `org.freedesktop.systemd1.UnitMasked`), and those of dbus-daemon's own launcher (such as
/// `org.freedesktop.DBus.Error.Spawn.ChildExited`). The service is not there.
const ACTIVATION_FAILURES: [&str; 2] =
    ["org.freedesktop.systemd1.", "org.freedesktop.DBus.Error.Spawn."];

/// The portal's answer when the desktop's lockdown or the permission store turns the request
/// off.
const PORTAL_NOT_ALLOWED: &str = "org.freedesktop.portal.Error.NotAllowed";

/// logind's answer to a caller that has no session and no display session: there is no session
/// to find.
const LOGIND_NO_SESSION: &str = "org.freedesktop.login1.NoSessionForPID";

/// What went wrong talking to a D-Bus service. The variants are [`PlatformError`]'s field for
/// field, and `From` maps each to its twin, so a trait implementation answers with `?`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DbusError {
    /// The bus or the service is not there: no session bus, nothing owns the name, or the bus
    /// could not start the service.
    #[error("{what} is not available here")]
    Unavailable {
        /// What was asked for.
        what: &'static str,
    },
    /// The call did not finish in time: the connection's method timeout passed, or the bus gave
    /// up first.
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
        /// The call or the value, as a bare noun.
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
    /// message can carry anything. A connection's method timeout, which zbus reports as an I/O
    /// error of kind `TimedOut`, is [`DbusError::DeadlineExceeded`].
    #[must_use]
    pub fn from_zbus(what: &'static str, error: &zbus::Error) -> Self {
        match error {
            zbus::Error::MethodError(name, _, _) => Self::from_error_name(what, name.as_str()),
            zbus::Error::FDO(fdo) => {
                Self::from_error_name(what, zbus::DBusError::name(fdo.as_ref()).as_str())
            }
            zbus::Error::InputOutput(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                Self::DeadlineExceeded { call: what }
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
        if ACTIVATION_FAILURES.iter().any(|prefix| name.starts_with(prefix)) {
            return Self::Unavailable { what };
        }
        if name == PORTAL_NOT_ALLOWED {
            return Self::Refused { what, reason: "not allowed" };
        }
        if name == LOGIND_NO_SESSION {
            return Self::NotFound { what };
        }
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

impl From<DbusError> for PlatformError {
    fn from(error: DbusError) -> Self {
        match error {
            DbusError::Unavailable { what } => Self::Unavailable { what },
            DbusError::DeadlineExceeded { call } => Self::DeadlineExceeded { call },
            DbusError::Refused { what, reason } => Self::Refused { what, reason },
            DbusError::NotFound { what } => Self::NotFound { what },
            DbusError::Invalid { what, detail } => Self::Invalid { what, detail },
            DbusError::Io { what, detail } => Self::Io { what, detail },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;

    use eileen_platform::PlatformError;

    use super::DbusError;

    #[test]
    fn each_error_becomes_its_platform_twin_with_the_same_text() {
        let pairs = [
            (DbusError::Unavailable { what: "a" }, PlatformError::Unavailable { what: "a" }),
            (
                DbusError::DeadlineExceeded { call: "a" },
                PlatformError::DeadlineExceeded { call: "a" },
            ),
            (
                DbusError::Refused { what: "a", reason: "b" },
                PlatformError::Refused { what: "a", reason: "b" },
            ),
            (DbusError::NotFound { what: "a" }, PlatformError::NotFound { what: "a" }),
            (
                DbusError::Invalid { what: "a", detail: "b".to_owned() },
                PlatformError::Invalid { what: "a", detail: "b".to_owned() },
            ),
            (
                DbusError::Io { what: "a", detail: "b".to_owned() },
                PlatformError::Io { what: "a", detail: "b".to_owned() },
            ),
        ];
        for (dbus, platform) in pairs {
            let text = dbus.to_string();
            let mapped = PlatformError::from(dbus);
            assert_eq!(mapped, platform, "{text}");
            assert_eq!(mapped.to_string(), text, "the same message");
        }
    }

    /// What `name` means for a call named "the call".
    fn of(name: &str) -> DbusError {
        DbusError::from_error_name("the call", name)
    }

    #[test]
    fn the_buses_error_names_map_to_what_they_mean() {
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

    #[test]
    fn a_service_the_bus_could_not_start_is_unavailable() {
        for name in [
            "org.freedesktop.systemd1.UnitMasked",
            "org.freedesktop.systemd1.NoSuchUnit",
            "org.freedesktop.DBus.Error.Spawn.ChildExited",
            "org.freedesktop.DBus.Error.Spawn.ExecFailed",
        ] {
            assert_eq!(of(name), DbusError::Unavailable { what: "the call" }, "{name}");
        }
    }

    #[test]
    fn the_portals_not_allowed_is_a_refusal() {
        assert_eq!(
            of("org.freedesktop.portal.Error.NotAllowed"),
            DbusError::Refused { what: "the call", reason: "not allowed" },
            "the desktop's lockdown"
        );
        assert_eq!(
            of("org.example.Error.NotAllowed"),
            DbusError::Invalid {
                what: "the call",
                detail: "org.example.Error.NotAllowed".to_owned()
            },
            "only the portal's name: another service's NotAllowed keeps its name"
        );
    }

    #[test]
    fn loginds_no_session_for_pid_is_not_found() {
        assert_eq!(
            of("org.freedesktop.login1.NoSessionForPID"),
            DbusError::NotFound { what: "the call" },
            "a caller in no session and no display session has none to find"
        );
        assert_eq!(
            of("org.example.Error.NoSessionForPID"),
            DbusError::Invalid {
                what: "the call",
                detail: "org.example.Error.NoSessionForPID".to_owned()
            },
            "only logind's name: another service's NoSessionForPID keeps its name"
        );
    }

    #[test]
    fn a_method_timeout_is_a_passed_deadline_and_other_io_keeps_its_kind() {
        let timed_out =
            zbus::Error::InputOutput(Arc::new(io::Error::from(io::ErrorKind::TimedOut)));
        assert_eq!(
            DbusError::from_zbus("the call", &timed_out),
            DbusError::DeadlineExceeded { call: "the call" },
            "zbus reports the connection's method timeout as TimedOut"
        );
        let reset =
            zbus::Error::InputOutput(Arc::new(io::Error::from(io::ErrorKind::ConnectionReset)));
        assert_eq!(
            DbusError::from_zbus("the call", &reset),
            DbusError::Io { what: "the call", detail: io::ErrorKind::ConnectionReset.to_string() },
            "any other I/O failure"
        );
    }

    #[test]
    fn a_method_error_keeps_its_name_and_drops_its_message() {
        let error = zbus::Error::MethodError(
            "org.example.Custom.Failed".try_into().unwrap(),
            Some("/home/someone/secret".to_owned()),
            zbus::Message::method_call("/", "Get").unwrap().build(&()).unwrap(),
        );
        let mapped = DbusError::from_zbus("the call", &error);
        assert!(!mapped.to_string().contains("someone"), "no service text: {mapped}");
    }
}

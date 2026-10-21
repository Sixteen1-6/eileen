//! Identifiers, time, correlation and text (BUILD.md section 3.3.1).
//!
//! IDs are prefixed ULIDs: `Display` writes `job_01K8...` and `FromStr` requires the prefix and
//! the canonical upper-case form. The core mints them from `MonoTime` plus its injected RNG;
//! drivers go through the `eileen_stdx` Ids port. Nothing here reads a clock or randomness.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;

use crate::schema::Registry;

mod text;
mod time;

pub use text::{Personal, Untrusted};
pub use time::{Date, MonoTime, WallTime};

#[cfg(test)]
mod tests;

/// Why a text form did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// An ID without its prefix, for example `01K8...` where `job_01K8...` was expected.
    MissingPrefix {
        /// The prefix the type requires.
        expected: &'static str,
    },
    /// The part after the prefix is not 26 upper-case Crockford base32 characters.
    BadUlid,
    /// A validated string that breaks its rule.
    Invalid {
        /// The type whose rule failed.
        what: &'static str,
        /// The rule, in words.
        rule: &'static str,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrefix { expected } => write!(f, "expected the prefix `{expected}_`"),
            Self::BadUlid => {
                f.write_str("expected 26 upper-case Crockford base32 characters after the prefix")
            }
            Self::Invalid { what, rule } => write!(f, "not a valid {what}: {rule}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// The Crockford base32 alphabet ULIDs use, upper case.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// True for a canonical ULID text: 26 upper-case Crockford characters, the first at most `7`.
fn is_canonical_ulid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 26
        && bytes.first().is_some_and(|b| (b'0'..=b'7').contains(b))
        && bytes.iter().all(|b| CROCKFORD.contains(b))
}

/// Parses `<prefix>_<ulid>`.
fn parse_prefixed(text: &str, prefix: &'static str) -> Result<Ulid, ParseError> {
    let rest = text
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .ok_or(ParseError::MissingPrefix { expected: prefix })?;
    if !is_canonical_ulid(rest) {
        return Err(ParseError::BadUlid);
    }
    Ulid::from_string(rest).map_err(|_| ParseError::BadUlid)
}

/// Declares a prefixed-ULID newtype with its text form, serde and schema.
macro_rules! prefixed_ulid {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Ulid);

        impl $name {
            /// The text prefix, written before the underscore.
            pub const PREFIX: &'static str = $prefix;

            /// Wraps a ULID the caller minted.
            #[must_use]
            pub const fn from_ulid(ulid: Ulid) -> Self {
                Self(ulid)
            }

            /// Builds the ID from a millisecond timestamp and 80 random bits, the way the core
            /// mints IDs from its clock and RNG.
            #[must_use]
            pub const fn from_parts(timestamp_ms: u64, random: u128) -> Self {
                Self(Ulid::from_parts(timestamp_ms, random))
            }

            /// The ULID inside.
            #[must_use]
            pub const fn ulid(self) -> Ulid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                parse_prefixed(text, $prefix).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = Cow::<'de, str>::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "pattern": concat!("^", $prefix, "_[0-7][0-9A-HJKMNP-TV-Z]{25}$"),
                    "description": concat!(
                        "A ULID with the prefix `", $prefix, "_`, upper-case Crockford base32"
                    ),
                })
            }
        }
    };
}

prefixed_ulid!(
    /// One run of the core, from start to exit.
    SessionId,
    "sess"
);
prefixed_ulid!(
    /// One conversational turn, minted by the speech process at the first speech onset after the
    /// previous end of turn (or by the core for typed and other turns).
    TurnId,
    "turn"
);
prefixed_ulid!(
    /// One job, from dispatch to its terminal state.
    JobId,
    "job"
);
prefixed_ulid!(
    /// One worker session (a vendor CLI process), pooled or bound to a job.
    WorkerSessionId,
    "ws"
);
prefixed_ulid!(
    /// One tool call through the dispatcher.
    CallId,
    "call"
);
prefixed_ulid!(
    /// One spoken utterance of Eileen's, split into clauses.
    SpeakId,
    "speak"
);
prefixed_ulid!(
    /// One approval question put to the user.
    ApprovalId,
    "appr"
);
prefixed_ulid!(
    /// One captured incident bundle.
    IncidentId,
    "inc"
);
prefixed_ulid!(
    /// One recording made with `eileen record`.
    RecordingId,
    "rec"
);
prefixed_ulid!(
    /// One settings proposal.
    ProposalId,
    "prop"
);
prefixed_ulid!(
    /// One app route proposal from a worker.
    RouteProposalId,
    "route"
);

/// Declares a string newtype validated on construction, with serde and a schema pattern.
macro_rules! validated_string {
    (
        $(#[$meta:meta])* $name:ident,
        rule: $rule:literal,
        pattern: $pattern:literal,
        check: $check:path
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Validates and wraps the text.
            pub fn new(text: impl Into<String>) -> Result<Self, ParseError> {
                let text = text.into();
                if $check(&text) {
                    Ok(Self(text))
                } else {
                    Err(ParseError::Invalid { what: stringify!($name), rule: $rule })
                }
            }

            /// The text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Self::new(text)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ParseError;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                Self::new(text)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({ "type": "string", "pattern": $pattern, "description": $rule })
            }
        }
    };
}

/// True for 1 to 64 lower-case letters, digits and hyphens.
fn is_skill_id(text: &str) -> bool {
    (1..=64).contains(&text.len()) && text.bytes().all(is_slug_byte)
}

/// True for at least one lower-case letter, digit or hyphen.
fn is_scenario_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(is_slug_byte)
}

const fn is_slug_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
}

/// True for `^[a-z][a-z0-9_]{2,62}$`.
fn is_tool_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    (3..=63).contains(&text.len())
        && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// True for dot-separated lower-case segments, each starting with a letter, at most 64 bytes.
fn is_connector_id(text: &str) -> bool {
    text.len() <= 64
        && text.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            bytes.next().is_some_and(|b| b.is_ascii_lowercase())
                && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// True for exactly 64 lower-case hex digits.
fn is_machine_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(is_lower_hex)
}

/// True for a lower-case hex digit.
pub(crate) const fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || matches!(b, b'a'..=b'f')
}

validated_string!(
    /// An Agent Skills name: 1 to 64 lower-case letters, digits and hyphens.
    SkillId,
    rule: "1 to 64 lower-case letters, digits and hyphens",
    pattern: "^[a-z0-9-]{1,64}$",
    check: is_skill_id
);
validated_string!(
    /// A scenario folder slug: lower-case letters, digits and hyphens.
    ScenarioId,
    rule: "lower-case letters, digits and hyphens",
    pattern: "^[a-z0-9-]+$",
    check: is_scenario_id
);
validated_string!(
    /// A tool name, unique across connectors, with a toolset prefix (`desktop_`, `apps_`,
    /// `jobs_`, ...).
    ToolName,
    rule: "a lower-case letter, then 2 to 62 lower-case letters, digits and underscores",
    pattern: "^[a-z][a-z0-9_]{2,62}$",
    check: is_tool_name
);
validated_string!(
    /// A connector, for example `pc.desktop` or `pc.apps`, later `phone` or `home`.
    ConnectorId,
    rule: "dot-separated lower-case segments, each starting with a letter, at most 64 bytes",
    pattern: "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$",
    check: is_connector_id
);
validated_string!(
    /// The fleet's machine identity (owner answer 6): 64 lower-case hex characters, BLAKE3 over
    /// a fixed public salt, the normalised OS machine id and the install id, each
    /// length-prefixed (`eileen_platform::machine::machine_hash`). It holds no hardware serial,
    /// user name or account, so a shared scenario keeps it.
    MachineHash,
    rule: "64 lower-case hex characters",
    pattern: "^[0-9a-f]{64}$",
    check: is_machine_hash
);

impl MachineHash {
    /// Renders a 32-byte digest as the 64-character hex form.
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(hex_lower(&digest))
    }
}

/// Lower-case hex of the bytes.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        for nibble in [byte >> 4, byte & 0x0f] {
            if let Some(&digit) = DIGITS.get(usize::from(nibble)) {
                out.push(char::from(digit));
            }
        }
    }
    out
}

/// 128 random bits, created once by `eileen setup` or the first `eileen run` (never by the core,
/// which reads no OS randomness) and kept only in `fleet\machine.toml`. Serialized as 32
/// lower-case hex characters. `Debug` does not show it, because it never leaves the PC.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstallId([u8; 16]);

impl InstallId {
    /// Wraps the bytes the caller drew from the OS.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The bytes, for the machine hash.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for InstallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstallId([hidden])")
    }
}

impl fmt::Display for InstallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex_lower(&self.0))
    }
}

impl FromStr for InstallId {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        const INVALID: ParseError =
            ParseError::Invalid { what: "InstallId", rule: "32 lower-case hex characters" };
        if text.len() != 32 || !text.bytes().all(is_lower_hex) {
            return Err(INVALID);
        }
        let mut bytes = [0_u8; 16];
        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        for (slot, [high, low]) in bytes.iter_mut().zip(pairs) {
            *slot = (hex_value(*high) << 4) | hex_value(*low);
        }
        Ok(Self(bytes))
    }
}

/// The value of one lower-case hex digit; the caller has checked it is one.
const fn hex_value(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => 0,
    }
}

impl Serialize for InstallId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for InstallId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Cow::<'de, str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for InstallId {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("InstallId")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9a-f]{32}$",
            "description": "128 random bits as 32 lower-case hex characters",
        })
    }
}

/// Correlation carried by every bus event, IPC frame and tool call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TraceCtx {
    /// The turn this belongs to, if any.
    pub turn: Option<TurnId>,
    /// The job this belongs to, if any.
    pub job: Option<JobId>,
    /// The tool call this belongs to, if any.
    pub call: Option<CallId>,
}

impl TraceCtx {
    /// The W3C `traceparent` value for a span of this context:
    /// `00-<32 hex of the turn ULID, or the job ULID>-<16 hex span>-01`. `None` when the
    /// context has neither a turn nor a job, or when the trace or span id would be all zeros,
    /// which the W3C format forbids.
    #[must_use]
    pub fn traceparent(&self, span: u64) -> Option<String> {
        let trace = self.turn.map(TurnId::ulid).or_else(|| self.job.map(JobId::ulid))?;
        let trace = trace.0;
        (trace != 0 && span != 0).then(|| format!("00-{trace:032x}-{span:016x}-01"))
    }
}

/// Every bus event, input and effect travels in this envelope. `seq` is the core-wide replay
/// order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Envelope_{E}")]
pub struct Envelope<E> {
    /// The core-wide replay order.
    pub seq: u64,
    /// Wall-clock time when it was made.
    pub at: WallTime,
    /// Monotonic time when it was made.
    pub mono: MonoTime,
    /// Its correlation.
    pub ctx: TraceCtx,
    /// What it carries.
    pub body: E,
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<SessionId>();
    r.add::<TurnId>();
    r.add::<JobId>();
    r.add::<WorkerSessionId>();
    r.add::<CallId>();
    r.add::<SpeakId>();
    r.add::<ApprovalId>();
    r.add::<IncidentId>();
    r.add::<RecordingId>();
    r.add::<ProposalId>();
    r.add::<RouteProposalId>();
    r.add::<MachineHash>();
    r.add::<InstallId>();
    r.add::<SkillId>();
    r.add::<ScenarioId>();
    r.add::<ToolName>();
    r.add::<ConnectorId>();
    r.add::<MonoTime>();
    r.add::<WallTime>();
    r.add::<Date>();
    r.add::<TraceCtx>();
}

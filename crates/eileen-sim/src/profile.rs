//! Simulation profiles: how long a run is, how the inputs are mixed and the limits the world
//! keeps. Each profile is a TOML file in `crates/eileen-sim/profiles/`, compiled into the
//! binary, and checked against the JSON Schema generated from [`Profile`]
//! (`profiles/profile.schema.json`, regenerated with `eileen-sim --print-schema`).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The profiles this build knows, by name, with their TOML text.
const PROFILES: &[(&str, &str)] = &[("default", include_str!("../profiles/default.toml"))];

/// One simulation profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// The profile's name, which a `SimRegression` records.
    pub name: String,
    /// Steps in one run.
    pub steps: u32,
    /// How many fresh seeds a random run of this profile takes (`main` and nightly).
    pub seeds: u32,
    /// The longest virtual time between two inputs, ms.
    pub max_step_ms: u32,
    /// The relative weights of the input kinds the generator draws.
    pub input_mix: InputMix,
    /// The most utterances the simulated speech process may hold queued (invariant 10).
    pub speech_queue_max: u32,
    /// The slow bus subscriber's queue (invariant 6).
    pub bus_subscriber_capacity: u32,
    /// How many events the slow subscriber takes off its queue per step.
    pub bus_subscriber_drain: u32,
}

/// The relative weights of the input kinds. A weight of 0 leaves the kind out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputMix {
    /// Speech from the microphone: speech start, partials, speech end, end of turn.
    pub voice: u32,
    /// Mute, unmute and typed or command-line text.
    pub user: u32,
    /// Playback accounting: drained utterances, device changes and stalls.
    pub playback: u32,
    /// A timer the core set firing.
    pub timer: u32,
}

impl InputMix {
    /// The sum of the weights.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.voice
            .saturating_add(self.user)
            .saturating_add(self.playback)
            .saturating_add(self.timer)
    }
}

/// A profile that could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// No profile has this name.
    Unknown {
        /// The name asked for.
        name: String,
    },
    /// The profile's TOML does not parse or breaks a rule.
    Invalid {
        /// The profile.
        name: String,
        /// Why.
        reason: String,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { name } => {
                write!(f, "no simulation profile is named `{name}` (known: {})", names().join(", "))
            }
            Self::Invalid { name, reason } => write!(f, "profile `{name}` is invalid: {reason}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl Profile {
    /// The profile named `name`.
    pub fn load(name: &str) -> Result<Self, ProfileError> {
        let Some((_, text)) = PROFILES.iter().find(|(known, _)| *known == name) else {
            return Err(ProfileError::Unknown { name: name.to_owned() });
        };
        Self::parse(name, text)
    }

    /// The default profile.
    pub fn default_profile() -> Result<Self, ProfileError> {
        Self::load("default")
    }

    /// Parses a profile's TOML and checks its rules: the name matches, a run has steps, and
    /// at least one input kind has a weight.
    pub fn parse(name: &str, text: &str) -> Result<Self, ProfileError> {
        let invalid = |reason: String| ProfileError::Invalid { name: name.to_owned(), reason };
        let profile: Self = toml::from_str(text).map_err(|e| invalid(e.message().to_owned()))?;
        if profile.name != name {
            return Err(invalid(format!("its `name` is `{}`", profile.name)));
        }
        if profile.steps == 0 || profile.max_step_ms == 0 {
            return Err(invalid("`steps` and `max_step_ms` must be at least 1".to_owned()));
        }
        if profile.input_mix.total() == 0 {
            return Err(invalid("every input weight is 0".to_owned()));
        }
        if profile.bus_subscriber_capacity == 0 || profile.bus_subscriber_drain == 0 {
            return Err(invalid("the bus subscriber needs a capacity and a drain".to_owned()));
        }
        Ok(profile)
    }
}

/// The names of the profiles this build knows.
#[must_use]
pub fn names() -> Vec<&'static str> {
    PROFILES.iter().map(|(name, _)| *name).collect()
}

/// The JSON Schema of [`Profile`], as `profiles/profile.schema.json` holds it.
#[must_use]
pub fn schema_json() -> String {
    let schema = schemars::schema_for!(Profile);
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_default();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::{Profile, ProfileError, names, schema_json};

    #[test]
    fn every_known_profile_loads() {
        for name in names() {
            let profile = Profile::load(name).unwrap();
            assert_eq!(profile.name, name, "the profile names itself");
        }
    }

    #[test]
    fn an_unknown_profile_names_the_known_ones() {
        let error = Profile::load("roomy").unwrap_err();
        assert!(matches!(error, ProfileError::Unknown { .. }), "{error}");
        assert!(error.to_string().contains("default"), "{error}");
    }

    #[test]
    fn a_profile_breaking_a_rule_is_refused() {
        let text = include_str!("../profiles/default.toml");
        let renamed = Profile::parse("other", text).unwrap_err();
        assert!(renamed.to_string().contains("`name`"), "{renamed}");
        let silent = text.replace("voice = ", "voice = 0 #").replace("user = ", "user = 0 #");
        let silent =
            silent.replace("playback = ", "playback = 0 #").replace("timer = ", "timer = 0 #");
        let error = Profile::parse("default", &silent).unwrap_err();
        assert!(error.to_string().contains("weight"), "{error}");
        let unknown = format!("{text}\nflavour = 1\n");
        assert!(Profile::parse("default", &unknown).is_err(), "unknown keys are refused");
    }

    #[test]
    fn the_checked_in_schema_is_current() {
        let checked_in = include_str!("../profiles/profile.schema.json");
        assert_eq!(
            checked_in.replace("\r\n", "\n"),
            schema_json(),
            "profiles/profile.schema.json is stale; regenerate it with \
             `cargo xtask sim --print-schema` and put the JSON it prints in that file"
        );
    }
}

//! Simulation profiles: how long a run is, how the inputs are mixed, the limits the world keeps,
//! the PC the world simulates (its memory, paging, CPU load and power, and a heavy app that
//! grows and closes), how the fake brain answers, and how often fault points fire. Each profile
//! is a TOML file in `crates/eileen-sim/profiles/`, compiled into the binary, and checked
//! against the JSON Schema generated from [`Profile`] (`profiles/profile.schema.json`,
//! regenerated with `eileen-sim --print-schema`).
//!
//! The profiles of section 4.7 built so far: `default`, `barge-in-heavy`, `many-jobs`,
//! `flaky-sidecars`, `media-playing` and `low-memory`. `heavy-app` and `roomy` follow with the
//! machine's facts (`wp1/machine-fit`), whose fixture machines they run on.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The profiles this build knows, by name, with their TOML text.
const PROFILES: &[(&str, &str)] = &[
    ("default", include_str!("../profiles/default.toml")),
    ("barge-in-heavy", include_str!("../profiles/barge-in-heavy.toml")),
    ("many-jobs", include_str!("../profiles/many-jobs.toml")),
    ("flaky-sidecars", include_str!("../profiles/flaky-sidecars.toml")),
    ("media-playing", include_str!("../profiles/media-playing.toml")),
    ("low-memory", include_str!("../profiles/low-memory.toml")),
];

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
    /// The chance, in percent, that a voice input drawn while Eileen speaks is a barge-in
    /// window rather than the next event of the user's turn.
    pub barge_in_pct: u8,
    /// Other audio (a video, music) is playing when the run starts.
    #[serde(default)]
    pub media_at_start: bool,
    /// The PC the world simulates.
    pub pc: PcProfile,
    /// What Eileen's own processes hold in memory.
    pub eileen: EileenCosts,
    /// How the fake brain backend answers.
    pub brain: BrainProfile,
    /// How often the world's fault points fire.
    pub faults: FaultProfile,
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
    /// What the PC and the room do: other audio starting and stopping, physical input, and
    /// another app taking the microphone.
    pub activity: u32,
}

impl InputMix {
    /// The sum of the weights.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.voice
            .saturating_add(self.user)
            .saturating_add(self.playback)
            .saturating_add(self.timer)
            .saturating_add(self.activity)
    }
}

/// The simulated PC: its memory, its paging, its CPU and its power, and the other apps on it.
/// Every figure is in MB unless its name says otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PcProfile {
    /// Physical memory.
    pub physical_mb: u32,
    /// The commit limit: physical memory and the page file together.
    pub commit_limit_mb: u32,
    /// What the other apps hold in physical memory, a heavy app aside.
    pub others_mb: u32,
    /// What the other apps have committed, a heavy app aside.
    pub others_commit_mb: u32,
    /// The biggest of the other apps, by a made-up name (`browser`), never a real process's.
    pub largest: String,
    /// The biggest other app's working set, part of `others_mb`.
    pub largest_mb: u32,
    /// Available memory under which the PC starts to page.
    pub paging_floor_mb: u32,
    /// How fast it pages, KB/s, for each MB of available memory under the floor.
    pub paging_kb_s_per_mb: u32,
    /// The effective `memory.max_paging_kb_s` the runtime reports with each reading.
    pub paging_limit_kb_s: u32,
    /// The CPU's busy percent with no heavy app open.
    pub cpu_busy_pct: u8,
    /// Where the PC draws its power from.
    pub power: Power,
    /// An app that opens, grows, holds its size and closes during the run.
    #[serde(default)]
    pub heavy_app: Option<HeavyApp>,
}

/// Where the simulated PC draws its power from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    /// Mains power.
    Ac,
    /// The battery.
    Battery,
    /// The battery, with Windows' battery saver on.
    BatterySaver,
}

/// An app that opens, grows to its peak, holds it and closes: a video editor rendering, a
/// virtual machine starting. Times are seconds of virtual time from the start of the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeavyApp {
    /// A made-up name (`video-editor`), never a real product's.
    pub name: String,
    /// When it opens.
    pub opens_at_s: u32,
    /// How fast its working set grows, MB per second, until it reaches its peak.
    pub grows_mb_s: u32,
    /// Its working set at its peak, MB; it commits as much as it holds.
    pub peak_mb: u32,
    /// When it closes and lets all of it go.
    pub closes_at_s: u32,
    /// The CPU busy percent it adds while it is open.
    pub cpu_pct: u8,
}

/// What Eileen's own processes hold, MB (section 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EileenCosts {
    /// The core process.
    pub core_mb: u32,
    /// The speech process, from its start until it ends.
    pub speech_mb: u32,
    /// The brain session.
    pub brain_mb: u32,
    /// Each worker session, from its start until it exits.
    pub worker_mb: u32,
}

/// How the fake brain backend answers a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainProfile {
    /// The shortest time to the first text, ms.
    pub first_ms: u32,
    /// How much longer than `first_ms` the first text may take, ms, drawn per turn.
    pub first_jitter_ms: u32,
    /// The time between two deltas, ms.
    pub delta_ms: u32,
    /// The chance, in percent, that a reply nobody scripted dispatches a job.
    pub dispatch_pct: u8,
    /// How long a stalled turn sends nothing before its whole answer comes, ms.
    pub stall_ms: u32,
}

/// How often the world's fault points fire (`eileen_stdx::buggify`): each site is switched on
/// for a whole run with the first chance, decided from the seed and the site's name, and an
/// active site fires each time it is reached with the second. Both are per mille; a 0 leaves
/// every site off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaultProfile {
    /// The chance a site is active in a run, per mille.
    pub activate_per_mille: u32,
    /// The chance an active site fires when reached, per mille.
    pub fire_per_mille: u32,
}

impl FaultProfile {
    /// Whether any fault can fire.
    #[must_use]
    pub const fn on(&self) -> bool {
        self.activate_per_mille > 0 && self.fire_per_mille > 0
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

    /// Parses a profile's TOML and checks its rules: the name matches, a run has steps, at
    /// least one input kind has a weight, every chance is one, and the PC can exist.
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
        if profile.barge_in_pct > 100 || profile.brain.dispatch_pct > 100 {
            return Err(invalid("a percent is at most 100".to_owned()));
        }
        if profile.faults.activate_per_mille > 1_000 || profile.faults.fire_per_mille > 1_000 {
            return Err(invalid("a per mille chance is at most 1000".to_owned()));
        }
        profile.pc.check().map_err(|reason| invalid(reason.to_owned()))?;
        Ok(profile)
    }
}

impl PcProfile {
    /// The rules a PC keeps: the other apps fit in it, the biggest is one of them, a percent
    /// is at most 100, and a heavy app has a name, a peak and a growth, and closes after it
    /// opens.
    fn check(&self) -> Result<(), &'static str> {
        if self.physical_mb == 0 || self.commit_limit_mb < self.physical_mb {
            return Err("the commit limit is at least the physical memory, which is not 0");
        }
        if self.others_mb >= self.physical_mb || self.others_commit_mb >= self.commit_limit_mb {
            return Err("the other apps leave some memory and some commit");
        }
        if self.largest.is_empty() || self.largest_mb > self.others_mb {
            return Err("the biggest other app has a name and is one of the other apps");
        }
        if self.cpu_busy_pct > 100 {
            return Err("a percent is at most 100");
        }
        if let Some(app) = &self.heavy_app {
            if app.name.is_empty() || app.peak_mb == 0 || app.grows_mb_s == 0 {
                return Err("the heavy app has a name, a peak and a growth");
            }
            if app.closes_at_s <= app.opens_at_s || app.cpu_pct > 100 {
                return Err("the heavy app closes after it opens and adds at most 100 percent");
            }
        }
        Ok(())
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
    fn the_profiles_of_section_4_7_are_known() {
        assert_eq!(
            names(),
            [
                "default",
                "barge-in-heavy",
                "many-jobs",
                "flaky-sidecars",
                "media-playing",
                "low-memory"
            ],
            "heavy-app and roomy follow with the machine's facts"
        );
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
        let silent = silent.replace("activity = ", "activity = 0 #");
        let error = Profile::parse("default", &silent).unwrap_err();
        assert!(error.to_string().contains("weight"), "{error}");
        let unknown = format!("flavour = 1\n{text}");
        assert!(Profile::parse("default", &unknown).is_err(), "unknown keys are refused");
    }

    #[test]
    fn a_pc_or_a_chance_that_cannot_be_is_refused() {
        let text = include_str!("../profiles/low-memory.toml");
        assert!(Profile::parse("low-memory", text).is_ok(), "the profile as written loads");
        for (from, to, why) in [
            ("others_mb = ", "others_mb = 999999 #", "more than the PC holds"),
            ("largest_mb = ", "largest_mb = 99999 #", "a biggest app bigger than all of them"),
            ("closes_at_s = ", "closes_at_s = 0 #", "a heavy app that closes before it opens"),
            ("fire_per_mille = ", "fire_per_mille = 1001 #", "a chance over 1000 per mille"),
            ("barge_in_pct = ", "barge_in_pct = 101 #", "a percent over 100"),
            ("dispatch_pct = ", "dispatch_pct = 101 #", "a dispatch percent over 100"),
        ] {
            assert!(text.contains(from), "the profile sets `{from}`");
            assert!(Profile::parse("low-memory", &text.replace(from, to)).is_err(), "{why}");
        }
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

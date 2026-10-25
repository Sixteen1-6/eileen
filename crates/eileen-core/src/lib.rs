//! Eileen's decisions (BUILD.md sections 2.2 to 2.12 and 3.3.3): one state machine with one entry
//! point, [`Core::step`], that turns every [`Input`] into [`Effect`]s and performs no IO. A stub
//! until WP1 builds it: the minimal core of slice S1 first, then the full `Core::step`.
//!
//! # Owns
//!
//! - [`Core`]: turn-taking and barge-in, the addressee gate, intent dispatch, jobs and the warm
//!   pool, memory admission, the permission gate, approvals, announcements, the settings flow
//!   and the learning loop's decisions.
//! - [`CorePorts`] and [`CoreConfig`]: what the runtime hands the core at start.
//! - The `shell_command` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - Any IO, clock or thread: drivers turn the world into inputs and carry out effects
//!   (`eileen-runtime`). The resolver, cleaner, settings validator and skill engine arrive through
//!   the ports of `eileen-contracts::ports`, implemented by WP7's and WP4's crates.
//!
//! # Invariants
//!
//! - No IO: the crate depends only on `eileen-contracts`, `eileen-stdx` and `eileen-trace`
//!   (check 24), time comes in as [`MonoTime`] and randomness as [`CoreRng`].
//! - Maps that can reach an effect are ordered: `crates/eileen-core/clippy.toml` bans `HashMap`.
//! - The stub takes every input and emits nothing (unit test
//!   `tests::a_stub_core_takes_any_input_and_emits_nothing`).
//! - The sixteen simulator invariants of section 4.7 hold after every step (WP1, `eileen-sim`).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime` (one task owns the core), `eileen-replay` and `eileen-sim`.
//! - May depend on `eileen-contracts`, `eileen-stdx` and `eileen-trace`; must not depend on any
//!   other Eileen crate, a native library or anything that can hang.
//! - Wire types: none of its own; inputs, effects and the snapshot are contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-core` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-core`).
//! - Layers: unit tests with scripted ports, exhaustive turn-taking (`turn::exhaustive`), the
//!   simulator's seeds and regressions, and scenario replay.
//! - A bug here starts as a failing simulator seed or a scenario whose expectation broke.
//!
//! # Traps
//!
//! - The stub's [`Core::snapshot`] returns the snapshot it started from (or a fresh one), so a
//!   replay of the stub never diverges; WP1 builds it from the live state.
//!
//! # Logs and tracing
//!
//! - None yet. WP1's spans and events come from `eileen_trace::names`, and every line inside a
//!   turn carries its turn id (invariant 13).
//!
//! # Do not change without a decision record
//!
//! - The signatures of [`Core::new`], [`Core::step`] and [`Core::snapshot`] (section 3.3.3), and
//!   the snapshot's version.

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;

use std::collections::BTreeMap;
use std::fmt;

use eileen_contracts::config::Config;
use eileen_contracts::core_io::{Effect, Input};
use eileen_contracts::ids::{Envelope, MonoTime};
use eileen_contracts::jobs::JobKind;
use eileen_contracts::ports::{DictationCleaner, IntentResolver, SettingsValidator, SkillEngine};
use eileen_contracts::record::CoreSnapshotV1;
use eileen_contracts::speech::VoiceCalibration;
use eileen_stdx::rng::CoreRng;

/// The ports the core calls inside [`Core::step`] (section 3.3.15).
pub struct CorePorts {
    /// The intent resolver (`eileen_intent::Resolver`).
    pub resolver: Box<dyn IntentResolver>,
    /// The dictation cleaner (`eileen_dictation::Cleaner`).
    pub cleaner: Box<dyn DictationCleaner>,
    /// The settings validator (`eileen_config::Validator`).
    pub settings: Box<dyn SettingsValidator>,
    /// The skill engine (`eileen_skills::Engine`).
    pub skills: Box<dyn SkillEngine>,
}

impl fmt::Debug for CorePorts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CorePorts").finish_non_exhaustive()
    }
}

/// What the runtime read from disk at start. WP1 adds `machine` and `speaker_bar` (section
/// 3.3.17) once their contract types exist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreConfig {
    /// The configuration (section 3.3.11).
    pub config: Config,
    /// `voices\<user>\calibration.toml`, when the first run measured one.
    pub calibration: Option<VoiceCalibration>,
    /// The measured peak working set per job kind, in MB (section 2.5).
    pub cost_estimates: BTreeMap<JobKind, u32>,
}

/// The core: every decision Eileen makes, as one state machine.
pub struct Core {
    config: CoreConfig,
    ports: CorePorts,
    snapshot: CoreSnapshotV1,
}

// Written out rather than derived: until WP1's `step` reads them, this is what reads `config`
// and `ports`, and dead-code analysis does not count a derived `Debug`.
impl fmt::Debug for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Core")
            .field("config", &self.config)
            .field("ports", &self.ports)
            .field("snapshot", &self.snapshot)
            .finish()
    }
}

impl Core {
    /// A core with this configuration and these ports, resumed from `snapshot` when given.
    #[must_use]
    pub fn new(config: CoreConfig, ports: CorePorts, snapshot: Option<CoreSnapshotV1>) -> Self {
        Self { config, ports, snapshot: snapshot.unwrap_or_else(fresh_snapshot) }
    }

    /// Takes one input at `now` and appends the effects it causes. The stub takes every input
    /// and appends nothing.
    #[expect(clippy::unused_self, reason = "a stub until WP1: the signature is section 3.3.3's")]
    pub fn step(
        &mut self,
        _now: MonoTime,
        _rng: &mut CoreRng,
        _input: Envelope<Input>,
        _effects: &mut Vec<Envelope<Effect>>,
    ) {
    }

    /// The state to checkpoint and replay from (section 3.3.16).
    #[must_use]
    pub fn snapshot(&self) -> CoreSnapshotV1 {
        self.snapshot.clone()
    }
}

/// The snapshot of a core that has never run.
fn fresh_snapshot() -> CoreSnapshotV1 {
    CoreSnapshotV1 { v: 1, ..CoreSnapshotV1::default() }
}

#[cfg(test)]
mod tests;

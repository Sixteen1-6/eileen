//! The pure half of Eileen's learning loop (BUILD.md sections 2.10 and 3.3.13): the ladder, the
//! success verdict, shadow checks, route lines, template grammar, linting, provenance and the
//! replay state machine, as pure functions the core calls through the `SkillEngine` port. A stub
//! until WP7 builds it.
//!
//! # Owns
//!
//! - [`Engine`], the implementation of `eileen_contracts::ports::SkillEngine`.
//! - The pure functions of section 3.3.13; the stub has those whose types exist already
//!   ([`ladder::next_level`], [`verdict::success`], [`shadow::checks`], [`shadow::agree`],
//!   [`routes::facts_line`]), and WP7 adds the rest (`template`, `lint`, `provenance`, `slots`,
//!   `facts`, `sources`, `forget`, `ReplayMachine`).
//! - The `skill_record` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The learning store, the recorder, the librarian and every other IO (`eileen-learn`), or
//!   acting on a decision (`eileen-core`).
//!
//! # Invariants
//!
//! - Pure: no IO, no clock, no native library (check 24).
//! - A replayed skill never raises a tier, skips a confirmation or calls a tool its origin does
//!   not allow (invariant 7).
//! - The stub engine knows no skill: it refuses every replay and counts nothing as a success
//!   (unit tests in `tests`).
//!
//! # Boundaries
//!
//! - Called by `eileen-core` through the port, and by `eileen-intent`, `eileen-learn` and
//!   `eileen-sim`.
//! - May depend on `eileen-contracts` and `eileen-stdx`.
//! - Wire types: none of its own; skills and recipes are contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-skills` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-skills`).
//! - Layers: table tests for the ladder and the linter, property tests for replay, and the
//!   `skill_record` fuzz target.
//! - A bug here starts as a skill promoted on too little evidence, or a replay that ran on an
//!   ambiguous slot.
//!
//! # Traps
//!
//! - Level 3 needs the user's approval as well as the ladder's thresholds.
//!
//! # Logs and tracing
//!
//! - None: the crate is pure.
//!
//! # Do not change without a decision record
//!
//! - The ladder's thresholds and the linter's rules (section 2.10).

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;

use eileen_contracts::connectors::{AppCapability, ConnectorManifest};
use eileen_contracts::ids::SkillId;
use eileen_contracts::intent::SlotValues;
use eileen_contracts::learn::{
    CheckPlan, Outcome, Recipe, ReplayRefusal, ReplayState, ReplayStep, RouteStat, ShadowVerdict,
    SkillMeta, SkillRef, StepResult, TrajectoryStep, Transition, UseRecord,
};
use eileen_contracts::ports::SkillEngine;

/// The ladder: when a skill moves up or down a level.
pub mod ladder {
    use eileen_contracts::learn::{LadderPolicy, SkillMeta, Transition, UseRecord};

    /// The ladder's decision for a skill after its uses. The stub keeps every skill where it
    /// is.
    #[must_use]
    #[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's ladder")]
    pub fn next_level(
        _meta: &SkillMeta,
        _uses: &[UseRecord],
        _policy: &LadderPolicy,
    ) -> Transition {
        Transition::Stay
    }
}

/// The success verdict.
pub mod verdict {
    use eileen_contracts::learn::Outcome;

    /// Whether an outcome counts as a success. The stub counts none.
    #[must_use]
    #[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's verdict rules")]
    pub fn success(_outcome: &Outcome) -> bool {
        false
    }
}

/// Shadow runs: whether a job that could have replayed a recipe did what it would have.
pub mod shadow {
    use eileen_contracts::intent::SlotValues;
    use eileen_contracts::learn::{CheckPlan, Recipe, ShadowVerdict, TrajectoryStep};

    /// The check ops the core issues after the newest step matched a recipe step whose
    /// postcondition names one. The stub issues none.
    #[must_use]
    #[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's shadow rules")]
    pub fn checks(
        _recipe: &Recipe,
        _slots: &SlotValues,
        _steps: &[TrajectoryStep],
    ) -> Vec<CheckPlan> {
        Vec::new()
    }

    /// Whether a shadowed job agreed with the recipe. The stub cannot tell.
    #[must_use]
    pub fn agree(
        _recipe: &Recipe,
        _slots: &SlotValues,
        _steps: &[TrajectoryStep],
    ) -> ShadowVerdict {
        ShadowVerdict::Inconclusive {
            step: String::new(),
            reason: "the skill engine is not built yet".to_owned(),
        }
    }
}

/// Route statistics turned into what workers read.
pub mod routes {
    use eileen_contracts::connectors::AppCapability;
    use eileen_contracts::learn::RouteStat;

    /// The one-line route summary for an app that goes into a worker's facts block, for
    /// example "Resolve: scripting worked 14 of 14 times for navigation; use `apps_resolve`
    /// before the screen". The stub has none.
    #[must_use]
    #[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's route lines")]
    pub fn facts_line(_stats: &[RouteStat], _caps: &AppCapability) -> Option<String> {
        None
    }
}

/// The skill engine the core drives replay and the ladder through. The stub knows no skill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Engine;

impl SkillEngine for Engine {
    fn rebuild(
        &mut self,
        _skills: &[SkillMeta],
        _recipes: &[Recipe],
        _manifests: &[ConnectorManifest],
        _routes: &[RouteStat],
    ) {
    }

    fn route_line(&self, _app: &str, caps: &AppCapability) -> Option<String> {
        routes::facts_line(&[], caps)
    }

    fn next_level(&self, _skill: &SkillId, _uses: &[UseRecord]) -> Transition {
        Transition::Stay
    }

    fn success(&self, outcome: &Outcome) -> bool {
        verdict::success(outcome)
    }

    fn start_replay(
        &self,
        _skill: &SkillRef,
        _slots: &SlotValues,
    ) -> Result<ReplayState, ReplayRefusal> {
        Err(ReplayRefusal::UnknownSkill)
    }

    fn next(&self, _state: &mut ReplayState, _last: Option<&StepResult>) -> ReplayStep {
        ReplayStep::Done
    }

    fn shadow_checks(
        &self,
        _skill: &SkillRef,
        _slots: &SlotValues,
        _steps: &[TrajectoryStep],
    ) -> Vec<CheckPlan> {
        Vec::new()
    }

    fn shadow_agree(
        &self,
        _skill: &SkillRef,
        _slots: &SlotValues,
        _steps: &[TrajectoryStep],
    ) -> ShadowVerdict {
        ShadowVerdict::Inconclusive {
            step: String::new(),
            reason: "the skill engine is not built yet".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::connectors::AppCapability;
    use eileen_contracts::ids::SkillId;
    use eileen_contracts::intent::SlotValues;
    use eileen_contracts::learn::{ReplayRefusal, SkillRef, Transition};
    use eileen_contracts::ports::SkillEngine;

    use super::Engine;

    fn skill() -> SkillRef {
        SkillRef { id: SkillId::new("resolve-sharpen-face").unwrap(), version: 1 }
    }

    #[test]
    fn the_stub_engine_refuses_every_replay() {
        let refused = Engine.start_replay(&skill(), &SlotValues::new());
        assert_eq!(refused.err(), Some(ReplayRefusal::UnknownSkill), "it knows no skill");
        assert_eq!(Engine.next_level(&skill().id, &[]), Transition::Stay, "no level changes");
    }

    #[test]
    fn the_stub_engine_has_no_route_line() {
        let resolve = AppCapability {
            app: "resolve".to_owned(),
            verbs: vec!["goto".to_owned()],
            version: None,
            rungs: Vec::new(),
        };
        assert_eq!(Engine.route_line("resolve", &resolve), None, "no statistics yet");
    }
}

//! Replay (BUILD.md section 3.3.16): strict mode for incidents, recordings and `--doublecheck`,
//! scenario mode for saved and shared scenarios, the expectation and expected-trace matcher, and
//! [`responder`], the component that answers brain and worker commands from a recording. A stub
//! until WP1 builds it.
//!
//! # Owns
//!
//! - Strict replay with the first-divergence report, and scenario replay with its scheduled,
//!   reactive and synthesized input rules, stable keys and ID remapping.
//! - The expectation and expected-trace matcher, and [`responder`] for `--replay-models`.
//! - `eileen replay` and `eileen scenario replay` ([`cli`]) and the `expected_trace` fuzz
//!   target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The core it drives (`eileen-core`), the recording files (`eileen-record`), saving, sharing
//!   and blessing scenarios (`eileen-scenarios`), or the simulator (`eileen-sim`).
//!
//! # Invariants
//!
//! - A strict replay stops at the first difference and names its sequence number and span path
//!   (WP1 acceptance 9).
//! - A changed hook decision fails a scenario replay; it is never passed silently (WP1
//!   acceptance 13).
//! - The stub [`responder`] accepts the brain and worker families and does nothing (unit test
//!   `tests::the_stub_responder_takes_the_harness_families`).
//!
//! # Boundaries
//!
//! - Called by `eileen-runtime` (`dev_replay_core`), `eileen-scenarios`, `eileen-fleet`,
//!   `eileen-sim` and `eileen-cli`.
//! - May depend on `eileen-core`, `eileen-record`, `eileen-contracts`, `eileen-stdx` and
//!   `eileen-trace`.
//! - Must not depend on `eileen-runtime`, `eileen-harness` (its [`responder`] stands in for it)
//!   or rmcp, nor know vendor CLI flags (section 3.1); nor on a native library or anything that
//!   can hang (check 24), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: none of its own.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-replay`.
//! - Layers: unit tests for the matcher and the stable keys, scenario replays, and the
//!   `expected_trace` fuzz target.
//! - A bug here starts as a scenario that passes after the behaviour it checks broke.
//!
//! # Traps
//!
//! - Reactive keys never digest a composed brain message, so rewording a prompt keeps a saved
//!   scenario passing (WP1 acceptance 13).
//!
//! # Logs and tracing
//!
//! - None yet; WP1 names its spans in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The expected-trace grammar and the stable-key rules of section 3.3.16.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

use std::path::Path;

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::ports::{Component, IdleComponent};

/// The component `eileen run --replay-models <dir>` swaps in for the harness: it answers brain
/// and worker commands from the recording in `scenario`. The stub accepts them and does nothing.
#[must_use]
pub fn responder(_scenario: &Path) -> Box<dyn Component> {
    Box::new(IdleComponent::new("responder", &[EffectFamily::Brain, EffectFamily::Worker]))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use eileen_contracts::core_io::EffectFamily;

    use super::responder;

    #[test]
    fn the_stub_responder_takes_the_harness_families() {
        let component = responder(Path::new("scenarios/core/example"));
        assert_eq!(component.name(), "responder", "its name");
        assert_eq!(
            component.accepts(),
            [EffectFamily::Brain, EffectFamily::Worker],
            "the families the harness takes"
        );
    }
}

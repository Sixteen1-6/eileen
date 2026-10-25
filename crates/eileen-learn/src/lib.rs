//! The IO half of Eileen's learning loop (BUILD.md sections 2.10 and 3.3.13): the trajectory
//! recorder, the `learn.sqlite` store, the snapshot writer and verifier, the librarian scheduler
//! and the `learn` toolset handlers, the user-model, phrasing, route-proposal, route-statistics
//! and ledger stores, the notes writer, and the forget executor. A stub until WP7 builds it.
//!
//! # Owns
//!
//! - The learning [`component`], which carries out `Effect::Learn` and reports `LearnEvent`s.
//! - The stores and the snapshot writer (`eileen-learn::store` is the only module here that may
//!   use vendored C, on the runtime's blocking pool).
//! - `eileen skills`, `eileen facts`, `eileen apps proposals` and `eileen notes` ([`cli`]).
//!
//! # Does not own
//!
//! - The pure rules (`eileen-skills`), what to learn and when (`eileen-core`), or the recording
//!   formats (`eileen-record`).
//!
//! # Invariants
//!
//! - Forgetting completes only when the store and the recordings both report it (the canary
//!   test of WP7 acceptance 6).
//! - A skill reaches a session only through a verified snapshot, re-hashed before spawn (WP7
//!   acceptance 10 tests a tampered one).
//! - The stub accepts the learn family and does nothing (unit test
//!   `tests::the_stub_component_takes_the_learn_family`).
//!
//! # Boundaries
//!
//! - Called by `eileen-cli` (the component and the commands).
//! - May depend on `eileen-skills`, `eileen-record`, `eileen-rt`, `eileen-contracts`,
//!   `eileen-stdx` and `eileen-trace`.
//! - Must not depend on `eileen-core`, `eileen-runtime`, `eileen-harness` or rmcp, nor know
//!   vendor CLI flags (section 3.1); nor on vendored C outside `eileen-learn::store` (check 24),
//!   or on a `-win` or `-linux` crate or a native library of section 4.1's table (check 15).
//! - Wire types: skill files and snapshots in the contract formats.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-learn` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-learn`).
//! - Layers: store tests on temporary folders, librarian runs against stub scripts in
//!   `tests/scripts/`, and the learning scenarios.
//! - A bug here starts as a forgotten fact that came back, or a snapshot that does not match
//!   its manifest.
//!
//! # Traps
//!
//! - The librarian is a Claude session; it runs only within `learning.librarian_max_plan_pct`.
//!
//! # Logs and tracing
//!
//! - None yet. WP7's spans are `learn.record` and `librarian.run` from `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - The snapshot manifest's hashing and what forgetting removes.

#![forbid(unsafe_code)]

pub mod cli;

use eileen_contracts::core_io::EffectFamily;
use eileen_contracts::ports::{Component, IdleComponent};

/// What the learning component reads at start. WP7 adds its fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LearnConfig;

/// The learning component. The stub accepts the learn family and does nothing.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "section 3.3.15's signature; LearnConfig has no fields until WP7"
)]
pub fn component(_cfg: &LearnConfig) -> Box<dyn Component> {
    Box::new(IdleComponent::new("learn", &[EffectFamily::Learn]))
}

#[cfg(test)]
mod tests {
    use eileen_contracts::core_io::EffectFamily;

    use super::{LearnConfig, component};

    #[test]
    fn the_stub_component_takes_the_learn_family() {
        let learn = component(&LearnConfig);
        assert_eq!(learn.name(), "learn", "its name");
        assert_eq!(learn.accepts(), [EffectFamily::Learn], "its family");
    }
}

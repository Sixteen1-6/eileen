//! Eileen's intent resolver (BUILD.md sections 2.4 and 3.3.10): L1 (the command grammar, then
//! learned templates), L2 (lexical and semantic matching for hints and confirmed phrasings) and
//! L3 (the brain), the follow-up feature against Eileen's last turn, and [`introspect`]. Pure and
//! deterministic; the core calls it through the `IntentResolver` port. A stub until WP7 builds
//! it.
//!
//! # Owns
//!
//! - [`Resolver`], the implementation of `eileen_contracts::ports::IntentResolver`, and the
//!   [`Grammar`] with its golden file.
//! - [`introspect`]: what Eileen can do here, each app's routes ranked.
//! - The `command_grammar` fuzz target's entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The template grammar (`eileen-skills::template`), the embedding model (`eileen-speech`),
//!   or acting on a resolution (`eileen-core`).
//!
//! # Invariants
//!
//! - Precedence: safety and control, then the built-in grammar, then learned templates.
//! - A slot resolver returns `Ambiguous` for bare numbers and times without units, and replay
//!   never runs on an ambiguous slot (WP7's golden-file tests).
//! - Pure: no IO, no clock, no native library (check 24).
//! - The stub sends every utterance to the brain (unit test
//!   `tests::the_stub_sends_every_utterance_to_the_brain`).
//!
//! # Boundaries
//!
//! - Called by `eileen-core` through the port, and by `eileen-sim`.
//! - May depend on `eileen-skills`, `eileen-contracts` and `eileen-stdx`.
//! - Wire types: none of its own; resolutions are contract types.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-intent` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-intent`).
//! - Layers: the resolver golden file, latency tests with loose bounds in CI, and the
//!   `command_grammar` fuzz target.
//! - A bug here starts as a phrase that reached the brain when the grammar has it, or the
//!   reverse.
//!
//! # Traps
//!
//! - A confirmed L2 phrasing runs directly only when its target has no slots and every entity
//!   name matches; otherwise it is a hint.
//!
//! # Logs and tracing
//!
//! - None: the crate is pure.
//!
//! # Do not change without a decision record
//!
//! - The grammar's precedence and the port's signature (section 3.3.15).

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;

use eileen_contracts::connectors::AppCapability;
use eileen_contracts::intent::{Capabilities, FollowUp, Resolution, ResolveCtx};
use eileen_contracts::learn::{PhrasingFact, RouteStat, SkillMeta};
use eileen_contracts::ports::IntentResolver;
use eileen_contracts::tools::ToolSpec;

/// The command grammar of section 2.4: its phrases, their precedence and what each resolves
/// to. WP7 writes it with its golden file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grammar;

/// The resolver: the grammar table, skill templates, the BM25 index and confirmed phrasing
/// pairs. The stub holds none of them and sends every utterance to the brain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resolver;

impl Resolver {
    /// A resolver for `grammar`.
    #[must_use]
    pub const fn new(_grammar: Grammar) -> Self {
        Self
    }

    /// Rebuilds the indexes from the current skills and phrasings. The stub keeps none.
    #[expect(clippy::unused_self, reason = "section 3.3.10's signature; the stub keeps no index")]
    pub const fn rebuild(&mut self, _skills: &[SkillMeta], _phrasings: &[PhrasingFact]) {}

    /// L1 and the lexical part of L2. The stub sends every utterance to the brain.
    #[must_use]
    #[expect(
        clippy::unused_self,
        clippy::trivially_copy_pass_by_ref,
        reason = "section 3.3.10's signature; the stub matches nothing"
    )]
    pub const fn resolve(&self, _utterance: &str, _ctx: &ResolveCtx) -> Resolution {
        Resolution::Brain { depth: None, addressee_unsure: false }
    }

    /// L2 with an embedding the core requested from the speech process. The stub ignores the
    /// embedding.
    #[must_use]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "section 3.3.10's signature; the stub resolver holds nothing"
    )]
    pub const fn resolve_semantic(
        &self,
        utterance: &str,
        _embedding: &[f32],
        ctx: &ResolveCtx,
    ) -> Resolution {
        self.resolve(utterance, ctx)
    }

    /// The conversation window's follow-up feature (section 2.8 rule 3). The stub finds none.
    #[must_use]
    #[expect(
        clippy::unused_self,
        clippy::trivially_copy_pass_by_ref,
        reason = "section 3.3.10's signature; the stub finds no follow-up"
    )]
    pub const fn follow_up(&self, _utterance: &str, _ctx: &ResolveCtx) -> FollowUp {
        FollowUp::None
    }
}

impl IntentResolver for Resolver {
    fn rebuild(&mut self, skills: &[SkillMeta], phrasings: &[PhrasingFact]) {
        Self::rebuild(self, skills, phrasings);
    }

    fn resolve(&self, utterance: &str, ctx: &ResolveCtx) -> Resolution {
        Self::resolve(self, utterance, ctx)
    }

    fn resolve_semantic(&self, utterance: &str, embedding: &[f32], ctx: &ResolveCtx) -> Resolution {
        Self::resolve_semantic(self, utterance, embedding, ctx)
    }

    fn follow_up(&self, utterance: &str, ctx: &ResolveCtx) -> FollowUp {
        Self::follow_up(self, utterance, ctx)
    }

    fn introspect(
        &self,
        tools: &[ToolSpec],
        app_caps: &[AppCapability],
        routes: &[RouteStat],
        focused: Option<&str>,
    ) -> Capabilities {
        introspect(tools, app_caps, &[], routes, focused)
    }
}

/// What Eileen can do here: each app's routes ranked by their declared rungs and the recorded
/// route statistics (`eileen_skills::routes::rank`), and the learned skills that apply. The stub
/// names the focused app and ranks nothing.
#[must_use]
pub fn introspect(
    _tools: &[ToolSpec],
    _app_caps: &[AppCapability],
    _skills: &[SkillMeta],
    _routes: &[RouteStat],
    focused: Option<&str>,
) -> Capabilities {
    Capabilities { focused: focused.map(str::to_owned), apps: Vec::new(), skills: Vec::new() }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::intent::{FollowUp, Resolution, ResolveCtx};
    use eileen_contracts::ports::IntentResolver;

    use super::{Grammar, Resolver};

    fn ctx() -> ResolveCtx {
        ResolveCtx {
            focused_app: Some("notepad".to_owned()),
            media_active: false,
            dictation: false,
            awaiting: None,
            assistant_name: "Eileen".to_owned(),
            monitors: 1,
            media_history: Vec::new(),
            last_turn: None,
        }
    }

    #[test]
    fn the_stub_sends_every_utterance_to_the_brain() {
        let resolver = Resolver::new(Grammar);
        let brain = Resolution::Brain { depth: None, addressee_unsure: false };
        for utterance in ["mute", "what time is it", "open the file I was working on"] {
            assert_eq!(resolver.resolve(utterance, &ctx()), brain, "{utterance}");
            assert_eq!(resolver.resolve_semantic(utterance, &[0.5], &ctx()), brain, "{utterance}");
            assert_eq!(resolver.follow_up(utterance, &ctx()), FollowUp::None, "{utterance}");
        }
    }

    #[test]
    fn the_stub_introspection_names_the_focused_app() {
        let capabilities = Resolver::new(Grammar).introspect(&[], &[], &[], Some("notepad"));
        assert_eq!(capabilities.focused.as_deref(), Some("notepad"), "the focused app");
        assert!(capabilities.apps.is_empty(), "nothing ranked yet");
    }
}

//! [`SimResolver`]: the scripted intent resolver the simulated core uses in place of the stub,
//! beside [`crate::world::SimSettings`]. It knows the phrases the minimal core answers itself,
//! resolves everything else to the brain, and finds a follow-up only for the phrases it is told
//! about, and only when the core passed Eileen's last turn.

use eileen_contracts::connectors::AppCapability;
use eileen_contracts::intent::{
    Capabilities, CoreCommand, FollowUp, Resolution, ResolveCtx, SlotValues,
};
use eileen_contracts::learn::{PhrasingFact, RouteStat, SkillMeta};
use eileen_contracts::ports::IntentResolver;
use eileen_contracts::tools::ToolSpec;

/// A scripted resolver: a table of phrases and core commands, and a list of follow-ups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimResolver {
    commands: Vec<(String, CoreCommand)>,
    follow_ups: Vec<String>,
}

impl Default for SimResolver {
    /// The phrases of the minimal core: what time, stop, go on, mute, unmute and cancel.
    fn default() -> Self {
        let commands = [
            ("what time is it", CoreCommand::WhatTime),
            ("what's the time", CoreCommand::WhatTime),
            ("stop", CoreCommand::Stop),
            ("go on", CoreCommand::GoOn),
            ("continue", CoreCommand::GoOn),
            ("mute", CoreCommand::Mute),
            ("unmute", CoreCommand::Unmute),
            ("cancel", CoreCommand::Cancel { job: None }),
            ("cancel that", CoreCommand::Cancel { job: None }),
        ];
        Self {
            commands: commands.into_iter().map(|(p, c)| (p.to_owned(), c)).collect(),
            follow_ups: Vec::new(),
        }
    }
}

impl SimResolver {
    /// The default table, plus utterances that resolve as a follow-up to Eileen's last turn.
    #[must_use]
    pub fn with_follow_ups(follow_ups: &[&str]) -> Self {
        Self { follow_ups: follow_ups.iter().map(|f| normalise(f)).collect(), ..Self::default() }
    }
}

/// Lower case, with punctuation off the ends of the words, as the core passes utterances.
fn normalise(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|word| !word.is_empty())
        .collect();
    words.join(" ")
}

impl IntentResolver for SimResolver {
    fn rebuild(&mut self, _: &[SkillMeta], _: &[PhrasingFact]) {}

    fn resolve(&self, utterance: &str, _: &ResolveCtx) -> Resolution {
        let utterance = normalise(utterance);
        let known = self.commands.iter().find(|(phrase, _)| *phrase == utterance);
        known.map_or(Resolution::Brain { depth: None, addressee_unsure: false }, |(_, command)| {
            Resolution::Command { command: command.clone(), slots: SlotValues::new() }
        })
    }

    fn resolve_semantic(&self, utterance: &str, _: &[f32], ctx: &ResolveCtx) -> Resolution {
        self.resolve(utterance, ctx)
    }

    fn follow_up(&self, utterance: &str, ctx: &ResolveCtx) -> FollowUp {
        let utterance = normalise(utterance);
        if ctx.last_turn.is_some() && self.follow_ups.contains(&utterance) {
            FollowUp::Referent { slot: "job".to_owned(), value: serde_json::Value::Null }
        } else {
            FollowUp::None
        }
    }

    fn introspect(
        &self,
        _: &[ToolSpec],
        _: &[AppCapability],
        _: &[RouteStat],
        _: Option<&str>,
    ) -> Capabilities {
        Capabilities { focused: None, apps: Vec::new(), skills: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use eileen_contracts::intent::{CoreCommand, FollowUp, LastTurnRefs, Resolution, ResolveCtx};
    use eileen_contracts::ports::IntentResolver;

    use super::SimResolver;

    fn ctx(last_turn: bool) -> ResolveCtx {
        ResolveCtx {
            focused_app: None,
            media_active: false,
            dictation: false,
            awaiting: None,
            assistant_name: "Eileen".to_owned(),
            monitors: 1,
            media_history: Vec::new(),
            last_turn: last_turn.then(|| LastTurnRefs {
                age_ms: 500,
                items: Vec::new(),
                job: None,
                apps: Vec::new(),
                files: Vec::new(),
                media: Vec::new(),
                content_words: vec!["render".to_owned()],
                topic: None,
            }),
        }
    }

    #[test]
    fn the_table_answers_its_phrases_and_sends_the_rest_to_the_brain() {
        let resolver = SimResolver::default();
        assert!(
            matches!(
                resolver.resolve("What time is it?", &ctx(false)),
                Resolution::Command { command: CoreCommand::WhatTime, .. }
            ),
            "a known phrase, any case and punctuation"
        );
        assert!(
            matches!(resolver.resolve("tell me a joke", &ctx(false)), Resolution::Brain { .. }),
            "the rest goes to the brain"
        );
    }

    #[test]
    fn a_follow_up_needs_the_last_turn() {
        let resolver = SimResolver::with_follow_ups(&["and how long will it take?"]);
        assert_eq!(
            resolver.follow_up("and how long will it take", &ctx(false)),
            FollowUp::None,
            "no last turn, no follow-up"
        );
        assert!(
            matches!(
                resolver.follow_up("and how long will it take", &ctx(true)),
                FollowUp::Referent { .. }
            ),
            "it resolves against the last turn"
        );
        assert_eq!(resolver.follow_up("want coffee", &ctx(true)), FollowUp::None, "room talk");
    }
}

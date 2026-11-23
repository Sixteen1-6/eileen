//! The router through `Core::step` (BUILD.md sections 2.3 and 2.5): spoken overrides, the
//! kind's policy, `routing.deep` for deep answers and the depth's model, with the reason each
//! job is accepted for and the words recorded only for what the user said.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::config::KindRouting;
use eileen_contracts::jobs::{JobKind, Route};
use serde_json::{Value, json};

use super::jobs::{BRAIN, addressed, dispatch_from};
use super::steering::{job_args, row};
use super::{Harness, ask_brain, done, events};
use crate::CoreConfig;

/// What the router decided for a dispatch of `args` in a turn where the user said `said`: the
/// model, the reason, and the words recorded as the user's override.
fn route(h: &mut Harness, said: &str, args: Value) -> (String, String, Option<String>) {
    let turn = ask_brain(h, said);
    let effects = h.step(dispatch_from(BRAIN, addressed(turn), args));
    h.step(done(turn));
    let accepted = events(&effects).into_iter().find_map(|e| {
        if let BusEvent::JobAccepted { job, model, reason, .. } = e {
            Some((*job, model.clone(), reason.clone()))
        } else {
            None
        }
    });
    let (job, model, reason) = accepted.expect("accepted");
    (model, reason, row(h, job).override_words.map(|w| w.get().clone()))
}

/// A deep answer the brain dispatches at deep depth, as it does for every one (section 2.3).
fn deep_answer() -> Value {
    let mut args = job_args("deep_answer", "the sky question");
    args["depth"] = json!("deep");
    args
}

#[test]
fn the_router_takes_an_override_then_the_policy_then_the_depths_model() {
    let mut config = CoreConfig::default();
    let research = KindRouting { model: Some("strong".to_owned()), ..KindRouting::default() };
    config.config.routing.kinds.insert(JobKind::Research, research);
    let codex = KindRouting {
        route: Some(Route::Codex),
        model: Some("gpt-5-codex".to_owned()),
        ..KindRouting::default()
    };
    config.config.routing.kinds.insert(JobKind::Coding, codex);
    let mut h = Harness::with(config, Vec::new());
    let said = "Eileen, use the fast model for this";
    let policy = route(&mut h, said, job_args("research", "the price check"));
    let wanted = ("strong".to_owned(), "the routing policy for research".to_owned(), None);
    assert_eq!(policy, wanted, "the policy for the kind, with no override words");
    let mut asked = job_args("research", "the price check");
    asked["model"] = json!("fast");
    let (model, reason, words) = route(&mut h, said, asked);
    assert_eq!((model.as_str(), reason.as_str()), ("fast", "asked for the model fast"), "override");
    assert_eq!(words.as_deref(), Some(said), "with the words");
    let mut quick = job_args("general", "the report");
    quick["depth"] = json!("quick");
    assert_eq!(route(&mut h, said, quick).0, "fast", "quick uses the fast model");
    let default = route(&mut h, said, job_args("general", "the report"));
    assert_eq!(default.0, "mid", "considered uses the mid model");
    assert_eq!(default.1, "the default for considered depth", "and says so");
    let unbuilt = route(&mut h, said, job_args("coding", "the parser fix"));
    let wanted = (
        "mid".to_owned(),
        "the routing policy names Codex, which this version does not run".to_owned(),
        None,
    );
    assert_eq!(unbuilt, wanted, "Claude with the depth's model, never Codex's model");
    let mut claude = job_args("general", "the report");
    claude["route"] = json!("claude");
    let (model, reason, words) = route(&mut h, "Eileen, give this to Claude", claude);
    assert_eq!((model.as_str(), reason.as_str()), ("mid", "asked for Claude"), "a route alone");
    assert_eq!(words.as_deref(), Some("Eileen, give this to Claude"), "with the words");
}

#[test]
fn a_depth_the_brain_chose_is_no_override_and_the_kinds_policy_wins() {
    let mut config = CoreConfig::default();
    let mid = KindRouting { model: Some("mid".to_owned()), ..KindRouting::default() };
    config.config.routing.kinds.insert(JobKind::DeepAnswer, mid);
    let mut h = Harness::with(config, Vec::new());
    let routed = route(&mut h, "Eileen, why is the sky blue?", deep_answer());
    let wanted = ("mid".to_owned(), "the routing policy for deep_answer".to_owned(), None);
    assert_eq!(routed, wanted, "the policy's model, and no words the user never said");
}

#[test]
fn a_depth_the_user_said_is_an_override_with_the_words() {
    let mut config = CoreConfig::default();
    let mid = KindRouting { model: Some("mid".to_owned()), ..KindRouting::default() };
    config.config.routing.kinds.insert(JobKind::DeepAnswer, mid);
    let mut h = Harness::with(config, Vec::new());
    let said = "Eileen, think hard about why the sky is blue";
    let (model, reason, words) = route(&mut h, said, deep_answer());
    assert_eq!((model.as_str(), reason.as_str()), ("strong", "asked for deep depth"), "override");
    assert_eq!(words.as_deref(), Some(said), "recorded with the words that set the depth");
}

#[test]
fn a_deep_answer_at_deep_depth_takes_routing_deep() {
    let mut h = Harness::new();
    let said = "Eileen, why is the sky blue?";
    let routed = route(&mut h, said, deep_answer());
    let wanted = ("strong".to_owned(), "routing.deep for deep answers".to_owned(), None);
    assert_eq!(routed, wanted, "the deep workers' model by default");
    let mut config = CoreConfig::default();
    config.config.routing.deep.model = "mid".to_owned();
    let mut h = Harness::with(config, Vec::new());
    let routed = route(&mut h, said, deep_answer());
    assert_eq!(routed.0, "mid", "a change to routing.deep changes the deep answer's model");
    let mut considered = job_args("deep_answer", "the sky question");
    considered["depth"] = json!("considered");
    assert_eq!(route(&mut h, said, considered).0, "mid", "a considered one keeps the mid model");
    let mut coding = job_args("coding", "the parser fix");
    coding["depth"] = json!("deep");
    assert_eq!(route(&mut h, said, coding).0, "strong", "other kinds keep the strongest model");
}

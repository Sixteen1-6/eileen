//! The router (BUILD.md section 2.5, "Routing"): each job's route, model and depth, and why,
//! for `JobAccepted` and the receipt.

use eileen_contracts::ids::Personal;
use eileen_contracts::jobs::{Depth, JobKind, Route};

use crate::Core;

/// Why a call naming a route v0.1 does not build was refused.
const ROUTE_NOT_BUILT: &str = "Only Claude can take jobs in this version.";

/// What the router decided for a job (section 2.5, "Routing").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Routed {
    pub(crate) route: Route,
    pub(crate) model: String,
    pub(crate) depth: Depth,
    /// Why, for `JobAccepted` and the receipt.
    pub(crate) reason: String,
    /// The user's words, when an override the brain passed on beat the policy.
    pub(crate) words: Option<Personal<String>>,
}

/// The model name a depth uses when nothing else names one (section 2.5: quick uses the fast
/// model, considered the mid model, deep the strongest).
const fn depth_model(depth: Depth) -> &'static str {
    match depth {
        Depth::Instant | Depth::Quick => "fast",
        Depth::Considered => "mid",
        Depth::Deep => "strong",
    }
}

/// A kind's wire name, such as `computer_use`.
fn kind_name(kind: JobKind) -> String {
    let named = serde_json::to_value(kind).ok();
    named.and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// A depth's wire name.
const fn depth_name(depth: Depth) -> &'static str {
    match depth {
        Depth::Instant => "instant",
        Depth::Quick => "quick",
        Depth::Considered => "considered",
        Depth::Deep => "deep",
    }
}

impl Core {
    /// The router (section 2.5): an override the brain passed on (route, model or depth) beats
    /// the policy and is recorded with the user's words; then the policy for the kind; then
    /// the depth's model. A route v0.1 does not build is refused when asked for, and a policy
    /// naming one falls back to Claude.
    pub(crate) fn route_for(
        &self,
        kind: JobKind,
        route: Option<Route>,
        model: Option<String>,
        depth: Option<Depth>,
    ) -> Result<Routed, &'static str> {
        if route.is_some_and(|route| !route.built_in_v0_1()) {
            return Err(ROUTE_NOT_BUILT);
        }
        let policy = self.config.config.routing.kinds.get(&kind).cloned().unwrap_or_default();
        let spoken = route.is_some() || model.is_some() || depth.is_some();
        let depth_now = depth.or(policy.depth).unwrap_or(Depth::Considered);
        let (model, reason) = if let Some(model) = model {
            let reason = format!("asked for the model {model}");
            (model, reason)
        } else if let Some(depth) = depth {
            (depth_model(depth).to_owned(), format!("asked for {} depth", depth_name(depth)))
        } else if let Some(model) = policy.model {
            (model, format!("the routing policy for {}", kind_name(kind)))
        } else {
            let reason = format!("the default for {} depth", depth_name(depth_now));
            (depth_model(depth_now).to_owned(), reason)
        };
        let route = route.or(policy.route).filter(|route| route.built_in_v0_1());
        let words = if spoken { self.brain.live_words() } else { None };
        Ok(Routed {
            route: route.unwrap_or(Route::Claude),
            model,
            depth: depth_now,
            reason,
            words: words.map(|words| Personal::new(words.to_owned())),
        })
    }
}

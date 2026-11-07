//! The router (BUILD.md section 2.5, "Routing"): each job's route, model and depth, and why,
//! for `JobAccepted` and the receipt.
//!
//! A spoken override beats the policy and is recorded on the job with the brain turn's words:
//! a route or a model the brain passes on, and a depth the user said, which is the turn's L1
//! override ("quick answer", "think hard about this") or a reroute's. A depth the brain chose
//! itself, as it does for every deep answer (section 2.3), is no override: it only picks the
//! depth's model once the kind's policy names none. After the overrides come the kind's
//! policy, then `routing.deep` for a deep answer at deep depth (section 2.3: the deep workers
//! are configured apart from the brain), then the depth's model. A route v0.1 does not build
//! is refused when asked for; a policy that names one falls back to Claude with the depth's
//! model, never with a model meant for the route it named.

use eileen_contracts::config::BackendChoice;
use eileen_contracts::ids::Personal;
use eileen_contracts::jobs::{Depth, JobKind, Route};

use crate::Core;

/// Why a call naming a route v0.1 does not build was refused.
const ROUTE_NOT_BUILT: &str = "Only Claude can take jobs in this version.";

/// What a dispatch or a reroute asks of the router.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Ask {
    pub(crate) route: Option<Route>,
    pub(crate) model: Option<String>,
    pub(crate) depth: Option<Depth>,
    /// Whether the depth is the user's own, as a reroute's is, rather than the brain's choice.
    /// A depth equal to the brain turn's L1 override is the user's either way.
    pub(crate) depth_said: bool,
}

/// What the router decided for a job (section 2.5, "Routing").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Routed {
    pub(crate) route: Route,
    pub(crate) model: String,
    pub(crate) depth: Depth,
    /// Why, for `JobAccepted` and the receipt.
    pub(crate) reason: String,
    /// The user's words, when a spoken override beat the policy.
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

/// The model `depth` uses on `route` when no override or policy names one, and why: the deep
/// workers' model from `routing.deep` (`deep`, given for a deep answer at deep depth) when
/// they run on that route, otherwise the depth's own.
fn depth_choice(deep: Option<&BackendChoice>, depth: Depth, route: Route) -> (String, String) {
    match deep {
        Some(deep) if deep.backend == route => {
            (deep.model.clone(), "routing.deep for deep answers".to_owned())
        }
        Some(deep) if !deep.backend.built_in_v0_1() => {
            let named = deep.backend.display_name();
            let reason = format!("routing.deep names {named}, which this version does not run");
            (depth_model(depth).to_owned(), reason)
        }
        Some(_) | None => {
            let reason = format!("the default for {} depth", depth_name(depth));
            (depth_model(depth).to_owned(), reason)
        }
    }
}

impl Core {
    /// The router (section 2.5): a spoken override (an asked route or model, or a depth the
    /// user said) beats the policy and is recorded with the user's words; then the policy for
    /// the kind, whose model counts only on the route it names; then `routing.deep` for a deep
    /// answer at deep depth; then the depth's model. A route v0.1 does not build is refused
    /// when asked for, and one a policy names falls back to Claude.
    pub(crate) fn route_for(&self, kind: JobKind, ask: &Ask) -> Result<Routed, &'static str> {
        if ask.route.is_some_and(|route| !route.built_in_v0_1()) {
            return Err(ROUTE_NOT_BUILT);
        }
        let routing = &self.config.config.routing;
        let policy = routing.kinds.get(&kind).cloned().unwrap_or_default();
        let depth = ask.depth.or(policy.depth).unwrap_or(Depth::Considered);
        let depth_said =
            ask.depth.is_some_and(|asked| ask.depth_said || self.brain.live_depth() == Some(asked));
        let deep = (kind == JobKind::DeepAnswer && depth == Depth::Deep).then_some(&routing.deep);
        let configured = policy.route.or(deep.map(|deep| deep.backend));
        let route = ask.route.or(configured.filter(|route| route.built_in_v0_1()));
        let route = route.unwrap_or(Route::Claude);
        let policy_model = policy.model.filter(|_| policy.route.is_none_or(|named| named == route));
        let (model, reason) = if let Some(model) = ask.model.clone() {
            let reason = format!("asked for the model {model}");
            (model, reason)
        } else if depth_said {
            let (model, _) = depth_choice(deep, depth, route);
            (model, format!("asked for {} depth", depth_name(depth)))
        } else if let Some(model) = policy_model {
            (model, format!("the routing policy for {}", kind_name(kind)))
        } else {
            depth_choice(deep, depth, route)
        };
        // With no asked model or said depth, an asked route, or a policy's route this version
        // does not run, is what decided.
        let overridden = ask.model.is_some() || depth_said;
        let decided = if overridden {
            None
        } else if let Some(asked) = ask.route {
            Some(format!("asked for {}", asked.display_name()))
        } else {
            policy.route.filter(|named| !named.built_in_v0_1()).map(|named| {
                let named = named.display_name();
                format!("the routing policy names {named}, which this version does not run")
            })
        };
        let reason = decided.unwrap_or(reason);
        let spoken = ask.route.is_some() || overridden;
        let words = if spoken { self.brain.live_words() } else { None };
        Ok(Routed {
            route,
            model,
            depth,
            reason,
            words: words.map(|words| Personal::new(words.to_owned())),
        })
    }
}

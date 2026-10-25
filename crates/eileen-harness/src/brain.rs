//! Brain backends (sections 2.3 and 3.3.8): a warm session that answers, dispatches jobs and
//! runs the deterministic `act` tools, and the brain benchmark (B10).

use std::future::{Future, ready};

use eileen_contracts::harness::BrainEvent;
use eileen_contracts::ids::TurnId;
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;

use crate::{DriverError, HarnessConfig};

/// A vendor CLI or API that can be the brain.
pub trait BrainBackend: Send {
    /// The route this backend serves.
    fn route(&self) -> Route;
    /// The languages its resolved model handles.
    fn languages(&self) -> LanguageSupport;
    /// Sends one turn's composed message.
    fn turn(
        &mut self,
        turn: TurnId,
        message: String,
    ) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// Interrupts the turn in progress.
    fn interrupt(&mut self) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// Starts a fresh session carrying `summary`.
    fn recycle(&mut self, summary: String) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// The next event; `None` once the backend has stopped and every event was delivered.
    fn next_event(&mut self) -> impl Future<Output = Option<BrainEvent>> + Send;
}

/// Declares a brain backend whose every call answers `RouteUnavailable` for its route.
macro_rules! unavailable_brain {
    ($(#[$doc:meta])* $name:ident, $route:expr, $languages:expr) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name;

        impl BrainBackend for $name {
            fn route(&self) -> Route {
                $route
            }

            fn languages(&self) -> LanguageSupport {
                $languages
            }

            fn turn(
                &mut self,
                _turn: TurnId,
                _message: String,
            ) -> impl Future<Output = Result<(), DriverError>> + Send {
                ready(Err(DriverError::RouteUnavailable { route: $route }))
            }

            fn interrupt(&mut self) -> impl Future<Output = Result<(), DriverError>> + Send {
                ready(Err(DriverError::RouteUnavailable { route: $route }))
            }

            fn recycle(
                &mut self,
                _summary: String,
            ) -> impl Future<Output = Result<(), DriverError>> + Send {
                ready(Err(DriverError::RouteUnavailable { route: $route }))
            }

            fn next_event(&mut self) -> impl Future<Output = Option<BrainEvent>> + Send {
                ready(None)
            }
        }
    };
}

unavailable_brain!(
    /// The Claude brain: a warm `claude -p` session with only Eileen's tools. A stub until WP3
    /// builds it.
    ClaudeBrain,
    Route::Claude,
    LanguageSupport::Any
);

unavailable_brain!(
    /// The Codex brain, a designed slot.
    CodexBrain,
    Route::Codex,
    LanguageSupport::Listed(Vec::new())
);

unavailable_brain!(
    /// The `DeepSeek` brain, a designed slot (configuration only in v0.1).
    DeepSeekBrain,
    Route::DeepSeek,
    LanguageSupport::Listed(Vec::new())
);

unavailable_brain!(
    /// A local model as the brain, a designed slot.
    LocalBrain,
    Route::Local,
    LanguageSupport::Listed(Vec::new())
);

/// Every brain backend, dispatched by variant.
#[derive(Debug)]
pub enum AnyBrain {
    /// Claude.
    Claude(ClaudeBrain),
    /// Codex.
    Codex(CodexBrain),
    /// `DeepSeek`.
    DeepSeek(DeepSeekBrain),
    /// A local model.
    Local(LocalBrain),
}

impl BrainBackend for AnyBrain {
    fn route(&self) -> Route {
        match self {
            Self::Claude(brain) => brain.route(),
            Self::Codex(brain) => brain.route(),
            Self::DeepSeek(brain) => brain.route(),
            Self::Local(brain) => brain.route(),
        }
    }

    fn languages(&self) -> LanguageSupport {
        match self {
            Self::Claude(brain) => brain.languages(),
            Self::Codex(brain) => brain.languages(),
            Self::DeepSeek(brain) => brain.languages(),
            Self::Local(brain) => brain.languages(),
        }
    }

    async fn turn(&mut self, turn: TurnId, message: String) -> Result<(), DriverError> {
        match self {
            Self::Claude(brain) => brain.turn(turn, message).await,
            Self::Codex(brain) => brain.turn(turn, message).await,
            Self::DeepSeek(brain) => brain.turn(turn, message).await,
            Self::Local(brain) => brain.turn(turn, message).await,
        }
    }

    async fn interrupt(&mut self) -> Result<(), DriverError> {
        match self {
            Self::Claude(brain) => brain.interrupt().await,
            Self::Codex(brain) => brain.interrupt().await,
            Self::DeepSeek(brain) => brain.interrupt().await,
            Self::Local(brain) => brain.interrupt().await,
        }
    }

    async fn recycle(&mut self, summary: String) -> Result<(), DriverError> {
        match self {
            Self::Claude(brain) => brain.recycle(summary).await,
            Self::Codex(brain) => brain.recycle(summary).await,
            Self::DeepSeek(brain) => brain.recycle(summary).await,
            Self::Local(brain) => brain.recycle(summary).await,
        }
    }

    async fn next_event(&mut self) -> Option<BrainEvent> {
        match self {
            Self::Claude(brain) => brain.next_event().await,
            Self::Codex(brain) => brain.next_event().await,
            Self::DeepSeek(brain) => brain.next_event().await,
            Self::Local(brain) => brain.next_event().await,
        }
    }
}

/// The brain backend for `route`; `RouteUnavailable` for every route but Claude in v0.1. The
/// harness component holds up to three, one per `BrainSlot`, and stamps every event with its
/// process's instance.
#[expect(
    clippy::missing_const_for_fn,
    clippy::trivially_copy_pass_by_ref,
    reason = "the stub's body and section 3.3.8's signature; WP3 locates the CLI here"
)]
pub fn brain_for(route: Route, _cfg: &HarnessConfig) -> Result<AnyBrain, DriverError> {
    match route {
        Route::Claude => Ok(AnyBrain::Claude(ClaudeBrain)),
        Route::Codex | Route::Grok | Route::DeepSeek | Route::Local => {
            Err(DriverError::RouteUnavailable { route })
        }
    }
}

/// The brain benchmark's script (B10, section 4.7): scripted turns, the brain cases of the
/// resolver golden file and interruptions mid-answer. WP3 adds the cases and interruptions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrainSuite {
    /// The scripted turns, in order.
    pub turns: Vec<String>,
}

/// What the brain benchmark measured; `None` where it measured nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainBenchResult {
    /// The route benchmarked.
    pub route: Route,
    /// The resolved model id, when the backend reported one.
    pub model: Option<String>,
    /// Time to the first spoken word, median, in milliseconds.
    pub first_word_p50_ms: Option<u32>,
    /// Time to the first spoken word, 95th percentile, in milliseconds.
    pub first_word_p95_ms: Option<u32>,
    /// The golden cases answered with the right action, per thousand.
    pub right_action_per_mille: Option<u16>,
}

/// Runs the brain benchmark (`eileen bench brain`, `CheckKind::BrainBenchmark`) against
/// `brain`. The stub measures nothing.
pub fn brain_benchmark(
    brain: &mut AnyBrain,
    _suite: &BrainSuite,
) -> impl Future<Output = BrainBenchResult> + Send {
    ready(BrainBenchResult {
        route: brain.route(),
        model: None,
        first_word_p50_ms: None,
        first_word_p95_ms: None,
        right_action_per_mille: None,
    })
}

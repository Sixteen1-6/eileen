//! Worker drivers (section 3.3.8): one session of a vendor CLI that runs one job at a time.

use std::future::{Future, ready};

use eileen_contracts::harness::{JobBinding, SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::WorkerSessionId;
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;

use crate::{DriverError, HarnessConfig};

/// A vendor CLI that runs worker sessions.
pub trait WorkerDriver: Send {
    /// The route this driver serves.
    fn route(&self) -> Route;
    /// The languages its resolved model handles.
    fn languages(&self) -> LanguageSupport;
    /// Spawns a session (pooled, or for one job) and waits for its init event.
    fn start(
        &mut self,
        spec: SessionSpec,
    ) -> impl Future<Output = Result<WorkerSession, DriverError>> + Send;
    /// Gives a started session its job: the first user turn.
    fn bind(&mut self, binding: JobBinding)
    -> impl Future<Output = Result<(), DriverError>> + Send;
    /// Sends a command to the session.
    fn send(&mut self, cmd: WorkerCmd) -> impl Future<Output = Result<(), DriverError>> + Send;
    /// The next event; `None` once the session has ended and every event was delivered.
    fn next_event(&mut self) -> impl Future<Output = Option<WorkerEvent>> + Send;
}

/// A started session, as its init event described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSession {
    /// Eileen's id for the session.
    pub session: WorkerSessionId,
    /// The vendor's own id for it.
    pub vendor_session: String,
    /// The resolved model id it runs.
    pub model: String,
}

/// Declares a worker driver whose every call answers `$error`: `NotBuilt` for the route v0.1
/// builds, until WP3 builds it, and `RouteUnavailable` for a designed slot.
macro_rules! stub_worker {
    ($(#[$doc:meta])* $name:ident, $route:expr, $languages:expr, $error:expr) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name;

        impl WorkerDriver for $name {
            fn route(&self) -> Route {
                $route
            }

            fn languages(&self) -> LanguageSupport {
                $languages
            }

            fn start(
                &mut self,
                _spec: SessionSpec,
            ) -> impl Future<Output = Result<WorkerSession, DriverError>> + Send {
                ready(Err($error))
            }

            fn bind(
                &mut self,
                _binding: JobBinding,
            ) -> impl Future<Output = Result<(), DriverError>> + Send {
                ready(Err($error))
            }

            fn send(
                &mut self,
                _cmd: WorkerCmd,
            ) -> impl Future<Output = Result<(), DriverError>> + Send {
                ready(Err($error))
            }

            fn next_event(&mut self) -> impl Future<Output = Option<WorkerEvent>> + Send {
                ready(None)
            }
        }
    };
}

stub_worker!(
    /// The Claude worker: `claude -p` in stream-json. A stub until WP3 builds it.
    ClaudeWorker,
    Route::Claude,
    LanguageSupport::Any,
    DriverError::NotBuilt { route: Route::Claude }
);

stub_worker!(
    /// The Codex app-server worker, a designed slot.
    CodexWorker,
    Route::Codex,
    LanguageSupport::Listed(Vec::new()),
    DriverError::RouteUnavailable { route: Route::Codex }
);

stub_worker!(
    /// The ACP worker, for Grok and other ACP agents, a designed slot.
    AcpWorker,
    Route::Grok,
    LanguageSupport::Listed(Vec::new()),
    DriverError::RouteUnavailable { route: Route::Grok }
);

/// Every worker driver, dispatched by variant.
#[derive(Debug)]
pub enum AnyWorker {
    /// Claude.
    Claude(ClaudeWorker),
    /// The Codex app server.
    Codex(CodexWorker),
    /// An ACP agent.
    Acp(AcpWorker),
}

impl WorkerDriver for AnyWorker {
    fn route(&self) -> Route {
        match self {
            Self::Claude(driver) => driver.route(),
            Self::Codex(driver) => driver.route(),
            Self::Acp(driver) => driver.route(),
        }
    }

    fn languages(&self) -> LanguageSupport {
        match self {
            Self::Claude(driver) => driver.languages(),
            Self::Codex(driver) => driver.languages(),
            Self::Acp(driver) => driver.languages(),
        }
    }

    async fn start(&mut self, spec: SessionSpec) -> Result<WorkerSession, DriverError> {
        match self {
            Self::Claude(driver) => driver.start(spec).await,
            Self::Codex(driver) => driver.start(spec).await,
            Self::Acp(driver) => driver.start(spec).await,
        }
    }

    async fn bind(&mut self, binding: JobBinding) -> Result<(), DriverError> {
        match self {
            Self::Claude(driver) => driver.bind(binding).await,
            Self::Codex(driver) => driver.bind(binding).await,
            Self::Acp(driver) => driver.bind(binding).await,
        }
    }

    async fn send(&mut self, cmd: WorkerCmd) -> Result<(), DriverError> {
        match self {
            Self::Claude(driver) => driver.send(cmd).await,
            Self::Codex(driver) => driver.send(cmd).await,
            Self::Acp(driver) => driver.send(cmd).await,
        }
    }

    async fn next_event(&mut self) -> Option<WorkerEvent> {
        match self {
            Self::Claude(driver) => driver.next_event().await,
            Self::Codex(driver) => driver.next_event().await,
            Self::Acp(driver) => driver.next_event().await,
        }
    }
}

/// The worker driver for `route`; `RouteUnavailable` unless v0.1 builds it, which is Claude
/// alone.
#[expect(
    clippy::missing_const_for_fn,
    clippy::trivially_copy_pass_by_ref,
    reason = "the stub's body and section 3.3.8's signature; WP3 locates the CLI here"
)]
pub fn worker_for(route: Route, _cfg: &HarnessConfig) -> Result<AnyWorker, DriverError> {
    match route {
        Route::Claude => Ok(AnyWorker::Claude(ClaudeWorker)),
        Route::Codex | Route::Grok | Route::DeepSeek | Route::Local => {
            Err(DriverError::RouteUnavailable { route })
        }
    }
}

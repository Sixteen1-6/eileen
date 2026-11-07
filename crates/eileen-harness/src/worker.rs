//! Worker drivers (section 3.3.8): one session of a vendor CLI that runs one job at a time.
//! Claude's is built ([`ClaudeWorker`]); Codex and ACP are designed slots.

pub(crate) mod claude;

use std::future::{Future, ready};
use std::sync::Arc;

use eileen_contracts::harness::{JobBinding, SessionSpec, WorkerCmd, WorkerEvent};
use eileen_contracts::ids::WorkerSessionId;
use eileen_contracts::jobs::Route;
use eileen_contracts::speech::LanguageSupport;
use eileen_trace::Redactor;

pub use self::claude::ClaudeWorker;
use crate::{DriverError, HarnessConfig};

/// A vendor CLI that runs worker sessions.
pub trait WorkerDriver: Send {
    /// The route this driver serves.
    fn route(&self) -> Route;
    /// The languages its resolved model handles.
    fn languages(&self) -> LanguageSupport;
    /// Spawns a session (pooled, or for one job) and waits until it is ready for its job.
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
    /// Cancel-safe: a call dropped before it answers (in a `select!`, say) loses no event and
    /// no part of a line.
    fn next_event(&mut self) -> impl Future<Output = Option<WorkerEvent>> + Send;
}

/// A session ready for its job. The vendor's id for it and the model it resolved come with
/// its first turn's `WorkerEvent::Init`, which the CLI writes only after the first user line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSession {
    /// Eileen's id for the session.
    pub session: WorkerSessionId,
    /// The process id of the CLI.
    pub pid: u32,
}

/// Declares a worker driver for a designed slot, whose every call answers `$error`
/// (`RouteUnavailable`).
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
    /// Claude, boxed: its session and config make it far larger than the designed slots.
    Claude(Box<ClaudeWorker>),
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
/// alone. Claude's runs `cfg.claude_exe` and redacts with `redactor`, the process's own.
pub fn worker_for(
    route: Route,
    cfg: &HarnessConfig,
    redactor: &Arc<Redactor>,
) -> Result<AnyWorker, DriverError> {
    match route {
        Route::Claude => {
            Ok(AnyWorker::Claude(Box::new(ClaudeWorker::new(cfg.clone(), Arc::clone(redactor)))))
        }
        Route::Codex | Route::Grok | Route::DeepSeek | Route::Local => {
            Err(DriverError::RouteUnavailable { route })
        }
    }
}

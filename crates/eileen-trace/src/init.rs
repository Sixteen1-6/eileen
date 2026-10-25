//! The subscriber stack every Eileen process starts with (observability report section 3.1).
//!
//! One `tracing_subscriber::Registry` with a filter per layer: the error layer (so
//! `SpanTrace::capture` works; a span trace shows each `pii.` field as its length alone), the
//! JSON file at INFO behind a reload handle, the flight recorder ring at `eileen=debug,warn`,
//! the slow-span and stage-stats layer, and a console at WARN only when stderr is a terminal.
//! `tracing-log` bridges dependencies that log through the `log` crate.

use std::collections::BTreeMap;
use std::io::{IsTerminal as _, Write};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use eileen_stdx::clock;
use eileen_stdx::soft_assert::{self, Failure, Mode};
use tracing::{Dispatch, Level, Subscriber};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layered, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer, Registry, reload};

use crate::file::{FileLayer, RedactingMakeWriter};
use crate::names;
use crate::pii::{PiiKey, Policy, SpanTraceFields};
use crate::redact::Redactor;
use crate::retention::{Retained, Retention};
use crate::ring::{Ring, RingLayer};
use crate::slow::{SlowLayer, Stats};

/// Which Eileen process is starting tracing; it names the log files and sizes the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProcessKind {
    /// `eileen.exe run`, the core.
    Core,
    /// `eileen-speech.exe`.
    Speech,
    /// `eileen-desktop.exe`.
    Desktop,
    /// `eileen-apps.exe`.
    Apps,
    /// `eileen-hud.exe`.
    Hud,
    /// `eileen-shim.exe`, one per Claude session.
    Shim,
    /// `eileen-guard.exe`.
    Guard,
    /// A short `eileen` command (`say`, `status`, `scenario check`).
    Cli,
}

impl ProcessKind {
    /// The process name: the log file prefix and the `process` field of every record.
    #[must_use]
    pub const fn process_name(self) -> &'static str {
        match self {
            Self::Core => "eileen",
            Self::Speech => "eileen-speech",
            Self::Desktop => "eileen-desktop",
            Self::Apps => "eileen-apps",
            Self::Hud => "eileen-hud",
            Self::Shim => "eileen-shim",
            Self::Guard => "eileen-guard",
            Self::Cli => "eileen-cli",
        }
    }

    /// The flight recorder's default size: 32 MB in the core, 8 MB elsewhere.
    #[must_use]
    pub const fn ring_bytes(self) -> usize {
        match self {
            Self::Core => 32 * 1024 * 1024,
            Self::Speech
            | Self::Desktop
            | Self::Apps
            | Self::Hud
            | Self::Shim
            | Self::Guard
            | Self::Cli => 8 * 1024 * 1024,
        }
    }
}

/// When the console layer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    /// Only when stderr is a terminal (started from a shell).
    Auto,
    /// Always.
    Always,
    /// Never (sidecars, whose stderr the supervisor forwards).
    Never,
}

/// How tracing starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceConfig {
    /// Where the JSON-lines files go (`%LOCALAPPDATA%\eileen\logs`); `None` writes no files.
    pub log_dir: Option<PathBuf>,
    /// The file filter, in `tracing`'s syntax (`logging.level`).
    pub file_filter: String,
    /// The ring's filter.
    pub ring_filter: String,
    /// The console's filter.
    pub console_filter: String,
    /// When the console runs.
    pub console: Console,
    /// The ring's size; `None` uses the process kind's default.
    pub ring_bytes: Option<usize>,
    /// The ring's window.
    pub ring_window: Duration,
    /// Keep `pii.` fields in the files (`logging.include_personal`).
    pub include_personal: bool,
    /// The per-install key for the files' keyed hash of `pii.` fields, which the runtime
    /// derives at setup and passes to every process it starts. `None`, the default, reduces
    /// them to their length alone: never a constant key. It never shows in `Debug`.
    pub pii_key: Option<PiiKey>,
    /// Slow-span thresholds by span name, milliseconds.
    pub slow_span_ms: BTreeMap<String, u64>,
    /// Daily files each process keeps of its own, today's included: the newest by the date in
    /// the name, matched exactly as `<process>.<YYYY-MM-DD>.jsonl`, pruned when tracing starts
    /// and after each UTC midnight. 1 keeps today's alone, and 0 keeps every file, as
    /// tracing-appender's `max_log_files(0)` did. Other processes' files and other names are
    /// never touched; the core's janitor caps the whole folder at 14 days and 500 MB (BUILD.md
    /// section 2.15).
    pub max_log_files: usize,
}

impl TraceConfig {
    /// The defaults of section 2.13, with files in `log_dir`.
    #[must_use]
    pub fn new(log_dir: Option<PathBuf>) -> Self {
        Self {
            log_dir,
            file_filter: "info".to_owned(),
            ring_filter: "eileen=debug,warn".to_owned(),
            console_filter: "warn".to_owned(),
            console: Console::Auto,
            ring_bytes: None,
            ring_window: Duration::from_mins(10),
            include_personal: false,
            pii_key: None,
            slow_span_ms: SlowLayer::default_thresholds(),
            max_log_files: 14,
        }
    }
}

/// Why tracing could not start.
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    /// A filter did not parse.
    #[error("invalid {which} filter: {detail}")]
    Filter {
        /// Which filter.
        which: &'static str,
        /// Why.
        detail: String,
    },
    /// The log folder could not be used.
    #[error("log files in {dir}: {detail}")]
    Logs {
        /// The folder.
        dir: String,
        /// Why.
        detail: String,
    },
    /// Tracing was already started in this process.
    #[error("tracing is already initialised in this process")]
    AlreadyInitialised,
}

/// The subscriber [`build`] makes.
pub type TraceSubscriber = Layered<Vec<Box<dyn Layer<Registry> + Send + Sync>>, Registry>;

/// Keeps tracing running: dropping it flushes the file writer.
pub struct TraceGuard {
    _file: Option<WorkerGuard>,
    dropped: Option<ErrorCounter>,
    file_filter: Option<reload::Handle<EnvFilter, Registry>>,
    ring: Arc<Ring>,
    stats: Arc<Stats>,
    redactor: Arc<Redactor>,
}

impl std::fmt::Debug for TraceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TraceGuard").field("ring", &self.ring).finish_non_exhaustive()
    }
}

impl TraceGuard {
    /// The flight recorder.
    #[must_use]
    pub const fn ring(&self) -> &Arc<Ring> {
        &self.ring
    }

    /// The per-span-name latency figures.
    #[must_use]
    pub const fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }

    /// The process's Redactor (register run-time secrets with `add_literal`).
    #[must_use]
    pub const fn redactor(&self) -> &Arc<Redactor> {
        &self.redactor
    }

    /// Lines the file writer dropped because its buffer was full (`logs.dropped`).
    #[must_use]
    pub fn dropped_lines(&self) -> usize {
        self.dropped.as_ref().map_or(0, ErrorCounter::dropped_lines)
    }

    /// Changes the file filter at once (`logging.level`, or "detailed logging for the
    /// desktop" for 30 minutes).
    pub fn set_file_filter(&self, filter: &str) -> Result<(), TraceError> {
        let parsed = parse_filter("file", filter)?;
        if let Some(handle) = &self.file_filter {
            handle
                .reload(parsed)
                .map_err(|e| TraceError::Filter { which: "file", detail: e.to_string() })?;
        }
        Ok(())
    }
}

fn parse_filter(which: &'static str, filter: &str) -> Result<EnvFilter, TraceError> {
    EnvFilter::try_new(filter).map_err(|e| TraceError::Filter { which, detail: e.to_string() })
}

/// The log writer's thread name. Linux keeps 15 bytes of a thread's name (`TASK_COMM_LEN` is
/// 16) and std cuts a longer one without a word, so `top -H`, gdb and core dumps would show a
/// cut name.
const LOG_WRITER_THREAD: &str = "eileen-log";

/// The file sink's writer: lines go to `inner` on the log writer's thread, and a full buffer
/// drops a line rather than block an always-on process.
fn log_writer<W: Write + Send + 'static>(inner: W) -> (NonBlocking, WorkerGuard) {
    NonBlockingBuilder::default().lossy(true).thread_name(LOG_WRITER_THREAD).finish(inner)
}

/// The console layer: `fmt`'s format on `writer` under `filter`, uncoloured, every write
/// through `redactor`. `pii.` fields stay (a terminal the owner is watching, as the
/// [`crate::pii`] table says); secrets do not. [`build`] gives it stderr.
pub fn console_layer<S, W>(
    writer: W,
    redactor: Arc<Redactor>,
    filter: &str,
) -> Result<impl Layer<S> + Send + Sync + 'static, TraceError>
where
    S: Subscriber + for<'a> LookupSpan<'a> + 'static,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    // Never coloured, whatever features the dependency graph turns on: the Redactor reads the
    // text `fmt` has written, and a colour code would put its final `m` right before each
    // value, where the card and token rules need a word to start.
    Ok(tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(RedactingMakeWriter::new(writer, redactor))
        .with_filter(parse_filter("console", filter)?))
}

/// Builds the stack for `kind` without installing it (tests use it with `with_default`).
pub fn build(
    kind: ProcessKind,
    config: &TraceConfig,
) -> Result<(TraceSubscriber, TraceGuard), TraceError> {
    let process = kind.process_name();
    let redactor = Arc::new(Redactor::new());
    let ring = Arc::new(Ring::new(
        process,
        config.ring_bytes.unwrap_or_else(|| kind.ring_bytes()),
        config.ring_window,
    ));
    let stats = Arc::new(Stats::default());
    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> =
        vec![tracing_error::ErrorLayer::new(SpanTraceFields).boxed()];

    let mut file_guard = None;
    let mut dropped = None;
    let mut file_filter = None;
    if let Some(dir) = &config.log_dir {
        // Not the appender's `max_log_files`: it matches by prefix, so the core (`eileen`)
        // would prune every other process's files as its own.
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(process)
            .filename_suffix("jsonl")
            .build(dir)
            .map_err(|e| TraceError::Logs {
                dir: dir.display().to_string(),
                detail: e.to_string(),
            })?;
        let rule = Retention { dir: dir.clone(), process, keep: config.max_log_files };
        let (writer, guard) = log_writer(Retained::new(appender, rule, clock::system()));
        dropped = Some(writer.error_counter());
        file_guard = Some(guard);
        let policy = if config.include_personal {
            Policy::Keep
        } else {
            Policy::Reduce { key: config.pii_key }
        };
        let (filter, handle) = reload::Layer::new(parse_filter("file", &config.file_filter)?);
        file_filter = Some(handle);
        layers.push(
            FileLayer::new(process, writer, policy, Arc::clone(&redactor))
                .with_filter(filter)
                .boxed(),
        );
    }
    layers.push(
        RingLayer::new(Arc::clone(&ring))
            .with_filter(parse_filter("ring", &config.ring_filter)?)
            .boxed(),
    );
    layers.push(SlowLayer::new(config.slow_span_ms.clone(), Arc::clone(&stats)).boxed());
    let console = match config.console {
        Console::Auto => std::io::stderr().is_terminal(),
        Console::Always => true,
        Console::Never => false,
    };
    if console {
        layers.push(
            console_layer::<Registry, _>(
                std::io::stderr,
                Arc::clone(&redactor),
                &config.console_filter,
            )?
            .boxed(),
        );
    }
    let subscriber = tracing_subscriber::registry().with(layers);
    Ok((subscriber, TraceGuard { _file: file_guard, dropped, file_filter, ring, stats, redactor }))
}

static GLOBAL_RING: OnceLock<Arc<Ring>> = OnceLock::new();

/// Logs a failed soft assertion and asks for a flight recorder dump.
fn soft_assert_hook(failure: &Failure<'_>) {
    tracing::event!(
        name: names::SOFT_ASSERT_FAILED,
        target: "eileen_trace",
        Level::ERROR,
        file = failure.file,
        line = failure.line,
        message = %failure.message,
    );
    if let Some(ring) = GLOBAL_RING.get() {
        ring.request_dump();
    }
}

/// Starts tracing for this process: installs the stack as the global default, bridges the
/// `log` crate, and switches soft assertions to reporting (log, count, request a dump). Call
/// it once, at the start of `main`, outside any `with_default`.
///
/// Only a global default set before is an error. When a `log` logger was set first (an
/// `env_logger::init()`, say), records logged through `log` stay with it: the rest starts all
/// the same, and a [`names::LOG_BRIDGE_FAILED`] warning says so.
pub fn init(kind: ProcessKind, config: &TraceConfig) -> Result<TraceGuard, TraceError> {
    let (subscriber, guard) = build(kind, config)?;
    let stack = Dispatch::new(subscriber);
    let installed = stack.clone().try_init();
    // `try_init` sets the global default, then the bridge: an error with this stack as this
    // thread's default, the global one outside any `with_default`, is the bridge's alone.
    if installed.is_err() && !tracing::dispatcher::get_default(|now| same_stack(now, &stack)) {
        return Err(TraceError::AlreadyInitialised);
    }
    let _first = GLOBAL_RING.set(Arc::clone(&guard.ring));
    soft_assert::set_hook(soft_assert_hook);
    soft_assert::set_mode(Mode::Report);
    if let Err(error) = installed {
        tracing::warn!(name: names::LOG_BRIDGE_FAILED, target: "eileen_trace", error = %error);
    }
    Ok(guard)
}

/// True when `a` and `b` dispatch to the same stack.
fn same_stack(a: &Dispatch, b: &Dispatch) -> bool {
    a.downcast_ref::<TraceSubscriber>()
        .zip(b.downcast_ref::<TraceSubscriber>())
        .is_some_and(|(a, b)| std::ptr::eq(a, b))
}

#[cfg(test)]
mod tests {
    use super::TraceConfig;
    use crate::pii::PiiKey;

    #[test]
    fn a_config_never_shows_its_pii_key_in_debug() {
        let mut config = TraceConfig::new(None);
        config.pii_key = Some(PiiKey::new([0xAB; 32]));
        let shown = format!("{config:?}");
        assert!(shown.contains("pii_key: Some(PiiKey(..))"), "{shown}");
        assert!(!shown.contains("171, 171"), "no byte of the key: {shown}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_log_writers_thread_keeps_its_whole_name_on_linux() {
        use std::io::Write;
        use std::sync::{Arc, Mutex, PoisonError};

        use super::{LOG_WRITER_THREAD, log_writer};

        /// Keeps the kernel's name for the thread that writes to it, the one `top -H`, gdb
        /// and core dumps show.
        struct KernelName(Arc<Mutex<Option<String>>>);

        impl Write for KernelName {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let comm = std::fs::read_to_string("/proc/thread-self/comm")?;
                *self.0.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(comm.trim_end().to_owned());
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let seen = Arc::new(Mutex::new(None));
        let (mut writer, guard) = log_writer(KernelName(Arc::clone(&seen)));
        writer.write_all(b"a line\n").unwrap();
        // Dropping the guard waits for the thread to write what it was sent.
        drop(guard);
        let name = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(name.as_deref(), Some(LOG_WRITER_THREAD), "Linux keeps 15 bytes of a name");
    }
}

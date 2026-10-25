//! The subscriber stack every Eileen process starts with (observability report section 3.1).
//!
//! One `tracing_subscriber::Registry` with a filter per layer: the error layer (so
//! `SpanTrace::capture` works; a span trace shows each `pii.` field as its length alone), the
//! JSON file at INFO behind a reload handle, the flight recorder ring at `eileen=debug,warn`,
//! the slow-span and stage-stats layer, and a console at WARN only when stderr is a terminal.
//! The file and the console write on threads of their own, so a stalled disk or a terminal that
//! stopped reading never blocks a thread that logs. `tracing-log` bridges dependencies that log
//! through the `log` crate.

use std::collections::BTreeMap;
use std::io::{IsTerminal as _, Write};
use std::path::PathBuf;
use std::sync::mpsc::sync_channel;
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

/// How long dropping a [`WriterThread`] waits for its thread to write what it holds.
/// tracing-appender's guard waits at most 1.1 s (100 ms to queue the stop, 1 s for the thread to
/// take it), except when the buffer is full: then it first prints that on stdout, and a print to
/// a terminal that stopped reading waits until the terminal reads again.
const FLUSH_WAIT: Duration = Duration::from_millis(1_200);

/// The thread that drops a writer's guard for [`WriterThread`]: 12 bytes, within Linux's 15.
const FLUSH_THREAD: &str = "eileen-flush";

/// A sink's writer thread as [`TraceGuard`] keeps it: `dropped` counts the lines a full buffer
/// dropped, and dropping it lets the thread write what it holds, waiting at most [`FLUSH_WAIT`].
struct WriterThread {
    flush: Option<WorkerGuard>,
    dropped: ErrorCounter,
}

impl WriterThread {
    fn new(writer: &NonBlocking, flush: WorkerGuard) -> Self {
        Self { flush: Some(flush), dropped: writer.error_counter() }
    }
}

impl Drop for WriterThread {
    fn drop(&mut self) {
        let Some(flush) = self.flush.take() else { return };
        // The guard goes to a thread of its own, so a guard stuck on stdout stays behind there.
        // If no thread can start, the guard drops here, with the closure.
        let (done_tx, done) = sync_channel(1);
        let flusher = eileen_stdx::thread::spawn_named(FLUSH_THREAD, move || {
            drop(flush);
            let _ignored = done_tx.send(());
        });
        if flusher.is_ok() {
            let _ignored = done.recv_timeout(FLUSH_WAIT);
        }
    }
}

/// Keeps tracing running. The files and the console write on threads of their own, and only
/// while this lives, so hold it for all of `main`:
///
/// - A process that ends without dropping it loses the lines still queued:
///   `std::process::exit` runs no destructors, which is why clippy.toml bans it.
/// - Once it drops, the files and the console write nothing more; the ring still records.
/// - Dropping it lets each writer write what it holds, waiting at most about 1.2 s for each.
///   With a terminal that stopped reading, the wait for the console ends there, and the lines
///   it still holds are lost when the process ends.
///
/// ```no_run
/// # fn main() -> Result<(), eileen_trace::TraceError> {
/// let config = eileen_trace::TraceConfig::new(None);
/// let _guard = eileen_trace::init(eileen_trace::ProcessKind::Cli, &config)?;
/// // The rest of `main`: `_guard` drops when it returns.
/// # Ok(())
/// # }
/// ```
///
/// Discarding it stops both writers at once, so the compiler warns, and the workspace's lints
/// make that an error:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// # fn main() -> Result<(), eileen_trace::TraceError> {
/// let config = eileen_trace::TraceConfig::new(None);
/// eileen_trace::init(eileen_trace::ProcessKind::Cli, &config)?;
/// # Ok(())
/// # }
/// ```
#[must_use = "dropping it stops the file and console writers"]
pub struct TraceGuard {
    file: Option<WriterThread>,
    console: Option<WriterThread>,
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
        self.file.as_ref().map_or(0, |file| file.dropped.dropped_lines())
    }

    /// Lines the console dropped because its buffer was full, as when its terminal stopped
    /// reading for a while; 0 when no console runs.
    #[must_use]
    pub fn console_dropped_lines(&self) -> usize {
        self.console.as_ref().map_or(0, |console| console.dropped.dropped_lines())
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

/// The console writer's thread name: 14 bytes, within Linux's 15.
const CONSOLE_WRITER_THREAD: &str = "eileen-console";

/// The lines the console holds while its terminal is not reading. A console at WARN writes far
/// fewer lines than the files at INFO, and the channel allocates every slot at the start: the
/// default 128,000 lines cost about 4 MB of resident memory, 4,096 about 0.25 MB.
const CONSOLE_LINES: usize = 4_096;

/// The console's writer: lines go to `inner` on the console writer's thread, so a terminal that
/// stops reading (Ctrl-S, a mouse selection in a Windows console, a stalled SSH session) blocks
/// that thread alone, and a full buffer drops a line rather than block the thread that logs.
fn console_writer<W: Write + Send + 'static>(inner: W) -> (NonBlocking, WorkerGuard) {
    NonBlockingBuilder::default()
        .lossy(true)
        .buffered_lines_limit(CONSOLE_LINES)
        .thread_name(CONSOLE_WRITER_THREAD)
        .finish(inner)
}

/// The console layer: `fmt`'s format on `writer` under `filter`, uncoloured, every write
/// through `redactor`. `pii.` fields stay (a terminal the owner is watching, as the
/// [`crate::pii`] table says); secrets do not. It writes on the thread that logs, so [`build`]
/// gives it stderr through a writer with a thread of its own, which never waits for the terminal.
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
    build_on(kind, config, std::io::stderr())
}

/// [`build`], with the console, when `config` runs one, writing to `stderr` (a terminal of a
/// test's own in the unit tests).
fn build_on<E: Write + Send + 'static>(
    kind: ProcessKind,
    config: &TraceConfig,
    stderr: E,
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

    let mut file = None;
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
        file = Some(WriterThread::new(&writer, guard));
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
    let wants_console = match config.console {
        Console::Auto => std::io::stderr().is_terminal(),
        Console::Always => true,
        Console::Never => false,
    };
    let mut console = None;
    if wants_console {
        let (writer, guard) = console_writer(stderr);
        console = Some(WriterThread::new(&writer, guard));
        layers.push(
            console_layer::<Registry, _>(writer, Arc::clone(&redactor), &config.console_filter)?
                .boxed(),
        );
    }
    let subscriber = tracing_subscriber::registry().with(layers);
    Ok((subscriber, TraceGuard { file, console, file_filter, ring, stats, redactor }))
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
/// it once, at the start of `main`, and hold the [`TraceGuard`] it returns until `main` returns.
///
/// A global default set before (an earlier `init`, say) is [`TraceError::AlreadyInitialised`].
/// When a `log` logger was set first (an `env_logger::init()`, say), records logged through
/// `log` stay with it: the rest starts all the same, and a [`names::LOG_BRIDGE_FAILED`]
/// warning says so. Under a scoped default (inside `with_default`, or while a `set_default`
/// guard is held) the stack starts too, but this thread's events go to that default until it
/// ends.
pub fn init(kind: ProcessKind, config: &TraceConfig) -> Result<TraceGuard, TraceError> {
    let (subscriber, guard) = build(kind, config)?;
    let stack = Dispatch::new(subscriber);
    let installed = stack.clone().try_init();
    // `try_init` sets the global default, then the bridge: an error with this stack as the
    // global default is the bridge's alone.
    if installed.is_err() && !is_global_default(&stack) {
        return Err(TraceError::AlreadyInitialised);
    }
    let _first = GLOBAL_RING.set(Arc::clone(&guard.ring));
    soft_assert::set_hook(soft_assert_hook);
    soft_assert::set_mode(Mode::Report);
    if let Err(error) = installed {
        // To the stack itself, past any scoped default this thread has.
        tracing::dispatcher::with_default(&stack, || {
            tracing::warn!(name: names::LOG_BRIDGE_FAILED, target: "eileen_trace", error = %error);
        });
    }
    Ok(guard)
}

/// The thread that reads the global default for [`init`]: 12 bytes, within Linux's 15.
const DEFAULT_CHECK_THREAD: &str = "eileen-trace";

/// True when `stack` is the global default. A new thread has no scoped default, so its default
/// is the global one. If no thread can start, this thread's default stands in, which is the
/// global one outside any `with_default`.
fn is_global_default(stack: &Dispatch) -> bool {
    let ours = stack.clone();
    eileen_stdx::thread::spawn_named(DEFAULT_CHECK_THREAD, move || {
        tracing::dispatcher::get_default(|now| same_stack(now, &ours))
    })
    .ok()
    .and_then(|thread| thread.join().ok())
    .unwrap_or_else(|| tracing::dispatcher::get_default(|now| same_stack(now, stack)))
}

/// True when `a` and `b` dispatch to the same stack.
fn same_stack(a: &Dispatch, b: &Dispatch) -> bool {
    a.downcast_ref::<TraceSubscriber>()
        .zip(b.downcast_ref::<TraceSubscriber>())
        .is_some_and(|(a, b)| std::ptr::eq(a, b))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use tracing::Dispatch;

    use super::{CONSOLE_LINES, Console, ProcessKind, TraceConfig, TraceGuard, build_on};
    use crate::pii::PiiKey;

    /// How long a test waits for a thread before calling it stuck: a writer thread takes a line
    /// at once, and a thread that logs returns at once, so only a failing run waits this long.
    const DEADLINE: Duration = Duration::from_secs(5);

    #[test]
    fn a_config_never_shows_its_pii_key_in_debug() {
        let mut config = TraceConfig::new(None);
        config.pii_key = Some(PiiKey::new([0xAB; 32]));
        let shown = format!("{config:?}");
        assert!(shown.contains("pii_key: Some(PiiKey(..))"), "{shown}");
        assert!(!shown.contains("171, 171"), "no byte of the key: {shown}");
    }

    /// Keeps the kernel's name for the thread that writes to it, the one `top -H`, gdb and core
    /// dumps show.
    #[cfg(target_os = "linux")]
    struct KernelName(Arc<Mutex<Option<String>>>);

    #[cfg(target_os = "linux")]
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

    /// The kernel's name for the thread of the writer that `make` builds.
    #[cfg(target_os = "linux")]
    fn writer_thread_name(
        make: fn(KernelName) -> (super::NonBlocking, super::WorkerGuard),
    ) -> Option<String> {
        let seen = Arc::new(Mutex::new(None));
        let (mut writer, guard) = make(KernelName(Arc::clone(&seen)));
        writer.write_all(b"a line\n").unwrap();
        // Dropping the guard waits for the thread to write what it was sent.
        drop(guard);
        seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_log_writers_thread_keeps_its_whole_name_on_linux() {
        use super::{LOG_WRITER_THREAD, log_writer};

        let name = writer_thread_name(log_writer);
        assert_eq!(name.as_deref(), Some(LOG_WRITER_THREAD), "Linux keeps 15 bytes of a name");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_console_writers_thread_keeps_its_whole_name_on_linux() {
        use super::{CONSOLE_WRITER_THREAD, console_writer};

        let name = writer_thread_name(console_writer);
        assert_eq!(name.as_deref(), Some(CONSOLE_WRITER_THREAD), "Linux keeps 15 bytes of a name");
    }

    /// A terminal that stops reading at the first line it is given, until the test resumes it,
    /// and keeps every line.
    struct Terminal {
        stop: Option<(SyncSender<()>, Receiver<()>)>,
        lines: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Write for Terminal {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some((stopped, resume)) = self.stop.take() {
                let _ignored = stopped.send(());
                let _ignored = resume.recv();
            }
            self.lines.lock().unwrap_or_else(PoisonError::into_inner).push(buf.to_vec());
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A stack whose console's terminal stopped at its first line, after which another thread
    /// logged 10 lines more than the console's buffer holds.
    struct StoppedConsole {
        guard: TraceGuard,
        /// Resumes the terminal.
        resume: SyncSender<()>,
        /// What the terminal got.
        lines: Arc<Mutex<Vec<Vec<u8>>>>,
        /// The thread that logged the lines, and whether it returned within the deadline.
        filler: JoinHandle<()>,
        returned: bool,
    }

    fn stopped_console() -> StoppedConsole {
        let (stopped_tx, stopped) = sync_channel(1);
        let (resume, resume_rx) = sync_channel(1);
        let lines = Arc::new(Mutex::new(Vec::new()));
        let terminal = Terminal { stop: Some((stopped_tx, resume_rx)), lines: Arc::clone(&lines) };
        let mut config = TraceConfig::new(None);
        config.console = Console::Always;
        let (subscriber, guard) = build_on(ProcessKind::Cli, &config, terminal).unwrap();
        let stack = Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&stack, || {
            tracing::warn!(target: "eileen_core", "the line the terminal stops on");
        });
        // The console's thread holds that line, blocked on the terminal, and its buffer is empty.
        stopped.recv_timeout(DEADLINE).unwrap();
        let (returned_tx, returned_rx) = sync_channel(1);
        let filler = eileen_stdx::thread::spawn_named("console-filler", move || {
            tracing::dispatcher::with_default(&stack, || {
                for _ in 0..CONSOLE_LINES + 10 {
                    tracing::warn!(target: "eileen_core", "a line");
                }
            });
            let _ignored = returned_tx.send(());
        })
        .unwrap();
        let returned = returned_rx.recv_timeout(DEADLINE).is_ok();
        StoppedConsole { guard, resume, lines, filler, returned }
    }

    #[test]
    fn the_console_drops_and_counts_lines_past_its_buffer() {
        let console = stopped_console();
        console.resume.send(()).unwrap();
        console.filler.join().unwrap();
        let dropped = console.guard.console_dropped_lines();
        // Dropping the guard waits for the console's thread to write what its buffer held.
        drop(console.guard);
        let written = console.lines.lock().unwrap_or_else(PoisonError::into_inner).len();
        assert!(console.returned, "a full console made a thread that logs wait");
        assert_eq!(dropped, 10, "the lines past the buffer were dropped and counted");
        assert_eq!(written, 1 + CONSOLE_LINES, "the terminal got every line the buffer held");
    }

    /// Stdout's lock, held on a thread of its own until this is dropped, as a print to a
    /// terminal that stopped reading holds it.
    struct HeldStdout {
        release: Option<SyncSender<()>>,
        holder: Option<JoinHandle<()>>,
    }

    impl HeldStdout {
        fn hold() -> Self {
            let (held_tx, held) = sync_channel(1);
            let (release, release_rx) = sync_channel::<()>(1);
            let holder = eileen_stdx::thread::spawn_named("held-stdout", move || {
                let _lock = std::io::stdout().lock();
                held_tx.send(()).unwrap();
                // Until the test releases it, or drops the sender as it unwinds.
                let _ignored = release_rx.recv();
            })
            .unwrap();
            held.recv().unwrap();
            Self { release: Some(release), holder: Some(holder) }
        }
    }

    impl Drop for HeldStdout {
        fn drop(&mut self) {
            drop(self.release.take());
            if let Some(holder) = self.holder.take() {
                let _ignored = holder.join();
            }
        }
    }

    /// Whether libtest's output capture is off: nextest runs each test with `--nocapture`, and
    /// `cargo test` needs `-- --no-capture` (or its older spelling `--nocapture`), or
    /// `RUST_TEST_NOCAPTURE` set to anything but `0`, as libtest reads it.
    fn output_capture_is_off() -> bool {
        std::env::args_os().any(|arg| arg == "--nocapture" || arg == "--no-capture")
            || std::env::var("RUST_TEST_NOCAPTURE").is_ok_and(|value| value != "0")
    }

    #[test]
    fn dropping_the_guard_never_waits_for_a_stopped_terminal() {
        // Every thread a test starts inherits libtest's output capture, which takes
        // tracing-appender's print into its buffer: the print never waits for stdout, and the
        // test would pass whatever the drop does.
        assert!(
            output_capture_is_off(),
            "needs nextest or `cargo test -- --no-capture`: libtest's output capture keeps \
             tracing-appender's print off stdout, so this test cannot see a drop that waits for it"
        );
        let console = stopped_console();
        // tracing-appender's guard finds the console's buffer full and prints that on stdout,
        // whose lock this holds, as a print to a terminal that stopped reading holds it.
        let stdout = HeldStdout::hold();
        let guard = console.guard;
        let (returned_tx, returned_rx) = sync_channel(1);
        let dropper = eileen_stdx::thread::spawn_named("guard-dropper", move || {
            drop(guard);
            let _ignored = returned_tx.send(());
        })
        .unwrap();
        let returned = returned_rx.recv_timeout(DEADLINE).is_ok();
        drop(stdout);
        console.resume.send(()).unwrap();
        console.filler.join().unwrap();
        dropper.join().unwrap();
        assert!(returned, "dropping the guard waited for a stopped terminal");
    }
}

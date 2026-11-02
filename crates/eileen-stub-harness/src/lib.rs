//! A stand-in for `claude -p` in tests (BUILD.md sections 3.3.16 and 4.7): it speaks the same
//! stream-json and plays `StubScriptLine` scripts from `tests/harness-scripts/*.ndjson`, so CI
//! runs workers and the brain with no vendor CLI and no credentials. Development only. A lib and
//! the `eileen-stub-harness` binary.
//!
//! The script is named by `--stub-script <path>` ([`SCRIPT_ARG`]) or, when no argument names
//! one, by `EILEEN_STUB_SCRIPT` ([`SCRIPT_ENV`]); every other argument is ignored, so
//! `ClaudeWorker` can start the stub with the argv it gives the claude CLI. The whole script is
//! loaded and checked before anything plays. Then each line plays in order: an `Event` is
//! written as one stream-json line, its `json` text exactly as the script holds it (key order
//! and number digits included); a `Delay` waits, a `Crash` exits with its code, `Malformed`
//! writes its bytes as one line, a `Narration` is written as an assistant text message whose
//! lines start with `> `, and `AwaitUser` holds until the next `user` line arrives on stdin.
//! After the last line it waits for stdin to close, as the CLI waits for its next turn.
//!
//! # Owns
//!
//! - Playing a script: its lines in order, held where the script waits for a user line, with its
//!   faults (crash, delay, malformed line).
//! - Answering the stdio interrupt: a `control_request` of subtype `interrupt` gets a success
//!   `control_response` with its `request_id` whenever it arrives (between lines, during a
//!   delay or while held). Any other request gets an error reply. The stub answers and plays
//!   on; it does not cut the turn short. The reply copies the `control_response` envelope
//!   Eileen sends the CLI (`docs/research/fixtures/wp3-approval/stdio-control.json`,
//!   `sent_to_cli`) without its inner `response` object. No reply the CLI wrote to an interrupt
//!   has been recorded; WP3 acceptance 3's owner-run check records one, and it replaces this.
//! - The `stub_script` fuzz target's entry (`fuzz_entry`): scripts and stdin lines, both
//!   untrusted input (section 3.3.16 asks a fuzz target of every parser of its formats).
//! - The binary `eileen run --stub-harness` starts in place of the claude CLI ([`run`]).
//!
//! # Does not own
//!
//! - The scripts (`tests/harness-scripts/`, recorded and redacted from real sessions), the
//!   driver it stands in for (`eileen-harness`), or the script format (`eileen-contracts`).
//! - Playing a `PermissionRequest` line, which `wp3/approval-contract` adds (the guard over
//!   loopback HTTP and `approve` through the shim, section 4.7). Until then such a line ends the
//!   run with [`RunError::NotPlayed`], and nothing after it plays: never a silent allow.
//!
//! # Invariants
//!
//! - Event shapes match the real CLI's: the nightly contract test compares them with the
//!   scripts (`cargo xtask contract-claude` on the owner's PC, which WP3 builds).
//! - A script that does not load plays nothing; the error names its line.
//! - Every line played is one stdout line: `load` refuses a `Malformed` line whose bytes hold
//!   `\n` or `\r`, and an `Event` with a `\r` between its tokens.
//! - Every stdin line read is bounded ([`MAX_INPUT_LINE_BYTES`]), and so is the queue between
//!   the reader thread and the player.
//!
//! # Boundaries
//!
//! - Started by `eileen-harness` through the supervisor in tests; no shipped crate depends on it
//!   (development only: section 3.1's table and `process = "tests"` in its metadata; no
//!   repository check enforces it yet).
//! - May depend on `eileen-harness`, `eileen-contracts` and `eileen-stdx`; the lib uses the last
//!   two. Its tests use `eileen-harness` (to read its stdout back), `eileen-supervisor` (to
//!   start it) and `eileen-trace` (the `Redactor`) as dev-dependencies only.
//! - Must not depend on `eileen-trace` (it writes only the protocol on stdout), `eileen-rt`,
//!   `eileen-runtime` or rmcp (section 3.1), or on a `-win` or `-linux` crate (check 15).
//! - Wire types: stream-json and the stdio control lines, as the claude CLI writes them.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-stub-harness`.
//! - Layers: unit tests of the player on in-memory stdio; integration tests that start the
//!   binary through the supervisor and read its stdout with `eileen_harness::stream`; a corpus
//!   test over `tests/harness-scripts/`; the `stub_script` corpus replay (`it::fuzz_corpus`);
//!   the harness's tests against it (WP3 acceptance 2).
//! - A bug here starts as a harness test that passes against the stub and fails against the
//!   real CLI.
//!
//! # Traps
//!
//! - A script holds redacted text only; the `Redactor` runs before a recording becomes one, and
//!   the corpus test checks it leaves every script line unchanged.
//! - A user line that arrives during a turn is kept for the next `AwaitUser`, as the CLI queues
//!   it; one that arrives after the script's last line is [`RunError::ScriptEnded`].
//!
//! # Logs and tracing
//!
//! - None: it writes only the protocol on stdout, and one `Error:` line on stderr when it fails.
//!
//! # Do not change without a decision record
//!
//! - The script format (`StubScriptLine`, a contract type).

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod fuzz_entry;
mod play;
mod script;

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;

pub use play::{Ending, InputError, MAX_INPUT_LINE_BYTES};
pub use script::{Script, ScriptError, load};

/// The argument that names the script: `--stub-script <path>` or `--stub-script=<path>`. Every
/// other argument is ignored, so the stub takes the claude CLI's own argv beside it.
pub const SCRIPT_ARG: &str = "--stub-script";

/// The environment variable that names the script when no argument does, so `ClaudeWorker` can
/// start the stub with exactly the argv it gives the claude CLI.
pub const SCRIPT_ENV: &str = "EILEEN_STUB_SCRIPT";

/// The largest script read. The first scripts are a few kilobytes; a recording of a long
/// session stays far below this.
pub const MAX_SCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// How many stdin lines the reader keeps ahead of the player before it waits for it.
const INPUT_QUEUE: usize = 64;

/// The script named by `args` (the arguments after the program's name), else by `env` (the
/// value of [`SCRIPT_ENV`]). The argument wins when both name one. `--stub-script` with no
/// path after it names none.
#[must_use]
pub fn script_path(
    args: impl IntoIterator<Item = OsString>,
    env: Option<OsString>,
) -> Option<PathBuf> {
    let mut args = args.into_iter();
    let joined = format!("{SCRIPT_ARG}=");
    while let Some(arg) = args.next() {
        if arg == SCRIPT_ARG {
            return args.next().filter(|path| !path.is_empty()).map(PathBuf::from);
        }
        if let Some(path) = arg.to_str().and_then(|arg| arg.strip_prefix(joined.as_str())) {
            return (!path.is_empty()).then(|| PathBuf::from(path));
        }
    }
    env.filter(|path| !path.is_empty()).map(PathBuf::from)
}

/// Reads and loads the script at `path`.
pub fn read_script(path: &Path) -> Result<Script, RunError> {
    let read = |source| RunError::Read { path: path.to_owned(), source };
    let file = File::open(path).map_err(read)?;
    let mut text = String::new();
    file.take(MAX_SCRIPT_BYTES.saturating_add(1)).read_to_string(&mut text).map_err(read)?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_SCRIPT_BYTES {
        return Err(RunError::TooLarge { path: path.to_owned() });
    }
    Ok(load(&text)?)
}

/// Runs the stub harness: loads the script named by this process's arguments or environment,
/// then plays it on stdout while a named thread reads stdin.
pub fn run() -> Result<Ending, RunError> {
    let path = script_path(std::env::args_os().skip(1), std::env::var_os(SCRIPT_ENV))
        .ok_or(RunError::NoScript)?;
    let script = read_script(&path)?;
    let (queue, input) = mpsc::sync_channel(INPUT_QUEUE);
    // The reader is never joined: it may sit in a read of stdin when the script ends, and the
    // process ends with `main` whatever it is doing.
    let _reader = eileen_stdx::thread::spawn_named("stub-stdin", move || {
        play::read_input(io::stdin().lock(), &queue);
    })
    .map_err(RunError::Thread)?;
    play::play(&script, &input, &mut io::stdout().lock())
}

/// Why the stub harness stopped without playing its script to the end.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Neither the argument nor the environment named a script.
    #[error("no script: pass `--stub-script <path>` or set EILEEN_STUB_SCRIPT")]
    NoScript,
    /// The script could not be read.
    #[error("the script {} could not be read: {source}", .path.display())]
    Read {
        /// The script.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// The script is larger than [`MAX_SCRIPT_BYTES`].
    #[error("the script {} is larger than MAX_SCRIPT_BYTES", .path.display())]
    TooLarge {
        /// The script.
        path: PathBuf,
    },
    /// The script did not load.
    #[error(transparent)]
    Script(#[from] ScriptError),
    /// A `PermissionRequest` line, which this stub does not play yet: the run ends there, and
    /// nothing is allowed.
    #[error(
        "line {line} is a PermissionRequest, which the stub does not play yet; the run ends \
         here and nothing was allowed"
    )]
    NotPlayed {
        /// The 1-based line number.
        line: usize,
    },
    /// Stdin carried a line that is not the protocol, or could not be read.
    #[error("stdin: {0}")]
    Input(#[from] InputError),
    /// A user line arrived after the script's last line: a turn the script does not have.
    #[error("a user line arrived after the script ended")]
    ScriptEnded,
    /// Stdout could not be written.
    #[error("stdout could not be written: {0}")]
    Output(io::Error),
    /// The thread that reads stdin did not start.
    #[error("the stdin reader did not start: {0}")]
    Thread(io::Error),
}

/// The `eileen-stub-harness` binary: plays the script and exits with the code a `Crash` line
/// gives, 0 when the script played, or 1 with the reason on standard error. Returns the
/// process's exit code, so no destructor is skipped.
#[must_use]
pub fn main() -> ExitCode {
    match run() {
        Ok(Ending::Crash(code)) => ExitCode::from(code),
        Ok(Ending::Played | Ending::InputClosed) => ExitCode::SUCCESS,
        Err(error) => match writeln!(io::stderr().lock(), "Error: {error}") {
            Ok(()) | Err(_) => ExitCode::FAILURE,
        },
    }
}

#[cfg(test)]
mod tests;

//! Repository tasks for Eileen, run as `cargo xtask <command>` (BUILD.md section 4b.5): the
//! guarded build commands, the fast repository checks, the pull request checks and the machine
//! commands. Every command section 4b.5 names parses; those later work builds say when it does.
//!
//! # Owns
//!
//! - The command line, its exit codes (0 pass, 1 findings, 2 usage or IO, 75 no build slot) and
//!   the guard's use: one slot per command, the re-check, the waiting report and the guard's
//!   log (`guard`, over `eileen-buildguard`).
//! - `lint-repo` (checks 1, 2, 4, 6, 10, 14 and 16 with `xtask/tidy/*.toml`) and `pr-meta`
//!   (checks 7, 8 and the fix half of 19), with the path classes of section 4.6.
//! - `check`, `test`, `clippy`, `build`, `insta`, `ci`, `pre-push`, `sim`, `setup`, `slots`,
//!   `desktop-lock`, `fetch-native` (the native table) and `lock`.
//!
//! # Does not own
//!
//! - Admission, the slots and the machine-wide files (`eileen-buildguard`), the simulator
//!   (`eileen-sim`, which `sim` runs), and the `evidence` job and `codegen` (phase B).
//!
//! # Invariants
//!
//! - Every command that runs cargo takes a slot first and exits 75 without one (tests
//!   `guard::tests::a_refused_command_exits_75_and_logs_the_refusal` and
//!   `guard::tests::an_admitted_command_runs_its_steps_and_logs_the_admission`).
//! - `pr-meta` reads only commits, never the working tree, and only `tests:` and `scenarios:`
//!   lines as ids (test `hyphenated_prose_is_not_an_id`); its path classes equal the `plan`
//!   job's filters (test `ci_filters_match_the_classifier`).
//! - The desktop group's filter equals `.config/nextest.toml`'s (test
//!   `cargo_run::tests::the_desktop_filter_is_nextests`).
//! - Every failure names its check or step, where it is, the fix and the command to rerun.
//!
//! # Boundaries
//!
//! - Called by people and agents at the shell, `.githooks/pre-push` and the workflows.
//! - May depend on `eileen-buildguard`, `serde_json` and `toml` (section 4b.5).
//! - Must not depend on any other Eileen crate, so it compiles in seconds (check 15).
//! - Wire types: the guard's log lines; it reads Markdown, TOML and git output.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p xtask` (it cannot guard its own build).
//! - Layers: unit tests over in-memory trees and repositories for every check, the guard with
//!   fake memory and a manual clock, and scratch git repositories (they need `git` on `PATH`).
//! - A bug here starts as a fixture test: a tree, description or memory reading misjudged.
//!
//! # Traps
//!
//! - Descriptions edited in the browser arrive with CRLF; parsing normalises first.
//! - git exports `GIT_DIR` and friends to hooks, and in a worktree they beat `-C`; every git
//!   command xtask starts removes them.
//! - A `## ` line inside a fenced code block is not a heading, and HTML comments outside code
//!   are removed before anything is read.
//! - Under `fetch-native --print-env`, standard output carries only the variables.
//!
//! # Logs and tracing
//!
//! - No tracing. Reports go to standard output, and on GitHub Actions each finding is also an
//!   `::error` annotation. The guard's log is `buildguard-log.jsonl` in the machine root.
//!
//! # Do not change without a decision record
//!
//! - The Evidence field grammar, the type values and the type-fits-files rule (D63, D68).
//! - The exit codes, which workflows and agents read, and the guard's log format.

#![forbid(unsafe_code)]

mod cargo_run;
mod cli;
mod guard;
mod lint_repo;
mod machine;
mod pr_meta;
mod repo;
mod sha256;

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

/// Exit code for a check that found problems.
pub(crate) const EXIT_FINDINGS: u8 = 1;
/// Exit code for a usage, IO or git error: the check could not run.
pub(crate) const EXIT_ERROR: u8 = 2;

/// The repository root: xtask lives one folder below it.
fn repo_root() -> std::path::PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap_or(manifest_dir).to_path_buf()
}

/// Runs one `cargo xtask` command with the arguments after the program name and returns the
/// process exit code.
pub fn run<I>(args: I) -> ExitCode
where
    I: IntoIterator<Item = OsString>,
{
    let mut out = io::stdout().lock();
    match dispatch(args, &mut out) {
        Ok(code) => code,
        Err(error) => report_output_failure(&error),
    }
}

/// Standard output is gone (a closed pipe, for example), so standard error is the last resort.
/// If it is closed too there is nobody left to tell, and the exit code still says so.
fn report_output_failure(error: &io::Error) -> ExitCode {
    match writeln!(io::stderr().lock(), "xtask: cannot write the report: {error}") {
        Ok(()) | Err(_) => ExitCode::from(EXIT_ERROR),
    }
}

fn dispatch<I>(args: I, out: &mut dyn Write) -> io::Result<ExitCode>
where
    I: IntoIterator<Item = OsString>,
{
    match cli::parse(args) {
        Ok(cli::Command::Help) => {
            out.write_all(cli::USAGE.as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        Ok(cli::Command::PrMeta(options)) => pr_meta::main(&options, out),
        Ok(cli::Command::LintRepo) => lint_repo::main(&repo_root(), out),
        Ok(cli::Command::Cargo(command)) => cargo_run::cargo(&command, out),
        Ok(cli::Command::Ci(guard)) => cargo_run::ci(guard, out),
        Ok(cli::Command::PrePush(options)) => cargo_run::pre_push(&options, out),
        Ok(cli::Command::Sim(options)) => cargo_run::sim(&options, out),
        Ok(cli::Command::Setup) => machine::setup(out),
        Ok(cli::Command::Slots(command)) => machine::slots(command, out),
        Ok(cli::Command::DesktopLock(options)) => machine::desktop_lock(options, out),
        Ok(cli::Command::FetchNative(options)) => machine::fetch_native(options, out),
        Ok(cli::Command::Lock) => machine::lock(out),
        Ok(cli::Command::Later { name, when }) => {
            writeln!(
                out,
                "xtask: `cargo xtask {name}` is not built yet; it arrives with {when} \
                 (BUILD.md section 4b.5)"
            )?;
            Ok(ExitCode::from(EXIT_ERROR))
        }
        Err(problem) => {
            writeln!(out, "xtask: {problem}\n")?;
            out.write_all(cli::USAGE.as_bytes())?;
            Ok(ExitCode::from(EXIT_ERROR))
        }
    }
}

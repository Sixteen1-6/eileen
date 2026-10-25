//! Repository tasks for Eileen, run as `cargo xtask <command>`.
//!
//! This first cut has one command, `pr-meta`, which checks a pull request's title, description
//! and changed files exactly as the `pr-meta` workflow does: checks 7 and 8 and the fix half of
//! check 19 in BUILD.md section 4b.3. The guarded build commands, `lint-repo` and the rest of
//! section 4b.5 arrive with `wp0/xtask`.
//!
//! # Owns
//!
//! - The `cargo xtask` command line and its exit codes (0 pass, 1 findings, 2 usage, IO or git).
//! - `pr-meta`: the title rule, the template and its Evidence grammar, the type-fits-files rule
//!   and the rule that a fix adds or changes a regression.
//! - The path classes of BUILD.md section 4.6 as `pr-meta` reads them.
//!
//! # Does not own
//!
//! - The wording of `.github/PULL_REQUEST_TEMPLATE.md`, which the tests read as the placeholder.
//! - The `evidence` job (a fix's tests run before and after), which arrives in phase B.
//!
//! # Invariants
//!
//! - `pr-meta` compiles nothing and reads only commits, never the working tree: every read goes
//!   through the `Repo` trait, whose one real implementation asks `git` about the base and head.
//! - Only `tests:` and `scenarios:` field lines are read as ids, so prose is never mistaken for an
//!   id (test `hyphenated_prose_is_not_an_id`).
//! - The path classes equal the `plan` job's filters in `.github/workflows/ci.yml` (test
//!   `ci_filters_match_the_classifier`).
//! - Every failure names its check, where it is, the fix and the command that reproduces it.
//!
//! # Boundaries
//!
//! - Called by people and agents at the shell, and by `.github/workflows/pr-meta.yml`.
//! - May depend on std and `toml`; BUILD.md section 3.1 adds `eileen-buildguard` and
//!   `serde_json` when the guarded commands arrive. Nothing else, so it compiles in seconds.
//! - Must not depend on any other Eileen crate.
//! - Wire types: none. It reads Markdown, TOML and git output, and writes a plain report.
//!
//! # Testing
//!
//! - Fast command: `cargo nextest run -p xtask`.
//! - Layers: unit tests over an in-memory repository for every rule, and tests against scratch
//!   git repositories for the git plumbing (they need `git` on `PATH`).
//! - A bug here starts as a fixture test: the title, description and changed files misjudged.
//!
//! # Traps
//!
//! - Descriptions edited in the browser arrive with CRLF line endings; parsing normalises first.
//! - A `## ` line inside a fenced code block is not a heading, and HTML comments outside code are
//!   removed before anything is read, so the template's guidance never counts as evidence while
//!   a `<!--` quoted in a code span or block stays text.
//! - Changed files come from the merge base (`base...head`), not from `base` itself, so commits
//!   that landed on `main` after the branch started are not counted as the pull request's own.
//! - git exports `GIT_DIR` and friends to hooks, and in a worktree they beat `-C`; every git
//!   command xtask starts removes them.
//!
//! # Logs and tracing
//!
//! - No tracing. The report goes to standard output and, on GitHub Actions, each finding is also
//!   an `::error` annotation. Reproduce any run with the `reproduce:` line it prints.
//!
//! # Do not change without a decision record
//!
//! - The Evidence field grammar, the type values and the type-fits-files rule (D63, D68).
//! - The exit codes, which workflows and agents read.

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
const EXIT_FINDINGS: u8 = 1;
/// Exit code for a usage, IO or git error: the check could not run.
const EXIT_ERROR: u8 = 2;

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

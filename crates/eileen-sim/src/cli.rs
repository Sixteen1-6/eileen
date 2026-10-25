//! The `eileen-sim` command line, which `cargo xtask sim` runs as
//! `cargo run --locked -p eileen-sim --bin eileen-sim -- <args>`.
//!
//! With no arguments it runs the pull request seed set. Exit codes: 0 every run passed, 1 an
//! invariant broke or `--doublecheck` found two traces, 2 a usage or IO error.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use eileen_contracts::record::SimRegression;

use crate::profile::{self, Profile};
use crate::run::{self, PR_SEEDS, RunError, RunReport};

/// Every run passed.
pub const EXIT_PASS: u8 = 0;
/// An invariant broke, or `--doublecheck` saw two traces.
pub const EXIT_FAIL: u8 = 1;
/// A usage or IO error.
pub const EXIT_ERROR: u8 = 2;

/// The usage text.
pub const USAGE: &str = "\
usage: eileen-sim [options]

With no options, runs the pull request seed set with the default profile.

  --seed <n>        run this seed
  --profile <name>  use this profile (default: default)
  --steps <n>       take this many steps instead of the profile's
  --doublecheck     run each seed twice and compare the trace hashes
  --replay <file>   replay a stored SimRegression (crates/eileen-sim/regressions/<name>.toml)
  --verbose         print the step log of passing runs too
  --print-schema    print the profiles' JSON Schema (profiles/profile.schema.json)
  --minimize        not built yet (WP1)
";

/// What the command line was asked to do besides running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Action {
    /// Run seeds, or replay a regression.
    #[default]
    Run,
    /// Print the usage text.
    Help,
    /// Print the profiles' schema.
    PrintSchema,
    /// Minimise a failure (WP1).
    Minimize,
}

/// Parsed options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Options {
    action: Action,
    seed: Option<u64>,
    profile: Option<String>,
    steps: Option<u32>,
    doublecheck: bool,
    replay: Option<PathBuf>,
    verbose: bool,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.into_iter().map(|arg| {
        arg.into_string().map_err(|raw| format!("argument `{}` is not valid UTF-8", raw.display()))
    });
    while let Some(arg) = args.next().transpose()? {
        let mut value =
            |flag: &str| args.next().transpose()?.ok_or_else(|| format!("`{flag}` needs a value"));
        match arg.as_str() {
            "--seed" => {
                let text = value("--seed")?;
                options.seed = Some(
                    text.parse().map_err(|_| format!("`--seed` needs a number, not `{text}`"))?,
                );
            }
            "--profile" => options.profile = Some(value("--profile")?),
            "--steps" => {
                let text = value("--steps")?;
                options.steps = Some(
                    text.parse().map_err(|_| format!("`--steps` needs a number, not `{text}`"))?,
                );
            }
            "--replay" => options.replay = Some(PathBuf::from(value("--replay")?)),
            "--doublecheck" => options.doublecheck = true,
            "--verbose" => options.verbose = true,
            "--print-schema" => options.action = Action::PrintSchema,
            "--minimize" => options.action = Action::Minimize,
            "--help" | "-h" => options.action = Action::Help,
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    if options.replay.is_some() && (options.seed.is_some() || options.doublecheck) {
        return Err("`--replay` takes no `--seed` or `--doublecheck`".to_owned());
    }
    Ok(options)
}

/// Runs the command line with `args` (after the program name), writing the report to `out`
/// and errors to `err`, and returns the exit code.
pub fn main(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    match dispatch(args, out, err) {
        Ok(code) => code,
        // The report itself could not be written; there is nobody left to tell.
        Err(_) => EXIT_ERROR,
    }
}

fn dispatch(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<u8> {
    let options = match parse(args) {
        Ok(options) => options,
        Err(problem) => {
            writeln!(err, "eileen-sim: {problem}\n\n{USAGE}")?;
            return Ok(EXIT_ERROR);
        }
    };
    match options.action {
        Action::Run => {}
        Action::Help => {
            out.write_all(USAGE.as_bytes())?;
            return Ok(EXIT_PASS);
        }
        Action::PrintSchema => {
            out.write_all(profile::schema_json().as_bytes())?;
            return Ok(EXIT_PASS);
        }
        Action::Minimize => {
            writeln!(err, "eileen-sim: `--minimize` is not built yet; WP1 adds the minimiser")?;
            return Ok(EXIT_ERROR);
        }
    }
    match run_options(&options, out) {
        Ok(code) => Ok(code),
        Err(error) => {
            writeln!(err, "eileen-sim: {error}")?;
            Ok(EXIT_ERROR)
        }
    }
}

/// What went wrong before a run could finish.
#[derive(Debug)]
enum CliError {
    Run(RunError),
    Io(io::Error),
    Regression(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Regression(why) => write!(f, "{why}"),
        }
    }
}

impl From<RunError> for CliError {
    fn from(error: RunError) -> Self {
        Self::Run(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn run_options(options: &Options, out: &mut dyn Write) -> Result<u8, CliError> {
    if let Some(path) = &options.replay {
        let text = std::fs::read_to_string(path)?;
        let regression: SimRegression = toml::from_str(&text).map_err(|e| {
            CliError::Regression(format!(
                "{} is not a SimRegression: {}",
                path.display(),
                e.message()
            ))
        })?;
        let report = run::replay(&regression)?;
        return Ok(report_one(&report, options.verbose, out)?);
    }
    let profile =
        Profile::load(options.profile.as_deref().unwrap_or("default")).map_err(RunError::from)?;
    let seeds: Vec<u64> = options.seed.map_or_else(|| PR_SEEDS.to_vec(), |seed| vec![seed]);
    let mut code = EXIT_PASS;
    for seed in seeds {
        let report = if options.doublecheck {
            match run::doublecheck(&profile, seed, options.steps)? {
                Ok(report) => report,
                Err((first, second)) => {
                    writeln!(
                        out,
                        "seed {seed}: FAIL doublecheck, the trace hashes differ \
                         ({first:#018x} then {second:#018x})"
                    )?;
                    code = EXIT_FAIL;
                    continue;
                }
            }
        } else {
            run::run_seed(&profile, seed, options.steps)?
        };
        if report_one(&report, options.verbose, out)? != EXIT_PASS {
            code = EXIT_FAIL;
        }
    }
    Ok(code)
}

fn report_one(report: &RunReport, verbose: bool, out: &mut dyn Write) -> io::Result<u8> {
    let name = report.seed.map_or_else(|| "replay".to_owned(), |seed| format!("seed {seed}"));
    match &report.failure {
        None => {
            if verbose {
                out.write_all(report.log.as_bytes())?;
            }
            writeln!(
                out,
                "{name}: pass, {} steps of profile `{}`, trace {:#018x}",
                report.steps, report.profile, report.hash
            )?;
            Ok(EXIT_PASS)
        }
        Some((step, violation)) => {
            out.write_all(report.log.as_bytes())?;
            writeln!(out, "{name}: FAIL at step {step}: {violation}")?;
            writeln!(out, "replay: {}", report.replay_command())?;
            Ok(EXIT_FAIL)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{EXIT_ERROR, EXIT_PASS, main};

    fn run(args: &[&str]) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = main(args.iter().map(OsString::from), &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn one_seed_passes_and_reports_its_trace() {
        let (code, out, _) = run(&["--seed", "3", "--steps", "100"]);
        assert_eq!(code, EXIT_PASS, "{out}");
        assert!(out.starts_with("seed 3: pass, 100 steps of profile `default`, trace 0x"), "{out}");
    }

    #[test]
    fn doublecheck_passes_and_verbose_prints_the_log() {
        let (code, out, _) = run(&["--seed", "4", "--steps", "20", "--doublecheck", "--verbose"]);
        assert_eq!(code, EXIT_PASS, "{out}");
        assert_eq!(out.lines().count(), 21, "twenty log lines and the summary: {out}");
    }

    #[test]
    fn usage_errors_exit_2() {
        assert_eq!(run(&["--seed"]).0, EXIT_ERROR, "a flag without its value");
        assert_eq!(run(&["--seed", "x"]).0, EXIT_ERROR, "a seed that is not a number");
        assert_eq!(run(&["--flavour"]).0, EXIT_ERROR, "an unknown option");
        assert_eq!(run(&["--profile", "roomy"]).0, EXIT_ERROR, "an unknown profile");
        let (code, _, err) = run(&["--minimize"]);
        assert_eq!(code, EXIT_ERROR, "the minimiser is WP1's");
        assert!(err.contains("not built yet"), "{err}");
        assert_eq!(run(&["--replay", "missing.toml"]).0, EXIT_ERROR, "a file that is not there");
    }

    #[test]
    fn the_schema_prints() {
        let (code, out, _) = run(&["--print-schema"]);
        assert_eq!(code, EXIT_PASS, "{out}");
        assert!(out.contains("\"Profile\""), "{out}");
    }
}

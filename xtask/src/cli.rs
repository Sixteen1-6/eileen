//! The command line: `cargo xtask <command> [options]`, parsed by hand because xtask keeps its
//! dependencies to the three BUILD.md section 4b.5 allows.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// The usage text printed by `cargo xtask help` and after a usage error.
pub(crate) const USAGE: &str = "\
usage: cargo xtask <command> [options]

Commands that run cargo take a build slot first (BUILD.md section 6.1 rule 1). When the guard
refuses one, they wait, saying why every 30 s, and exit with code 75 after --wait-max.

commands:
  check      cargo check (default --workspace --all-targets)
  test       cargo nextest run (default --workspace); --owner runs the owner profile
  clippy     cargo clippy (default --workspace --all-targets)
  build      cargo build; --timings is cargo's, --release takes both slots
  insta      cargo insta test --unreferenced reject; --review is cargo-insta's
  ci         what the pull request lane runs, then the CI-only steps it skipped
  pre-push   fmt, clippy, the changed crates' tests and lint-repo (.githooks/pre-push)
  sim        the simulator's pull request seeds, or the eileen-sim arguments given
  lint-repo  the fast repository checks (BUILD.md section 4b.3)
  pr-meta    check a pull request's title, description and changed files (checks 7, 8, 19)
  setup      the pinned toolchain, the prebuilt tools and the git hooks
  slots      list the build slots' holders; --kill-stale, --baseline [--hours <n>],
             --pressure <GB>
  desktop-lock --hold
             hold desktop.lock until Enter or until stopped, for manual desktop work
  fetch-native
             fetch and verify the pinned native libraries (Windows); --print-env prints the
             variables cargo needs, one NAME=value per line
  lock       cargo update --workspace, failing if any third-party version changed
  help       print this text

Options of the commands that take a slot:
  --wait-max <minutes>          how long to wait for a slot (default 30)
  --recheck-wait-max <minutes>  how long a running build waits on its re-check (default 20)
  --exclusive                   take both slots (speech models, soaks, benchmarks)
Every other argument goes to cargo (or to eileen-sim for `sim`); `--` passes the rest as is.

cargo xtask pr-meta --body-file <file> [--title <title>] [--base <rev>] [--head <rev>] [--author <login>]
  --body-file  the pull request description, as Markdown
  --title      the pull request title (default: the head commit's subject, which GitHub proposes
               as the title of a one-commit pull request; the report says which title it checked)
  --base       the branch the pull request merges into (default origin/main)
  --head       the pull request's last commit (default HEAD)
  --author     the pull request author's login; dependabot[bot] gets the bot rule (default: a person)

cargo xtask pre-push [--base <rev>]
  --base       what the changed crates are counted from (default origin/main)
";

/// Every command BUILD.md section 4b.5 names, built or not. Check 4 of `lint-repo` reads this
/// list: a guidance file may name any of them.
pub(crate) const COMMANDS: &[&str] = &[
    "setup",
    "slots",
    "desktop-lock",
    "check",
    "test",
    "clippy",
    "build",
    "insta",
    "ci",
    "pre-push",
    "lint-repo",
    "pr-meta",
    "evidence",
    "codegen",
    "sim",
    "fuzz-replay",
    "fuzz",
    "scenarios",
    "e2e",
    "bench",
    "soak",
    "measure-idle",
    "nightly-local",
    "fetch-native",
    "lock",
    "deps-age",
    "symbolicate",
    "triage",
    "docs-drift",
    "contract-claude",
    "fleet-validate",
    "tidy",
    "help",
];

/// The commands of section 4b.5 that later work builds, with when it arrives. They parse, so
/// guidance can name them, and say they are not built yet.
const LATER: &[(&str, &str)] = &[
    ("evidence", "WP0 phase B"),
    ("codegen", "WP0 phase B"),
    ("scenarios", "WP0 phase B"),
    ("measure-idle", "WP0 phase B"),
    ("deps-age", "WP0 phase B"),
    ("symbolicate", "WP0 phase B"),
    ("tidy", "WP0 phase B"),
    ("contract-claude", "WP0 phase B, as a dispatcher to WP3's harness"),
    ("triage", "WP0 phase B, as a dispatcher to WP9's binary"),
    ("bench", "WP0 phase B, as a dispatcher to the harness and end-to-end binaries"),
    ("fleet-validate", "WP0 phase B, as a dispatcher to WP9's binary"),
    ("fuzz-replay", "the pull request that adds fuzz corpus replay to CI"),
    ("fuzz", "the pull request that adds fuzzing to CI"),
    ("e2e", "WP9, as a dispatcher to its binary"),
    ("soak", "the nightly lane"),
    ("nightly-local", "the owner's nightly lane"),
    ("docs-drift", "WP0 phase B"),
];

/// A parsed command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Print the usage text.
    Help,
    /// Run the pull request checks.
    PrMeta(PrMetaOptions),
    /// Run the fast repository checks.
    LintRepo,
    /// One guarded cargo command.
    Cargo(CargoCommand),
    /// What the pull request lane runs, locally.
    Ci(GuardOptions),
    /// The checks `.githooks/pre-push` runs.
    PrePush(PrePushOptions),
    /// The simulator, through the guard.
    Sim(SimOptions),
    /// The toolchain, the prebuilt tools and the hooks.
    Setup,
    /// The build slots.
    Slots(SlotsCommand),
    /// Hold `desktop.lock`.
    DesktopLock(DesktopLockOptions),
    /// The pinned native libraries.
    FetchNative(FetchNativeOptions),
    /// Update the lockfile within the pins.
    Lock,
    /// A command of section 4b.5 that later work builds.
    Later {
        /// The command.
        name: &'static str,
        /// When it arrives.
        when: &'static str,
    },
}

/// Options of `cargo xtask pr-meta`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PrMetaOptions {
    /// The file holding the pull request description.
    pub(crate) body_file: PathBuf,
    /// The pull request title, or `None` to check the head commit's subject.
    pub(crate) title: Option<String>,
    /// The base revision.
    pub(crate) base: String,
    /// The head revision.
    pub(crate) head: String,
    /// The author's login, empty for a person who did not say.
    pub(crate) author: String,
}

/// The guard's options, shared by every command that takes a build slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuardOptions {
    /// How long to wait for a slot.
    pub(crate) wait_max: Duration,
    /// How long a running build waits on its re-check.
    pub(crate) recheck_wait_max: Duration,
    /// Both slots.
    pub(crate) exclusive: bool,
}

impl Default for GuardOptions {
    fn default() -> Self {
        Self {
            wait_max: eileen_buildguard::guard::DEFAULT_WAIT_MAX,
            recheck_wait_max: eileen_buildguard::guard::DEFAULT_RECHECK_WAIT_MAX,
            exclusive: false,
        }
    }
}

/// Which cargo tool a guarded command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CargoTool {
    /// `cargo check`.
    Check,
    /// `cargo nextest run`, with the owner profile when `owner` is set.
    Test {
        /// `--owner`: nextest's `owner` profile, which runs the owner's-PC tests too.
        owner: bool,
    },
    /// `cargo clippy`.
    Clippy,
    /// `cargo build`.
    Build,
    /// `cargo insta test --unreferenced reject`.
    Insta,
}

/// A guarded cargo command: the tool, the guard's options and the arguments cargo gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoCommand {
    /// The tool.
    pub(crate) tool: CargoTool,
    /// The guard's options.
    pub(crate) guard: GuardOptions,
    /// The arguments passed to cargo after the tool's own.
    pub(crate) args: Vec<String>,
}

/// Options of `cargo xtask pre-push`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrePushOptions {
    /// The guard's options.
    pub(crate) guard: GuardOptions,
    /// What the changed crates are counted from.
    pub(crate) base: String,
}

/// Options of `cargo xtask sim`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimOptions {
    /// The guard's options.
    pub(crate) guard: GuardOptions,
    /// The arguments `eileen-sim` gets; none runs the pull request seeds.
    pub(crate) args: Vec<String>,
}

/// The forms of `cargo xtask slots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotsCommand {
    /// List the slots and their holders.
    List,
    /// Remove the records of holders whose locks are free.
    KillStale,
    /// Sample every 5 s and keep `buildguard.toml` current, for `hours` or until stopped.
    Baseline {
        /// How long to sample, when given.
        hours: Option<u32>,
    },
    /// Commit and touch this many GB for 60 s.
    Pressure {
        /// The size, in GB.
        gb: u32,
    },
}

/// Options of `cargo xtask desktop-lock --hold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesktopLockOptions {
    /// How long to wait for the lock.
    pub(crate) wait_max: Duration,
}

/// Options of `cargo xtask fetch-native`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FetchNativeOptions {
    /// Print the variables cargo needs, `NAME=value` per line, and nothing else.
    pub(crate) print_env: bool,
}

/// Parses the arguments after the program name.
pub(crate) fn parse<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|raw| format!("argument `{}` is not valid UTF-8", raw.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "help" | "--help" | "-h" => no_options(&command, &rest).map(|()| Command::Help),
        "pr-meta" => parse_pr_meta(rest).map(Command::PrMeta),
        "lint-repo" => no_options(&command, &rest).map(|()| Command::LintRepo),
        "check" => parse_cargo(CargoTool::Check, rest).map(Command::Cargo),
        "test" => parse_cargo(CargoTool::Test { owner: false }, rest).map(Command::Cargo),
        "clippy" => parse_cargo(CargoTool::Clippy, rest).map(Command::Cargo),
        "build" => parse_cargo(CargoTool::Build, rest).map(Command::Cargo),
        "insta" => parse_cargo(CargoTool::Insta, rest).map(Command::Cargo),
        "ci" => parse_guard_only(&command, rest).map(Command::Ci),
        "pre-push" => parse_pre_push(rest).map(Command::PrePush),
        "sim" => {
            let (guard, args) = split_guard(rest)?;
            Ok(Command::Sim(SimOptions { guard, args }))
        }
        "setup" => no_options(&command, &rest).map(|()| Command::Setup),
        "slots" => parse_slots(&rest).map(Command::Slots),
        "desktop-lock" => parse_desktop_lock(rest).map(Command::DesktopLock),
        "fetch-native" => parse_fetch_native(&rest).map(Command::FetchNative),
        "lock" => no_options(&command, &rest).map(|()| Command::Lock),
        other => match LATER.iter().find(|(name, _)| *name == other) {
            Some(&(name, when)) => Ok(Command::Later { name, when }),
            None => Err(format!("unknown command `{other}`")),
        },
    }
}

fn no_options(command: &str, rest: &[String]) -> Result<(), String> {
    match rest.first() {
        None => Ok(()),
        Some(arg) => Err(format!("{command} does not take `{arg}`")),
    }
}

fn parse_pr_meta(rest: Vec<String>) -> Result<PrMetaOptions, String> {
    let mut args = rest.into_iter();
    let mut body_file = None;
    let mut title = None;
    let mut base = None;
    let mut head = None;
    let mut author = None;
    while let Some(arg) = args.next() {
        let (flag, inline) = split_flag(arg);
        let slot = match flag.as_str() {
            "--body-file" => &mut body_file,
            "--title" => &mut title,
            "--base" => &mut base,
            "--head" => &mut head,
            "--author" => &mut author,
            _ => return Err(format!("pr-meta does not take `{flag}`")),
        };
        if slot.is_some() {
            return Err(format!("`{flag}` is given twice"));
        }
        *slot = Some(value_of(&flag, inline, &mut args)?);
    }
    Ok(PrMetaOptions {
        body_file: PathBuf::from(body_file.ok_or("pr-meta needs `--body-file <file>`")?),
        title,
        base: base.unwrap_or_else(|| "origin/main".to_owned()),
        head: head.unwrap_or_else(|| "HEAD".to_owned()),
        author: author.unwrap_or_default(),
    })
}

/// Takes the guard's own options out of `rest`, up to a `--`, and returns them with every other
/// argument in order (what follows `--` passes through as it is).
fn split_guard(rest: Vec<String>) -> Result<(GuardOptions, Vec<String>), String> {
    let mut guard = GuardOptions::default();
    let mut seen: Vec<String> = Vec::new();
    let mut passed = Vec::new();
    let mut args = rest.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            passed.extend(args.by_ref());
            break;
        }
        let (flag, inline) = split_flag(arg);
        match flag.as_str() {
            "--wait-max" | "--recheck-wait-max" | "--exclusive" => {
                if seen.contains(&flag) {
                    return Err(format!("`{flag}` is given twice"));
                }
                seen.push(flag.clone());
            }
            _ => {
                passed.push(match inline {
                    Some(value) => format!("{flag}={value}"),
                    None => flag,
                });
                continue;
            }
        }
        match flag.as_str() {
            "--exclusive" => {
                if inline.is_some() {
                    return Err("`--exclusive` takes no value".to_owned());
                }
                guard.exclusive = true;
            }
            "--wait-max" => guard.wait_max = minutes(&flag, &value_of(&flag, inline, &mut args)?)?,
            _ => {
                guard.recheck_wait_max = minutes(&flag, &value_of(&flag, inline, &mut args)?)?;
            }
        }
    }
    Ok((guard, passed))
}

fn parse_cargo(tool: CargoTool, rest: Vec<String>) -> Result<CargoCommand, String> {
    let (guard, mut args) = split_guard(rest)?;
    let tool = match tool {
        CargoTool::Test { .. } => {
            let before = args.len();
            args.retain(|arg| arg != "--owner");
            CargoTool::Test { owner: args.len() != before }
        }
        CargoTool::Check | CargoTool::Clippy | CargoTool::Build | CargoTool::Insta => tool,
    };
    Ok(CargoCommand { tool, guard, args })
}

fn parse_guard_only(command: &str, rest: Vec<String>) -> Result<GuardOptions, String> {
    let (guard, args) = split_guard(rest)?;
    no_options(command, &args).map(|()| guard)
}

fn parse_pre_push(rest: Vec<String>) -> Result<PrePushOptions, String> {
    let (guard, args) = split_guard(rest)?;
    let mut base = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = split_flag(arg);
        if flag != "--base" {
            return Err(format!("pre-push does not take `{flag}`"));
        }
        if base.is_some() {
            return Err("`--base` is given twice".to_owned());
        }
        base = Some(value_of(&flag, inline, &mut args)?);
    }
    Ok(PrePushOptions { guard, base: base.unwrap_or_else(|| "origin/main".to_owned()) })
}

fn parse_slots(rest: &[String]) -> Result<SlotsCommand, String> {
    let mut args = rest.iter().cloned();
    let Some(first) = args.next() else {
        return Ok(SlotsCommand::List);
    };
    let (flag, inline) = split_flag(first);
    let command = match flag.as_str() {
        "--kill-stale" if inline.is_none() => SlotsCommand::KillStale,
        "--baseline" if inline.is_none() => {
            let mut hours = None;
            while let Some(arg) = args.next() {
                let (flag, inline) = split_flag(arg);
                if flag != "--hours" || hours.is_some() {
                    return Err(format!("slots --baseline does not take `{flag}` here"));
                }
                hours = Some(number(&flag, &value_of(&flag, inline, &mut args)?)?);
            }
            SlotsCommand::Baseline { hours }
        }
        "--pressure" => {
            SlotsCommand::Pressure { gb: number(&flag, &value_of(&flag, inline, &mut args)?)? }
        }
        _ => return Err(format!("slots does not take `{flag}`")),
    };
    match args.next() {
        None => Ok(command),
        Some(extra) => Err(format!("slots does not take `{extra}` here")),
    }
}

fn parse_desktop_lock(rest: Vec<String>) -> Result<DesktopLockOptions, String> {
    let (guard, args) = split_guard(rest)?;
    if guard.exclusive || guard.recheck_wait_max != GuardOptions::default().recheck_wait_max {
        return Err("desktop-lock takes only `--hold` and `--wait-max`".to_owned());
    }
    match args.as_slice() {
        [hold] if hold == "--hold" => Ok(DesktopLockOptions { wait_max: guard.wait_max }),
        _ => Err("desktop-lock needs `--hold` (and takes only `--wait-max` besides)".to_owned()),
    }
}

fn parse_fetch_native(rest: &[String]) -> Result<FetchNativeOptions, String> {
    match rest {
        [] => Ok(FetchNativeOptions::default()),
        [flag] if flag == "--print-env" => Ok(FetchNativeOptions { print_env: true }),
        [other, ..] => Err(format!("fetch-native does not take `{other}`")),
    }
}

/// The value of `flag`: its inline `=value`, or the next argument.
fn value_of(
    flag: &str,
    inline: Option<String>,
    args: &mut impl Iterator<Item = String>,
) -> Result<String, String> {
    match inline {
        Some(value) => Ok(value),
        None => args.next().ok_or_else(|| format!("`{flag}` needs a value")),
    }
}

fn number(flag: &str, value: &str) -> Result<u32, String> {
    value.parse().map_err(|_| format!("`{flag}` needs a whole number, not `{value}`"))
}

fn minutes(flag: &str, value: &str) -> Result<Duration, String> {
    number(flag, value).map(|m| Duration::from_secs(u64::from(m).saturating_mul(60)))
}

/// Splits `--flag=value` into its flag and value; anything else is a flag with no inline value.
fn split_flag(arg: String) -> (String, Option<String>) {
    if arg.starts_with("--")
        && let Some((flag, value)) = arg.split_once('=')
    {
        return (flag.to_owned(), Some(value.to_owned()));
    }
    (arg, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse_strs(&[]), Ok(Command::Help), "bare `cargo xtask` should print usage");
    }

    #[test]
    fn pr_meta_takes_both_flag_forms_and_defaults() {
        let parsed = parse_strs(&["pr-meta", "--body-file", "b.md", "--title=Add a check"]);
        assert_eq!(
            parsed,
            Ok(Command::PrMeta(PrMetaOptions {
                body_file: PathBuf::from("b.md"),
                title: Some("Add a check".to_owned()),
                base: "origin/main".to_owned(),
                head: "HEAD".to_owned(),
                author: String::new(),
            })),
            "flags with a space or an equals sign, base and head defaulted"
        );
    }

    #[test]
    fn pr_meta_runs_as_documented_with_only_the_body_file() {
        let parsed = parse_strs(&["pr-meta", "--body-file", "b.md"]);
        assert!(
            matches!(parsed, Ok(Command::PrMeta(PrMetaOptions { title: None, .. }))),
            "BUILD.md section 4b.3's local form needs no title: {parsed:?}"
        );
    }

    #[test]
    fn pr_meta_rejects_missing_repeated_and_unknown_flags() {
        let missing = parse_strs(&["pr-meta", "--title", "Add a check"]);
        assert!(missing.is_err_and(|e| e.contains("--body-file")), "body file is required");
        let twice = parse_strs(&["pr-meta", "--title", "A", "--title", "B"]);
        assert!(twice.is_err_and(|e| e.contains("twice")), "a repeated flag is an error");
        let unknown = parse_strs(&["pr-meta", "--bodyfile", "b.md"]);
        assert!(unknown.is_err_and(|e| e.contains("--bodyfile")), "a misspelt flag is named");
        let dangling = parse_strs(&["pr-meta", "--body-file"]);
        assert!(dangling.is_err_and(|e| e.contains("needs a value")), "a flag needs its value");
    }

    #[test]
    fn unknown_command_is_named() {
        let parsed = parse_strs(&["lint-repos"]);
        assert!(parsed.is_err_and(|e| e.contains("lint-repos")), "the unknown command is named");
    }

    #[test]
    fn guarded_commands_take_the_guards_options_and_pass_the_rest_to_cargo() {
        let parsed = parse_strs(&[
            "test",
            "-p",
            "eileen-core",
            "--wait-max",
            "5",
            "--exclusive",
            "--owner",
            "--",
            "--exclusive",
        ]);
        assert_eq!(
            parsed,
            Ok(Command::Cargo(CargoCommand {
                tool: CargoTool::Test { owner: true },
                guard: GuardOptions {
                    wait_max: Duration::from_secs(300),
                    exclusive: true,
                    ..GuardOptions::default()
                },
                args: vec!["-p".to_owned(), "eileen-core".to_owned(), "--exclusive".to_owned()],
            })),
            "the guard's flags are taken out, and everything after `--` passes through"
        );
        let build = parse_strs(&["build", "--timings", "--recheck-wait-max=1"]);
        assert_eq!(
            build,
            Ok(Command::Cargo(CargoCommand {
                tool: CargoTool::Build,
                guard: GuardOptions {
                    recheck_wait_max: Duration::from_secs(60),
                    ..GuardOptions::default()
                },
                args: vec!["--timings".to_owned()],
            })),
            "cargo's own --timings passes through"
        );
    }

    #[test]
    fn guard_options_need_whole_minutes_and_come_once() {
        let bad = parse_strs(&["check", "--wait-max", "soon"]);
        assert!(bad.is_err_and(|e| e.contains("whole number")), "minutes are numbers");
        let twice = parse_strs(&["clippy", "--exclusive", "--exclusive"]);
        assert!(twice.is_err_and(|e| e.contains("twice")), "a repeated guard flag is an error");
        let ci = parse_strs(&["ci", "--workspace"]);
        assert!(ci.is_err_and(|e| e.contains("--workspace")), "ci passes nothing to cargo");
    }

    #[test]
    fn slots_has_four_forms() {
        assert_eq!(parse_strs(&["slots"]), Ok(Command::Slots(SlotsCommand::List)), "list");
        assert_eq!(
            parse_strs(&["slots", "--kill-stale"]),
            Ok(Command::Slots(SlotsCommand::KillStale)),
            "kill-stale"
        );
        assert_eq!(
            parse_strs(&["slots", "--baseline", "--hours", "8"]),
            Ok(Command::Slots(SlotsCommand::Baseline { hours: Some(8) })),
            "a baseline for eight hours"
        );
        assert_eq!(
            parse_strs(&["slots", "--pressure", "2"]),
            Ok(Command::Slots(SlotsCommand::Pressure { gb: 2 })),
            "two GB of pressure"
        );
        let both = parse_strs(&["slots", "--kill-stale", "--baseline"]);
        assert!(both.is_err(), "one form at a time");
    }

    #[test]
    fn desktop_lock_needs_hold() {
        assert_eq!(
            parse_strs(&["desktop-lock", "--hold", "--wait-max", "2"]),
            Ok(Command::DesktopLock(DesktopLockOptions { wait_max: Duration::from_secs(120) })),
            "hold with a wait"
        );
        assert!(parse_strs(&["desktop-lock"]).is_err(), "--hold is required");
        assert!(parse_strs(&["desktop-lock", "--hold", "--exclusive"]).is_err(), "no slots");
    }

    #[test]
    fn every_command_section_4b5_names_parses() {
        for name in COMMANDS {
            let parsed = match *name {
                "pr-meta" => parse_strs(&[name, "--body-file", "b.md"]),
                "desktop-lock" => parse_strs(&[name, "--hold"]),
                _ => parse_strs(&[name]),
            };
            assert!(parsed.is_ok(), "`cargo xtask {name}` parses: {parsed:?}");
        }
        let later = parse_strs(&["codegen"]);
        assert_eq!(
            later,
            Ok(Command::Later { name: "codegen", when: "WP0 phase B" }),
            "a later command says when it arrives"
        );
    }

    #[test]
    fn later_commands_are_all_in_the_command_list() {
        for (name, _) in LATER {
            assert!(COMMANDS.contains(name), "`{name}` is a section 4b.5 command");
        }
    }
}

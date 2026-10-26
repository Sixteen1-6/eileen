//! The guarded cargo commands: `check`, `test`, `clippy`, `build`, `insta`, `ci`, `pre-push` and
//! `sim`, each run under one build slot (BUILD.md sections 4b.3, 4b.5 and 6.1).
//!
//! On Windows, `test` runs the `desktop` test group separately and under `desktop.lock`, with
//! the filter `.config/nextest.toml` gives that group (test
//! `the_desktop_filter_is_nextests`).

use std::io::{self, Write};
use std::process::{Command, ExitCode};

use crate::cli::{CargoCommand, CargoTool, GuardOptions, PrePushOptions, SimOptions};
use crate::guard::{self, Ran, Step};

/// The filter of the `desktop` test group in `.config/nextest.toml`.
pub(crate) const DESKTOP_FILTER: &str = "test(/::desktop::/) | package(eileen-desktop-win) | \
     package(eileen-apps-win) | package(eileen-shell-win) | package(eileen-hud-win) | \
     package(eileen-e2e) | test(/::e2e::/)";

/// Files whose change can affect every crate, so `pre-push` tests the whole workspace.
const WORKSPACE_FILES: [&str; 7] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "clippy.toml",
    "rustfmt.toml",
    ".cargo/",
    ".config/",
];

/// `cargo xtask check|test|clippy|build|insta`.
pub(crate) fn cargo(command: &CargoCommand, out: &mut dyn Write) -> io::Result<ExitCode> {
    let mut options = command.guard;
    // Release builds take both slots (section 6.1 rules 2 and 9), however they are spelled.
    if is_release(&command.args) {
        options.exclusive = true;
    }
    let steps = cargo_steps(command.tool, &command.args, cfg!(windows));
    let name = format!("cargo xtask {}", command_name(command.tool));
    let ran = guard::run(&command_line(&name, &command.args), options, steps, false, out)?;
    if command.tool == (CargoTool::Test { owner: false }) && ran != Ran::NoSlot {
        // Section 4.7: say what the profile's filter left out; nextest counts those as skipped.
        writeln!(
            out,
            "xtask: the profile's filter leaves out the `owner_pc::` and `real_claude::` tests, \
             counted as skipped above; `cargo xtask test --owner` runs them"
        )?;
    }
    finish(&ran, out)
}

/// Whether `args` ask cargo for the release profile: `--release`, `-r` or `--profile release`.
fn is_release(args: &[String]) -> bool {
    args.iter().enumerate().any(|(at, arg)| {
        arg == "--release"
            || arg == "-r"
            || arg == "--profile=release"
            || (arg == "--profile" && args.get(at + 1).is_some_and(|next| next == "release"))
    })
}

/// The steps one cargo tool runs, with `args` from the command line; `windows` splits the
/// desktop test group off.
pub(crate) fn cargo_steps(tool: CargoTool, args: &[String], windows: bool) -> Vec<Step> {
    let or_workspace = |defaults: &[&str]| -> Vec<String> {
        if args.is_empty() {
            defaults.iter().map(|&d| d.to_owned()).collect()
        } else {
            args.to_vec()
        }
    };
    match tool {
        CargoTool::Check => {
            vec![Step::cargo(
                "check",
                with(&["check"], &or_workspace(&["--workspace", "--all-targets"])),
            )]
        }
        CargoTool::Clippy => vec![Step::cargo(
            "clippy",
            with(&["clippy"], &or_workspace(&["--workspace", "--all-targets"])),
        )],
        CargoTool::Build => vec![Step::cargo("build", with(&["build"], args))],
        CargoTool::Insta => {
            // `cargo insta test` runs every test, the desktop group's included, so on Windows it
            // holds `desktop.lock` throughout (section 6.1 rule 12).
            let mut step =
                Step::cargo("insta", with(&["insta", "test", "--unreferenced", "reject"], args));
            step.desktop = windows;
            vec![step]
        }
        CargoTool::Test { owner } => {
            let mut base = vec!["nextest".to_owned(), "run".to_owned()];
            if owner && !args.iter().any(|a| a == "--profile" || a.starts_with("--profile=")) {
                base.extend(["--profile".to_owned(), "owner".to_owned()]);
            }
            base.extend(or_workspace(&["--workspace"]));
            nextest_steps("nextest", base, args, windows)
        }
    }
}

/// `base` as one nextest step, or on Windows as two: everything but the desktop group, then
/// the desktop group under `desktop.lock`. A run that names its own filter stays one step and
/// takes `desktop.lock` for all of it, since the split cannot know what the filter selects.
fn nextest_steps(label: &str, base: Vec<String>, args: &[String], windows: bool) -> Vec<Step> {
    if !windows {
        return vec![Step::cargo(label, base)];
    }
    let filtered = args.iter().any(|a| a == "-E" || a.starts_with("--filterset") || a == "--");
    if filtered {
        let mut step = Step::cargo(label, base);
        step.desktop = true;
        return vec![step];
    }
    let mut rest = base.clone();
    rest.extend(["-E".to_owned(), format!("not ({DESKTOP_FILTER})")]);
    let mut desktop = base;
    desktop.extend(["-E".to_owned(), DESKTOP_FILTER.to_owned(), "--no-tests=pass".to_owned()]);
    let mut desktop_step = Step::cargo(&format!("{label} (desktop group)"), desktop);
    desktop_step.desktop = true;
    vec![Step::cargo(label, rest), desktop_step]
}

fn with(first: &[&str], rest: &[String]) -> Vec<String> {
    first.iter().map(|&s| s.to_owned()).chain(rest.iter().cloned()).collect()
}

const fn command_name(tool: CargoTool) -> &'static str {
    match tool {
        CargoTool::Check => "check",
        CargoTool::Test { .. } => "test",
        CargoTool::Clippy => "clippy",
        CargoTool::Build => "build",
        CargoTool::Insta => "insta",
    }
}

/// The command as the holder record names it.
fn command_line(name: &str, args: &[String]) -> String {
    if args.is_empty() { name.to_owned() } else { format!("{name} {}", args.join(" ")) }
}

/// Prints the summary of a guarded command and returns its exit code: 0 when every step
/// passed, 1 when one failed, 75 without a slot.
fn finish(ran: &Ran, out: &mut dyn Write) -> io::Result<ExitCode> {
    match ran {
        Ran::NoSlot => Ok(crate::machine::exit_code(eileen_buildguard::EXIT_NO_SLOT)),
        Ran::Steps(steps) => {
            if steps.len() > 1 {
                for (label, passed) in steps {
                    writeln!(out, "xtask: {}: {label}", if *passed { "passed" } else { "FAILED" })?;
                }
            }
            Ok(if ran.passed() { ExitCode::SUCCESS } else { ExitCode::from(crate::EXIT_FINDINGS) })
        }
    }
}

/// Whether `cargo <tool>` is installed.
fn has_cargo_tool(tool: &str) -> bool {
    Command::new("cargo")
        .args([tool, "--version"])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The steps of the pull request lane that `cargo xtask ci` runs locally, in the lanes'
/// order, with what it skipped and why.
pub(crate) fn ci_steps(
    windows: bool,
    installed: &dyn Fn(&str) -> bool,
) -> (Vec<Step>, Vec<String>) {
    let mut steps = vec![
        Step::cargo("fmt", ["fmt", "--all", "--check"]),
        Step::cargo(
            "clippy",
            ["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
        ),
    ];
    let mut skipped = Vec::new();
    if installed("nextest") {
        let base = with(&["nextest", "run", "--workspace", "--locked", "--profile", "ci"], &[]);
        steps.extend(nextest_steps("nextest", base, &[], windows));
    } else {
        skipped.push("nextest: cargo-nextest is not installed (`cargo xtask setup`)".to_owned());
    }
    steps.push(Step::cargo("doc tests", ["test", "--workspace", "--doc", "--locked"]));
    let mut docs = Step::cargo("docs", ["doc", "--workspace", "--no-deps", "--locked"]);
    docs.invocation = docs.invocation.with_env([("RUSTDOCFLAGS", "-D warnings")]);
    steps.push(docs);
    for (tool, step) in [
        ("deny", Step::cargo("cargo-deny", ["deny", "--locked", "check"])),
        ("machete", Step::cargo("cargo-machete", ["machete"])),
    ] {
        if installed(tool) {
            steps.push(step);
        } else {
            skipped.push(format!("cargo-{tool}: not installed (`cargo xtask setup`)"));
        }
    }
    steps.push(Step::cargo(
        "simulator seeds",
        ["run", "--locked", "-p", "eileen-sim", "--bin", "eileen-sim", "--", "--doublecheck"],
    ));
    if windows {
        skipped.push(
            "the cfg(not(windows)) compilation and cargo fuzz build: CI's Linux lane runs them"
                .to_owned(),
        );
    }
    skipped.push(
        "cargo mutants --in-diff: never run locally, it is heavy on this PC (CI's advisory lane)"
            .to_owned(),
    );
    skipped.push(
        "codegen --check, typos, Markdown lint, internal links, zizmor and scenario validation: \
         they join with WP0 phase B"
            .to_owned(),
    );
    (steps, skipped)
}

/// `cargo xtask ci`: what the pull request lane runs, through the guard, then `lint-repo`, then
/// the CI-only steps it skipped and why.
pub(crate) fn ci(guard_options: GuardOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    let (steps, skipped) = ci_steps(cfg!(windows), &has_cargo_tool);
    let ran = guard::run("cargo xtask ci", guard_options, steps, true, out)?;
    if ran == Ran::NoSlot {
        return finish(&ran, out);
    }
    let lint = crate::lint_repo::main(&crate::repo_root(), out)?;
    let lint_passed = lint == ExitCode::SUCCESS;
    let code = finish(&ran, out)?;
    writeln!(out, "xtask: {}: lint-repo", if lint_passed { "passed" } else { "FAILED" })?;
    for reason in &skipped {
        writeln!(out, "xtask: skipped: {reason}")?;
    }
    Ok(if ran.passed() && lint_passed { code } else { ExitCode::from(crate::EXIT_FINDINGS) })
}

/// The packages the changed files touch, or `None` when a changed file affects every crate.
/// Files outside any member (documentation, workflows) touch no package.
pub(crate) fn changed_packages(files: &[String]) -> Option<Vec<String>> {
    let mut packages = Vec::new();
    for file in files {
        if WORKSPACE_FILES.iter().any(|w| file == w || (w.ends_with('/') && file.starts_with(w))) {
            return None;
        }
        let package = if let Some(rest) = file.strip_prefix("crates/") {
            rest.split('/').next().filter(|_| rest.contains('/'))
        } else if file.starts_with("xtask/") {
            Some("xtask")
        } else {
            None
        };
        if let Some(package) = package
            && !packages.iter().any(|p| p == package)
        {
            packages.push(package.to_owned());
        }
    }
    packages.sort();
    Some(packages)
}

/// The files changed between the merge base of `base` and `HEAD`, and `HEAD`.
fn changed_files(base: &str) -> io::Result<Vec<String>> {
    let mut git = Command::new("git");
    git.arg("-C").arg(crate::repo_root()).args(["diff", "--name-only", &format!("{base}...HEAD")]);
    for var in crate::repo::GIT_LOCATION_ENV {
        git.env_remove(var);
    }
    let output = git.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git diff --name-only {base}...HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
}

/// `cargo xtask pre-push`: fmt, clippy with warnings denied, the changed crates' tests and
/// `lint-repo`, stopping at the first failure. `.githooks/pre-push` runs it.
pub(crate) fn pre_push(options: &PrePushOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    let files = changed_files(&options.base)?;
    let mut steps = vec![
        Step::cargo("fmt", ["fmt", "--all", "--check"]),
        Step::cargo("clippy", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
    ];
    match changed_packages(&files) {
        None => steps.extend(nextest_steps(
            "nextest",
            with(&["nextest", "run", "--workspace"], &[]),
            &[],
            cfg!(windows),
        )),
        Some(packages) if packages.is_empty() => {
            writeln!(out, "pre-push: no crate changed since {}, so no tests", options.base)?;
        }
        Some(packages) => {
            let mut args = vec!["nextest".to_owned(), "run".to_owned()];
            for package in &packages {
                args.extend(["-p".to_owned(), package.clone()]);
            }
            let selection: Vec<String> = args.iter().skip(2).cloned().collect();
            steps.extend(nextest_steps("nextest", args, &selection, cfg!(windows)));
        }
    }
    let ran = guard::run("cargo xtask pre-push", options.guard, steps, false, out)?;
    if !ran.passed() {
        return finish(&ran, out);
    }
    // Phase A: CI does not run `lint-repo` yet and `main` still has findings in WP10's crates
    // (docs/wp/WP0-foundation.md), so the hook reports them without blocking the push. It blocks
    // once phase B adds `lint-repo` to CI.
    if crate::lint_repo::main(&crate::repo_root(), out)? != ExitCode::SUCCESS {
        writeln!(
            out,
            "xtask: lint-repo found problems (above); the push goes on, since CI does not run \
             lint-repo until WP0 phase B"
        )?;
    }
    finish(&ran, out)
}

/// The commit `HEAD` names, when git can say.
fn head_commit() -> Option<String> {
    let output =
        Command::new("git").args(["rev-parse", "HEAD"]).current_dir(crate::repo_root()).output();
    output
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// `cargo xtask sim`: the simulator's binary through the guard. No arguments runs the pull
/// request seed set.
pub(crate) fn sim(options: &SimOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    // `cargo run` already ends its own arguments with `--`, so a `--` typed after `sim` is not
    // passed on as a simulator argument.
    let mut rest = options.args.clone();
    if let Some(at) = rest.iter().position(|a| a == "--") {
        rest.remove(at);
    }
    let args = with(&["run", "--locked", "-p", "eileen-sim", "--bin", "eileen-sim", "--"], &rest);
    let mut step = Step::cargo("eileen-sim", args);
    if let Some(commit) = head_commit() {
        // A failure prints its replay command with this commit (section 4.7).
        step.invocation = step.invocation.with_env([("EILEEN_SIM_COMMIT", commit)]);
    }
    let steps = vec![step];
    finish(
        &guard::run(
            &command_line("cargo xtask sim", &options.args),
            options.guard,
            steps,
            false,
            out,
        )?,
        out,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_spelling_of_a_release_build_takes_both_slots() {
        let args = |list: &[&str]| list.iter().map(|&a| a.to_owned()).collect::<Vec<_>>();
        for release in
            [&["--release"][..], &["-r"], &["--profile", "release"], &["--profile=release"]]
        {
            assert!(is_release(&args(release)), "{release:?} is a release build");
        }
        for other in [&["--profile", "ci"][..], &["-p", "release"], &["--timings"], &[]] {
            assert!(!is_release(&args(other)), "{other:?} is not a release build");
        }
    }

    use super::*;

    fn args_of(step: &Step) -> Vec<String> {
        step.invocation.args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn the_desktop_filter_is_nextests() {
        let text =
            std::fs::read_to_string(crate::repo_root().join(".config/nextest.toml")).unwrap();
        let config: toml::Table = toml::from_str(&text).unwrap();
        let overrides = config["profile"]["default"]["overrides"].as_array().unwrap();
        let desktop = overrides
            .iter()
            .find(|o| o.get("test-group").and_then(toml::Value::as_str) == Some("desktop"))
            .expect("the desktop group's override");
        assert_eq!(
            desktop["filter"].as_str(),
            Some(DESKTOP_FILTER),
            "xtask splits on the same filter"
        );
    }

    #[test]
    fn a_bare_command_covers_the_workspace_and_arguments_replace_the_default() {
        let check = cargo_steps(CargoTool::Check, &[], false);
        assert_eq!(args_of(&check[0]), ["check", "--workspace", "--all-targets"], "the default");
        let one =
            cargo_steps(CargoTool::Clippy, &["-p".to_owned(), "eileen-core".to_owned()], false);
        assert_eq!(args_of(&one[0]), ["clippy", "-p", "eileen-core"], "the arguments given");
        let insta = cargo_steps(CargoTool::Insta, &[], false);
        assert_eq!(args_of(&insta[0]), ["insta", "test", "--unreferenced", "reject"], "insta");
    }

    #[test]
    fn tests_on_windows_run_the_desktop_group_apart_and_under_its_lock() {
        let steps = cargo_steps(CargoTool::Test { owner: true }, &[], true);
        assert_eq!(steps.len(), 2, "the rest, then the desktop group");
        let rest = args_of(&steps[0]);
        assert!(
            rest.starts_with(&[
                "nextest".to_owned(),
                "run".to_owned(),
                "--profile".to_owned(),
                "owner".to_owned()
            ]),
            "{rest:?}"
        );
        assert!(
            rest.contains(&format!("not ({DESKTOP_FILTER})")),
            "everything but the group: {rest:?}"
        );
        assert!(!steps[0].desktop, "the rest takes no desktop lock");
        assert!(steps[1].desktop, "the group takes desktop.lock");
        assert!(
            args_of(&steps[1]).contains(&"--no-tests=pass".to_owned()),
            "an empty group passes"
        );
        let linux = cargo_steps(CargoTool::Test { owner: false }, &[], false);
        assert_eq!(args_of(&linux[0]), ["nextest", "run", "--workspace"], "one step off Windows");
        let filtered = cargo_steps(
            CargoTool::Test { owner: false },
            &["-E".to_owned(), "test(x)".to_owned()],
            true,
        );
        assert_eq!(filtered.len(), 1, "a filtered run stays whole");
        assert!(filtered[0].desktop, "and takes the lock for all of it");
    }

    #[test]
    fn ci_runs_the_lanes_steps_and_says_what_it_skipped() {
        let (steps, skipped) = ci_steps(true, &|_| true);
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "fmt",
                "clippy",
                "nextest",
                "nextest (desktop group)",
                "doc tests",
                "docs",
                "cargo-deny",
                "cargo-machete",
                "simulator seeds"
            ],
            "the pull request lane's steps"
        );
        assert!(
            skipped.iter().any(|s| s.contains("cfg(not(windows))")),
            "Windows skips the Linux compile"
        );
        assert!(skipped.iter().any(|s| s.contains("mutants")), "mutants never run locally");
        let (_, without_tools) = ci_steps(false, &|_| false);
        assert!(without_tools.iter().any(|s| s.contains("cargo-deny")), "a missing tool is named");
    }

    #[test]
    fn pre_push_tests_the_changed_crates_or_everything() {
        let files = |list: &[&str]| -> Vec<String> { list.iter().map(|&f| f.to_owned()).collect() };
        assert_eq!(
            changed_packages(&files(&[
                "crates/eileen-core/src/lib.rs",
                "xtask/src/cli.rs",
                "docs/BUILD.md"
            ])),
            Some(vec!["eileen-core".to_owned(), "xtask".to_owned()]),
            "the two crates, not the docs"
        );
        assert_eq!(
            changed_packages(&files(&["Cargo.lock"])),
            None,
            "the lockfile touches every crate"
        );
        assert_eq!(changed_packages(&files(&[".config/nextest.toml"])), None, "so does .config/");
        assert_eq!(changed_packages(&files(&["README.md"])), Some(Vec::new()), "docs touch none");
    }
}

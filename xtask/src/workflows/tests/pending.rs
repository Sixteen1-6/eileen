//! The pending-snapshot route's rules (AGENTS.md, Testing): every job that runs tests keeps
//! insta's pending snapshots out of the tree and in the artifact of a failed run, one failed run
//! writes every changed one, and `hygiene` rejects one committed by hand. Each rule is a
//! function over a workflow's text, shown failing on a fixture and holding on `ci.yml`.

use super::{CI, has_key, indent_of, job, jobs, steps, try_job};

const NEXTEST: &str = include_str!("../../../../.config/nextest.toml");

/// A job's own keys, such as `if:` or `env:`, trimmed, without its steps or comments.
fn job_keys<'a>(job: &[&'a str]) -> Vec<&'a str> {
    job.iter()
        .copied()
        .filter(|line| indent_of(line) == 4)
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect()
}

/// The `NAME: value` lines of a job's own `env:`, trimmed, without comments.
fn job_env<'a>(job: &[&'a str]) -> Vec<&'a str> {
    let Some(at) = job.iter().position(|line| indent_of(line) == 4 && line.trim() == "env:") else {
        return Vec::new();
    };
    job[at + 1..]
        .iter()
        .copied()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .take_while(|line| indent_of(line) > 4)
        .map(str::trim)
        .collect()
}

/// The shell commands a step runs, as `steps` gives it, each with its words joined by one
/// space. A folded block (`run: >` or `run: >-`) is one line, as YAML folds it; a line of any
/// other run that ends in `\` goes on with the next; each line is split at `|`, `;` and `&`,
/// so a `--no-run` counts only in the command it belongs to. The step's name is left out, and
/// its other keys come along as commands that run nothing.
fn commands(step: &str) -> Vec<String> {
    let lines: Vec<&str> = step
        .lines()
        .map(|line| line.trim_start_matches("- "))
        .filter(|line| !line.starts_with("name: "))
        .collect();
    let folded = lines.iter().any(|line| matches!(*line, "run: >" | "run: >-"));
    let mut joined: Vec<String> = Vec::new();
    let mut open = false;
    for line in lines {
        let text = line.strip_suffix('\\').unwrap_or(line);
        match joined.last_mut().filter(|_| open || folded) {
            Some(last) => {
                last.push(' ');
                last.push_str(text);
            }
            None => joined.push(text.to_owned()),
        }
        open = line.ends_with('\\');
    }
    joined
        .iter()
        .flat_map(|line| line.split(['|', ';', '&']))
        .map(|command| command.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|command| !command.is_empty())
        .collect()
}

/// Whether one command runs tests: `cargo` with `test` or `nextest` among its later words
/// (`cargo test`, `cargo +nightly test`, `cargo xtask test`, `cargo llvm-cov nextest`), and
/// no build-only `--no-run`.
fn command_runs_tests(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    let Some(cargo) = words.iter().position(|word| *word == "cargo") else {
        return false;
    };
    !words.contains(&"--no-run")
        && words[cargo..].iter().any(|word| matches!(*word, "test" | "nextest"))
}

/// Whether a step, as `steps` gives it, runs tests in any of its commands.
fn runs_tests(step: &str) -> bool {
    commands(step).iter().any(|command| command_runs_tests(command))
}

/// The jobs of a workflow that have a step that runs tests.
fn test_jobs(workflow: &str) -> Vec<&str> {
    jobs(workflow)
        .into_iter()
        .filter(|name| steps(&job(workflow, name)).iter().any(|step| runs_tests(step)))
        .collect()
}

/// What every step of a test job inherits from the job's `env:`. With `INSTA_UPDATE=new`,
/// insta writes a failing or new snapshot as `.snap.new`, and an inline one as
/// `.<file>.pending-snap`, a hidden file, even when it detects CI. `INSTA_PENDING_DIR` puts
/// both under `target/`, at their repository paths, so none lands in the tree.
const PENDING_ENV: [&str; 2] =
    ["INSTA_UPDATE: new", "INSTA_PENDING_DIR: ${{ github.workspace }}/target/insta-pending"];
/// The lines of the upload that keeps the pending folder when a test job fails.
const UPLOAD_LINES: [&str; 3] =
    ["if: failure()", "target/insta-pending/", "include-hidden-files: true"];
/// The line of `hygiene`'s step that lists the pending files in the tree.
const HYGIENE_LISTING: &str = "if git ls-files -- '*.snap.new' '*.pending-snap' | grep .; then";
/// The problem when `hygiene` has no step that lists the pending files.
const NO_LISTING: &str = "hygiene: no step rejects a committed pending snapshot";
/// The problem when that step lists them but might not fail the job.
const NOT_THE_LISTING: &str = "hygiene: the pending-snapshot step is not `run: |`, the \
                               listing, an `::error::` echo, `exit 1` and `fi`";

/// Every rule a workflow breaks that keeps insta's pending snapshots from a failed run, one
/// line each, naming the job; empty when the workflow keeps them.
fn pending_snapshot_problems(workflow: &str) -> Vec<String> {
    let mut problems: Vec<String> = test_jobs(workflow)
        .into_iter()
        .flat_map(|name| test_job_problems(name, &job(workflow, name)))
        .collect();
    problems.extend(hygiene_problems(workflow));
    problems
}

/// A test job's rules: its own `env:` sets both variables, so every step has them however it
/// is written, and no step sets them again over the job's; an upload after the last test step
/// keeps the pending folder on failure, hidden files included.
fn test_job_problems(name: &str, job: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    let env = job_env(job);
    for line in PENDING_ENV {
        if !env.contains(&line) {
            problems.push(format!("{name}: the job's env lacks `{line}`"));
        }
    }
    let steps = steps(job);
    for step in &steps {
        if step.contains("INSTA_UPDATE") || step.contains("INSTA_PENDING_DIR") {
            let first = step.lines().next().unwrap_or_default();
            problems.push(format!("{name}: `{first}` sets insta's variables over the job's"));
        }
    }
    let uploads: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| has_key(step, "uses: actions/upload-artifact@"))
        .map(|(at, _)| at)
        .collect();
    let Some(&upload) = uploads
        .iter()
        .find(|at| steps[**at].contains("target/insta-pending/"))
        .or_else(|| uploads.last())
    else {
        problems.push(format!("{name}: no upload keeps what a failed run leaves"));
        return problems;
    };
    if steps.iter().rposition(|step| runs_tests(step)).is_some_and(|last| upload < last) {
        problems.push(format!(
            "{name}: the upload comes before a test step, so it misses what that step leaves"
        ));
    }
    for line in UPLOAD_LINES {
        if !steps[upload].lines().any(|held| held == line) {
            problems.push(format!("{name}: the upload lacks `{line}`"));
        }
    }
    problems
}

/// `hygiene`'s rules: the job has no `if:` or `continue-on-error`, and one step lists the
/// pending files in the tree and fails on any. An `if:`, `continue-on-error`, `|| true`, a
/// dropped `exit 1` or a turned-around test would each let a committed one through, so that
/// step's lines are fixed but for its name and its message.
fn hygiene_problems(workflow: &str) -> Vec<String> {
    let Some(job) = try_job(workflow, "hygiene") else {
        return vec!["the workflow has no `hygiene` job".to_owned()];
    };
    let mut problems = Vec::new();
    for key in ["if:", "continue-on-error:"] {
        if job_keys(&job).iter().any(|line| line.starts_with(key)) {
            problems.push(format!("hygiene: the job has `{key}`, so it may skip or pass"));
        }
    }
    let steps = steps(&job);
    match steps.iter().find(|step| step.contains("git ls-files -- '*.snap.new'")) {
        None => problems.push(NO_LISTING.to_owned()),
        Some(step) if !fails_on_a_listed_file(step) => problems.push(NOT_THE_LISTING.to_owned()),
        Some(_) => {}
    }
    problems
}

/// Whether a step is exactly the listing that fails the job, whatever its name and message.
fn fails_on_a_listed_file(step: &str) -> bool {
    let lines: Vec<&str> = step
        .lines()
        .map(|line| line.trim_start_matches("- "))
        .filter(|line| !line.starts_with("name: "))
        .collect();
    matches!(
        lines.as_slice(),
        ["run: |", listing, echo, "exit 1", "fi"]
            if *listing == HYGIENE_LISTING && echo.starts_with("echo \"::error::")
    )
}

/// Steps in every form `runs_tests` reads, and steps that run no test.
const RUN_FORMS: &str = r"name: example
jobs:
  inline:
    steps:
      - name: Tests
        run: cargo nextest run --workspace
  folded:
    steps:
      - name: Build the tests
        run: cargo nextest run --workspace --no-run
      - name: The tests, on two lines
        run: >-
          cargo nextest run --workspace
          --locked --profile ci
      - name: A build, its flag on the second line
        run: >-
          cargo nextest run --workspace
          --no-run
  literal:
    steps:
      - name: Doc tests
        run: |
          echo first
          cargo test --workspace --doc
      - name: A build, then the tests
        run: |
          cargo nextest run --no-run
          cargo nextest run --profile ci
      - name: A build, its flag after an escaped line break
        run: |
          cargo nextest run --workspace \
            --no-run
  chained:
    steps:
      - name: A build and the tests on one line
        run: cargo nextest run --no-run && cargo nextest run --profile ci
  other-forms:
    steps:
      - name: A pinned toolchain
        run: cargo +nightly test -p eileen-core
      - name: Through xtask
        run: cargo xtask test -p eileen-core
      - name: Coverage
        run: cargo llvm-cov nextest --workspace
      - name: Every feature set
        run: cargo hack test --each-feature
  elsewhere:
    steps:
      - name: Lint, as cargo test would build it
        # cargo test is only named in this comment
        run: cargo clippy --workspace --all-targets
      - uses: taiki-e/install-action@v0
        with:
          tool: cargo-nextest@0.9.148
";

#[test]
fn a_test_step_is_found_in_every_run_form() {
    let found: Vec<(&str, Vec<bool>)> = jobs(RUN_FORMS)
        .into_iter()
        .map(|name| {
            let steps = steps(&job(RUN_FORMS, name));
            (name, steps.iter().map(|step| runs_tests(step)).collect())
        })
        .collect();
    assert_eq!(
        found,
        [
            ("inline", vec![true]),
            ("folded", vec![false, true, false]),
            ("literal", vec![true, true, false]),
            ("chained", vec![true]),
            ("other-forms", vec![true, true, true, true]),
            ("elsewhere", vec![false, false]),
        ],
        "the steps that run tests, by job"
    );
}

/// A workflow that keeps pending snapshots; each case of `BROKEN_RULES` breaks it once.
const KEEPS_PENDING: &str = r#"name: example
on: pull_request
jobs:
  hygiene:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v0
      - name: No pending snapshot committed
        # A comment is not part of the step.
        run: |
          if git ls-files -- '*.snap.new' '*.pending-snap' | grep .; then
            echo "::error::Remove the pending snapshot files listed above."
            exit 1
          fi
  lint:
    runs-on: ubuntu-latest
    steps:
      - name: Clippy, which builds the tests and runs none
        run: cargo clippy --workspace --all-targets
  tests:
    runs-on: ubuntu-latest
    env:
      # Every step has them.
      INSTA_UPDATE: new
      INSTA_PENDING_DIR: ${{ github.workspace }}/target/insta-pending
    steps:
      - uses: actions/checkout@v0
      - name: Tests
        run: cargo nextest run --workspace --profile ci
      - name: Doc tests
        run: cargo test --workspace --doc
      - name: Keep the pending snapshots
        if: failure()
        uses: actions/upload-artifact@v0
        with:
          name: tests-junit
          path: |
            target/nextest/ci/junit.xml
            target/insta-pending/
          include-hidden-files: true
"#;

/// One edit to `KEEPS_PENDING`: a text found there once, and what replaces it.
type Edit = (&'static str, &'static str);

/// Each case: what it breaks, its edits to `KEEPS_PENDING`, and the one problem it gives.
const BROKEN_RULES: [(&str, &[Edit], &str); 15] = [
    (
        "the job's env without the folder",
        &[("      INSTA_PENDING_DIR: ${{ github.workspace }}/target/insta-pending\n", "")],
        concat!(
            "tests: the job's env lacks ",
            "`INSTA_PENDING_DIR: ${{ github.workspace }}/target/insta-pending`",
        ),
    ),
    (
        "insta told to write nothing",
        &[("INSTA_UPDATE: new", "INSTA_UPDATE: no")],
        "tests: the job's env lacks `INSTA_UPDATE: new`",
    ),
    (
        "a step setting a variable over the job's",
        &[(
            "        run: cargo test --workspace --doc\n",
            concat!(
                "        env:\n",
                "          INSTA_UPDATE: always\n",
                "        run: cargo test --workspace --doc\n",
            ),
        )],
        "tests: `- name: Doc tests` sets insta's variables over the job's",
    ),
    (
        "an upload without hidden files",
        &[("          include-hidden-files: true\n", "")],
        "tests: the upload lacks `include-hidden-files: true`",
    ),
    (
        "an upload on every run",
        &[("        if: failure()\n", "")],
        "tests: the upload lacks `if: failure()`",
    ),
    (
        "an upload without the pending folder",
        &[("            target/insta-pending/\n", "")],
        "tests: the upload lacks `target/insta-pending/`",
    ),
    (
        "a test step after the upload",
        &[
            ("      - name: Doc tests\n        run: cargo test --workspace --doc\n", ""),
            (
                "          include-hidden-files: true\n",
                concat!(
                    "          include-hidden-files: true\n",
                    "      - name: Doc tests\n",
                    "        run: cargo test --workspace --doc\n",
                ),
            ),
        ],
        "tests: the upload comes before a test step, so it misses what that step leaves",
    ),
    (
        "no upload",
        &[(
            concat!(
                "      - name: Keep the pending snapshots\n",
                "        if: failure()\n",
                "        uses: actions/upload-artifact@v0\n",
                "        with:\n",
                "          name: tests-junit\n",
                "          path: |\n",
                "            target/nextest/ci/junit.xml\n",
                "            target/insta-pending/\n",
                "          include-hidden-files: true\n",
            ),
            "",
        )],
        "tests: no upload keeps what a failed run leaves",
    ),
    (
        "hygiene without the step",
        &[(
            concat!(
                "      - name: No pending snapshot committed\n",
                "        # A comment is not part of the step.\n",
                "        run: |\n",
                "          if git ls-files -- '*.snap.new' '*.pending-snap' | grep .; then\n",
                "            echo \"::error::Remove the pending snapshot files listed above.\"\n",
                "            exit 1\n",
                "          fi\n",
            ),
            "",
        )],
        NO_LISTING,
    ),
    ("a listing that cannot fail", &[("            exit 1\n", "")], NOT_THE_LISTING),
    (
        "a listing whose failure is ignored",
        &[("| grep .; then", "| grep . || true; then")],
        NOT_THE_LISTING,
    ),
    (
        "a listing allowed to fail",
        &[(
            "        run: |\n          if git",
            "        continue-on-error: true\n        run: |\n          if git",
        )],
        NOT_THE_LISTING,
    ),
    ("a listing turned around", &[("if git ls-files", "if ! git ls-files")], NOT_THE_LISTING),
    (
        "a hygiene job that may not run",
        &[("  hygiene:\n    runs-on", "  hygiene:\n    if: false\n    runs-on")],
        "hygiene: the job has `if:`, so it may skip or pass",
    ),
    ("no hygiene job", &[("  hygiene:\n", "  tidy:\n")], "the workflow has no `hygiene` job"),
];

#[test]
fn each_broken_rule_is_named() {
    let kept = pending_snapshot_problems(KEEPS_PENDING);
    assert!(kept.is_empty(), "the fixture keeps pending snapshots: {kept:#?}");
    for (case, edits, problem) in BROKEN_RULES {
        let mut workflow = KEEPS_PENDING.to_owned();
        for &(from, to) in edits {
            assert_eq!(workflow.matches(from).count(), 1, "{case}: `{from}` is there once");
            workflow = workflow.replacen(from, to, 1);
        }
        assert_eq!(pending_snapshot_problems(&workflow), [problem], "{case}");
    }
}

#[test]
fn ci_keeps_pending_snapshots() {
    let problems = pending_snapshot_problems(CI);
    assert!(problems.is_empty(), "ci.yml loses pending snapshots: {problems:#?}");
    let testing = test_jobs(CI);
    for name in ["logic-linux", "windows-test"] {
        assert!(testing.contains(&name), "{name} runs tests: {testing:?}");
    }
}

/// With nextest's `fail-fast`, a run stops at the first failing test, so a pull request that
/// moves several snapshots would need one CI round trip for each.
#[test]
fn one_failed_run_writes_every_changed_snapshot() {
    let config: toml::Table = toml::from_str(NEXTEST).unwrap();
    assert_eq!(
        config["profile"]["ci"].get("fail-fast").and_then(toml::Value::as_bool),
        Some(false),
        "nextest's ci profile runs every test after a failure"
    );
    let mut runs = 0;
    for name in test_jobs(CI) {
        for step in steps(&job(CI, name)) {
            for command in commands(&step) {
                let nextest = command.split_whitespace().any(|word| word == "nextest");
                if nextest && command_runs_tests(&command) {
                    runs += 1;
                    assert!(command.contains("--profile ci"), "{name}: the ci profile: {command}");
                }
            }
        }
    }
    assert!(runs >= 2, "logic-linux and windows-test run nextest: {runs} runs");
}

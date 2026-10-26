//! `cargo xtask pr-meta`: the pull request title, description and changed files, checked as
//! BUILD.md section 4b.3 describes checks 7 and 8 and the fix half of check 19.

mod body;
mod classes;
mod resolve;
mod scan;
mod title;

use std::io::{self, Write};
use std::process::ExitCode;

use crate::cli::PrMetaOptions;
use crate::repo::{Change, ChangeKind, GitRepo, Repo, RepoError};
use body::{Description, Evidence, PrType, Problem};
use classes::Classes;
use resolve::{Resolution, TestSite};

/// The author Dependabot's pull requests carry in the event payload.
const DEPENDABOT: &str = "dependabot[bot]";

/// The description's headings, in the template's order. The one for fixes is required only
/// when the type is `fix`.
const HEADINGS: [&str; 10] = [
    "Evidence",
    "Type",
    "What",
    "Why",
    "Tests at which layer",
    FIX_HEADING,
    "Scenario",
    "Suggested guidance changes",
    "Architectural changes",
    "Playbook",
];
const FIX_HEADING: &str = "Seen failing before the fix";

/// The checks `pr-meta` runs, named by their number in BUILD.md section 4b.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// Check 7: the title is a valid squash subject.
    Title,
    /// Check 8: the description follows the template, its evidence exists and its type fits.
    Description,
    /// Check 19, first half: a fix adds or changes a regression.
    Regression,
}

impl Check {
    const fn number(self) -> u8 {
        match self {
            Self::Title => 7,
            Self::Description => 8,
            Self::Regression => 19,
        }
    }
}

/// Where a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// The pull request title.
    Title,
    /// The description, at a line when one is known.
    Body(Option<usize>),
    /// The changed files.
    Files,
}

/// One problem: the check, where it is, what is wrong and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) check: Check,
    pub(crate) place: Place,
    pub(crate) problem: String,
    pub(crate) fix: String,
}

/// What a run found: problems that fail it, advice that does not, and a one-line summary.
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub(crate) findings: Vec<Finding>,
    pub(crate) advice: Vec<String>,
    pub(crate) summary: String,
}

impl Outcome {
    fn fail(
        &mut self,
        check: Check,
        place: Place,
        problem: impl Into<String>,
        fix: impl Into<String>,
    ) {
        self.findings.push(Finding { check, place, problem: problem.into(), fix: fix.into() });
    }

    fn fail_body(&mut self, problem: Problem) {
        self.fail(Check::Description, Place::Body(problem.line), problem.what, problem.fix);
    }
}

/// The pull request as the event payload gives it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Input<'a> {
    /// The title, or `None` to check the head commit's subject, which is what GitHub proposes
    /// as the title of a one-commit pull request.
    pub(crate) title: Option<&'a str>,
    pub(crate) body: &'a str,
    pub(crate) author: &'a str,
}

/// Runs `cargo xtask pr-meta` and writes its report.
pub(crate) fn main(options: &PrMetaOptions, out: &mut dyn Write) -> io::Result<ExitCode> {
    let body = match std::fs::read_to_string(&options.body_file) {
        Ok(body) => body,
        Err(error) => {
            writeln!(out, "pr-meta: cannot read {}: {error}", options.body_file.display())?;
            return Ok(ExitCode::from(crate::EXIT_ERROR));
        }
    };
    let outcome =
        GitRepo::open(&crate::repo_root(), &options.base, &options.head).and_then(|repo| {
            let input =
                Input { title: options.title.as_deref(), body: &body, author: &options.author };
            let outcome = check(&input, &repo)?;
            Ok((outcome, repo))
        });
    match outcome {
        Ok((outcome, repo)) => {
            let annotate = std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
            let reproduce = reproduce_command(options, repo.base(), repo.head());
            render(&outcome, &options.body_file.display().to_string(), &reproduce, annotate, out)?;
            Ok(if outcome.findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(crate::EXIT_FINDINGS)
            })
        }
        Err(error) => {
            writeln!(out, "pr-meta: could not run: {error}")?;
            Ok(ExitCode::from(crate::EXIT_ERROR))
        }
    }
}

/// Checks a pull request against the repository.
pub(crate) fn check(input: &Input<'_>, repo: &dyn Repo) -> Result<Outcome, RepoError> {
    let changes = repo.changed_files()?;
    let classes = classes::classify(changes.iter().map(|change| change.path.as_str()));
    let mut outcome = Outcome::default();
    if input.author == DEPENDABOT {
        check_deps_files(repo, &changes, "a Dependabot pull request", &mut outcome)?;
        outcome.summary =
            format!("Dependabot pull request, read as type deps; {} changed files", changes.len());
        return Ok(outcome);
    }
    let subjects = repo.commit_subjects()?;
    let (title, title_note) = match (input.title, subjects.first()) {
        (Some(title), _) => (title.to_owned(), format!("title \"{title}\"")),
        (None, Some((sha, subject))) => (
            subject.clone(),
            format!("title \"{subject}\" (the head commit's subject, {sha}; --title sets one)"),
        ),
        (None, None) => {
            return Err(RepoError(
                "no --title was given and the branch has no commits since the base, so there is \
                 no title to check; pass --title <title>"
                    .to_owned(),
            ));
        }
    };
    for (problem, fix) in title::subject_problems(&title) {
        outcome.fail(Check::Title, Place::Title, problem, fix);
    }
    // When the title is the head commit's subject, its problems are findings already.
    let advised = if input.title.is_some() {
        subjects.as_slice()
    } else {
        subjects.get(1..).unwrap_or_default()
    };
    advise_on_commits(advised, &mut outcome);

    let description = Description::parse(input.body);
    let pr_type = description.section("Type").map(body::read_type);
    let is_fix = matches!(pr_type, Some(Ok(PrType::Fix)));
    check_layout(&description, is_fix, &mut outcome);
    let evidence = description.section("Evidence").map(Evidence::read).unwrap_or_default();
    if let Some(section) = description.section("Evidence") {
        check_evidence_filled(&evidence, section.line, &mut outcome);
    }
    let listed = resolve_listed_ids(repo, &evidence, &mut outcome)?;
    let known_type = match pr_type {
        Some(Ok(pr_type)) => {
            check_type_fits(repo, pr_type, &changes, classes, &listed, &mut outcome)?;
            if pr_type == PrType::Fix {
                check_fix(repo, &description, &changes, &listed, &mut outcome)?;
            }
            Some(pr_type)
        }
        Some(Err(problem)) => {
            outcome.fail_body(problem);
            None
        }
        None => None,
    };
    outcome.summary = format!("{title_note}; {}", summary(known_type, &changes, classes, &listed));
    Ok(outcome)
}

/// Check 7 applies to the pull request title; each commit's subject gets the same rule as
/// advice only, since the squash merge keeps only the title.
fn advise_on_commits(subjects: &[(String, String)], outcome: &mut Outcome) {
    for (sha, subject) in subjects {
        for (problem, _) in title::subject_problems(subject) {
            let problem = problem.replacen("the title", "its subject", 1);
            outcome.advice.push(format!("commit {sha} \"{subject}\": {problem}"));
        }
    }
}

/// The template's shape: `## Evidence` first with nothing said before it, `## Type` second,
/// and every heading present once, in order.
fn check_layout(description: &Description, is_fix: bool, outcome: &mut Outcome) {
    let headings: Vec<&str> = description.sections.iter().map(|s| s.heading.as_str()).collect();
    let evidence_first = headings.first() == Some(&"Evidence");
    if headings.contains(&"Evidence") && !evidence_first {
        outcome.fail(
            Check::Description,
            Place::Body(description.sections.first().map(|section| section.line)),
            "the description does not start with `## Evidence`",
            "move `## Evidence` to the top, as the template has it",
        );
    }
    if evidence_first && description.preamble_has_words() {
        outcome.fail(
            Check::Description,
            Place::Body(description.preamble.first().map(|line| line.number)),
            "text comes before `## Evidence`",
            "start the description with `## Evidence`; put that text in a section",
        );
    }
    if evidence_first && headings.contains(&"Type") && headings.get(1) != Some(&"Type") {
        outcome.fail(
            Check::Description,
            Place::Body(description.section("Type").map(|section| section.line)),
            "`## Type` does not come right after `## Evidence`",
            "put `## Type` second, as the template has it",
        );
    }
    let mut previous: Option<usize> = None;
    for heading in HEADINGS {
        let at: Vec<usize> =
            (0..headings.len()).filter(|i| headings.get(*i) == Some(&heading)).collect();
        if heading == FIX_HEADING && !is_fix && at.is_empty() {
            continue;
        }
        let Some(&first) = at.first() else {
            outcome.fail(
                Check::Description,
                Place::Body(None),
                format!("the description has no `## {heading}` section"),
                "fill in the pull request template; keep every heading it has",
            );
            continue;
        };
        if at.len() > 1 {
            outcome.fail(
                Check::Description,
                Place::Body(at.get(1).and_then(|i| description.sections.get(*i)).map(|s| s.line)),
                format!("`## {heading}` appears more than once"),
                "merge the two sections",
            );
        }
        if previous.is_some_and(|before| first < before) {
            outcome.fail(
                Check::Description,
                Place::Body(description.sections.get(first).map(|s| s.line)),
                format!("`## {heading}` is out of the template's order"),
                format!("keep the template's order: {}", HEADINGS.join(", ")),
            );
        }
        previous = Some(first);
    }
}

fn check_evidence_filled(evidence: &Evidence, heading_line: usize, outcome: &mut Outcome) {
    for problem in &evidence.problems {
        outcome.fail_body(problem.clone());
    }
    if evidence.is_empty() {
        outcome.fail(
            Check::Description,
            Place::Body(Some(heading_line)),
            "the Evidence section is empty or still the template's placeholder",
            "say what shows the change is right: a prose line, or a `tests:` or `scenarios:` line \
             with ids that exist",
        );
    }
    for (name, prefix) in [("replay", "rec_"), ("incident", "inc_")] {
        if let Some(field) = evidence.field(name)
            && !is_prefixed_ulid(&field.value, prefix)
        {
            outcome.fail(
                Check::Description,
                Place::Body(Some(field.line)),
                format!("`{name}: {}` is not `{prefix}<ulid>`", field.value),
                format!("write the id as `{prefix}` followed by its 26-character ULID"),
            );
        }
    }
}

fn is_prefixed_ulid(value: &str, prefix: &str) -> bool {
    const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    value.strip_prefix(prefix).is_some_and(|ulid| {
        ulid.chars().count() == 26
            && ulid.chars().all(|c| CROCKFORD.contains(c.to_ascii_uppercase()))
    })
}

/// The ids listed on the `tests:` and `scenarios:` lines and where the found tests live.
#[derive(Debug, Default)]
struct Listed {
    tests: usize,
    scenarios: usize,
    /// Where each listed test that exists is defined, at the head commit.
    test_sites: Vec<TestSite>,
}

impl Listed {
    const fn any(&self) -> bool {
        self.tests + self.scenarios > 0
    }
}

fn split_ids(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|id| !id.is_empty())
}

fn resolve_listed_ids(
    repo: &dyn Repo,
    evidence: &Evidence,
    outcome: &mut Outcome,
) -> Result<Listed, RepoError> {
    let mut listed = Listed::default();
    if let Some(field) = evidence.field("tests") {
        let members = resolve::members(repo)?;
        for id in split_ids(&field.value) {
            listed.tests += 1;
            let resolution = match resolve::parse_test_id(id) {
                Ok(parsed) => resolve::resolve_test(repo, &members, &parsed)?,
                Err(why) => Resolution::Missing(why),
            };
            match resolution {
                Resolution::Found(site) => listed.test_sites.push(site),
                Resolution::Missing(why) => outcome.fail(
                    Check::Description,
                    Place::Body(Some(field.line)),
                    format!("the test `{id}` does not exist at the head commit: {why}"),
                    "name a test that exists, as `<package>::<module>::...::<fn>`",
                ),
            }
        }
    }
    if let Some(field) = evidence.field("scenarios") {
        for id in split_ids(&field.value) {
            listed.scenarios += 1;
            if let Resolution::Missing(why) = resolve::resolve_scenario(repo, id)? {
                outcome.fail(
                    Check::Description,
                    Place::Body(Some(field.line)),
                    format!("the scenario `{id}` does not exist: {why}"),
                    "list scenarios as `<area>/<id>` with a scenarios/<area>/<id>/scenario.toml",
                );
            }
        }
    }
    Ok(listed)
}

/// The type must fit the files: `docs` and `scenario` only for their path classes, `deps` only
/// for manifests, lockfiles and action pins, and the feature rule (at least one id) for
/// feature, fix, refactor and any amendment that changes more than docs.
fn check_type_fits(
    repo: &dyn Repo,
    pr_type: PrType,
    changes: &[Change],
    classes: Classes,
    listed: &Listed,
    outcome: &mut Outcome,
) -> Result<(), RepoError> {
    let mismatch = |class: &str| {
        (
            format!("the type is {} but the changed files are not {class}", pr_type.as_str()),
            "pick the type that fits every changed file (BUILD.md section 4.6 has the classes)",
        )
    };
    let needs_ids = match pr_type {
        PrType::Docs if !classes.docs_only => {
            let (problem, fix) = mismatch("docs-only (Markdown or docs/)");
            outcome.fail(Check::Description, Place::Files, problem, fix);
            false
        }
        PrType::Scenario if !classes.scenarios_only => {
            let (problem, fix) = mismatch("scenarios-only (under scenarios/)");
            outcome.fail(Check::Description, Place::Files, problem, fix);
            false
        }
        PrType::Deps => {
            check_deps_files(repo, changes, "type deps", outcome)?;
            false
        }
        PrType::Amendment => !classes.docs_only,
        PrType::Feature | PrType::Fix | PrType::Refactor => true,
        PrType::Docs | PrType::Scenario => false,
    };
    if needs_ids && !listed.any() {
        outcome.fail(
            Check::Description,
            Place::Body(None),
            format!(
                "a {} pull request that changes code lists no test or scenario",
                pr_type.as_str()
            ),
            "add a `tests:` or `scenarios:` line to Evidence naming what shows the change works",
        );
    }
    Ok(())
}

/// `deps` changes only dependency manifests, lockfiles and the `uses:` lines of workflow and
/// action files. Any other file, an added or deleted workflow, a change with no text line (a
/// binary or mode-only change) and any changed line that is not a `uses:` line fail it.
fn check_deps_files(
    repo: &dyn Repo,
    changes: &[Change],
    what: &str,
    outcome: &mut Outcome,
) -> Result<(), RepoError> {
    let mut offenders = Vec::new();
    for change in changes {
        if classes::is_dependency_manifest(&change.path) {
            continue;
        }
        let why = if !classes::may_hold_pins(&change.path) {
            Some("not a manifest, lockfile, workflow or action file")
        } else if change.kind != ChangeKind::Modified {
            Some("added or deleted")
        } else {
            let diff = repo.file_diff(&change.path)?;
            if diff.lines.is_empty() {
                Some("no text line changed: a binary or mode-only change")
            } else if diff.lines.iter().all(|line| classes::is_pin_line(line)) {
                None
            } else {
                Some("changes lines other than `uses:`")
            }
        };
        if let Some(why) = why {
            offenders.push(format!("{} ({why})", change.path));
        }
    }
    if !offenders.is_empty() {
        outcome.fail(
            Check::Description,
            Place::Files,
            format!(
                "{what} may change only Cargo.toml and Cargo.lock files and the `uses:` lines of \
                 workflows and actions, but it changes {}",
                offenders.join(", ")
            ),
            "split the other changes into their own pull request, or pick the type that fits",
        );
    }
    Ok(())
}

/// A fix adds or changes a regression: a scenario, a fuzz or simulator regression, a file
/// under a `tests/` folder, or a test it lists, where an added or changed line inside the
/// listed test or the test module around it counts. It also says how the failure was seen
/// before the fix.
fn check_fix(
    repo: &dyn Repo,
    description: &Description,
    changes: &[Change],
    listed: &Listed,
    outcome: &mut Outcome,
) -> Result<(), RepoError> {
    let mut has_regression = false;
    for change in changes.iter().filter(|change| change.kind != ChangeKind::Deleted) {
        if is_regression_path(&change.path) {
            has_regression = true;
            break;
        }
        let sites: Vec<&TestSite> =
            listed.test_sites.iter().filter(|site| site.file == change.path).collect();
        if sites.is_empty() {
            continue;
        }
        let diff = repo.file_diff(&change.path)?;
        if diff.added_at.iter().any(|&line| sites.iter().any(|site| site.is_touched_at(line))) {
            has_regression = true;
            break;
        }
    }
    if !has_regression {
        outcome.fail(
            Check::Regression,
            Place::Files,
            "a fix that adds or changes no regression: no scenario, fuzz or simulator \
             regression or file under `tests/` is changed, and no line inside a listed test or \
             the test module around it is added or changed",
            "add or change the test or scenario that failed before the fix, and list it on \
             `tests:` or `scenarios:`",
        );
    }
    if let Some(section) = description.section(FIX_HEADING)
        && !section.has_words()
    {
        outcome.fail(
            Check::Regression,
            Place::Body(Some(section.line)),
            "a fix that does not say how it was seen failing before",
            "say under `## Seen failing before the fix` how the failure was reproduced",
        );
    }
    Ok(())
}

/// A file whose change is a regression by its path: a file inside a scenario folder,
/// `scenarios/<area>/<id>/...` (areas starting with `_`, such as `_schema`, hold no
/// scenarios), a fuzz or simulator regression, or a test file.
fn is_regression_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let scenario =
        matches!(parts.as_slice(), ["scenarios", area, _, _, ..] if !area.starts_with('_'));
    scenario
        || path.starts_with("fuzz/regressions/")
        || path.starts_with("crates/eileen-sim/regressions/")
        || parts.contains(&"tests")
        || path.ends_with("/tests.rs")
}

fn summary(
    pr_type: Option<PrType>,
    changes: &[Change],
    classes: Classes,
    listed: &Listed,
) -> String {
    let class = match (classes.docs_only, classes.scenarios_only) {
        (true, _) => "docs-only",
        (false, true) => "scenarios-only",
        (false, false) => "code or configuration",
    };
    format!(
        "type {}; {} changed files ({class}); {} tests and {} scenarios listed",
        pr_type.map_or("unknown", PrType::as_str),
        changes.len(),
        listed.tests,
        listed.scenarios,
    )
}

/// The command that repeats this run, for the report's `reproduce:` line.
fn reproduce_command(options: &PrMetaOptions, base: &str, head: &str) -> String {
    let author = if options.author.is_empty() {
        String::new()
    } else {
        format!(" --author {}", shell_quote(&options.author))
    };
    let title = options.title.as_deref().map(|title| format!(" --title {}", shell_quote(title)));
    format!(
        "cargo xtask pr-meta --body-file {}{} --base {base} --head {head}{author}",
        shell_quote(&options.body_file.display().to_string()),
        title.unwrap_or_default(),
    )
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Writes the report: the summary, each finding with its check, place and fix, the advice,
/// and the command that reproduces the run. On GitHub Actions each finding is also an error
/// annotation.
fn render(
    outcome: &Outcome,
    body_file: &str,
    reproduce: &str,
    annotate: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    writeln!(out, "pr-meta: {}", outcome.summary)?;
    for finding in &outcome.findings {
        let place = match finding.place {
            Place::Title => "the title".to_owned(),
            Place::Body(Some(line)) => format!("{body_file}:{line}"),
            Place::Body(None) => body_file.to_owned(),
            Place::Files => "the changed files".to_owned(),
        };
        let number = finding.check.number();
        writeln!(out, "check {number} failed at {place}: {}", finding.problem)?;
        writeln!(out, "  fix: {}", finding.fix)?;
        if annotate {
            let message = format!("{} Fix: {}", finding.problem, finding.fix);
            writeln!(out, "::error title=pr-meta check {number}::{}", escape_annotation(&message))?;
        }
    }
    for advice in &outcome.advice {
        writeln!(out, "advice (does not fail): {advice}")?;
    }
    if outcome.findings.is_empty() {
        writeln!(out, "pr-meta: passed")
    } else {
        writeln!(out, "reproduce: {reproduce}")?;
        writeln!(out, "pr-meta: {}", crate::count(outcome.findings.len(), "problem", "problems"))
    }
}

fn escape_annotation(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

#[cfg(test)]
mod tests;

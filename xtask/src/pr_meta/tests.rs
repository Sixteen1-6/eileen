//! Fixture tests for `pr-meta`: one per rule of checks 7, 8 and 19 that BUILD.md section 6.2
//! (WP0 acceptance 11) names, over an in-memory repository, plus two runs against scratch git
//! repositories.

use super::*;
use crate::repo::mem::{MemRepo, ScratchGit};

const TEMPLATE: &str = include_str!("../../../.github/PULL_REQUEST_TEMPLATE.md");
const GOOD_TITLE: &str = "Add the title rule";
const PERSON: &str = "a-contributor";
const XTASK_TEST: &str = "xtask::pr_meta::tests::title_rule_holds";
const CORE_IT_TEST: &str = "eileen-core::it::job_model::focus_job_follows_the_last_mention";

/// A workspace like this repository's: xtask with a unit test, and a core crate with an
/// integration test target `it` holding `job_model.rs`.
fn workspace() -> MemRepo {
    MemRepo::default()
        .file("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\n")
        .file("xtask/Cargo.toml", "[package]\nname = \"xtask\"\n")
        .file("xtask/src/lib.rs", "mod pr_meta;\n")
        .file("xtask/src/pr_meta.rs", XTASK_SOURCE)
        .file("crates/eileen-core/Cargo.toml", "[package]\nname = \"eileen-core\"\n")
        .file("crates/eileen-core/src/lib.rs", "//! The core.\npub fn step() {}\n")
        .file("crates/eileen-core/tests/it/main.rs", "mod job_model;\n")
        .file("crates/eileen-core/tests/it/job_model.rs", JOB_MODEL_SOURCE)
}

const XTASK_SOURCE: &str =
    "#[cfg(test)]\nmod tests {\n    #[test]\n    fn title_rule_holds() {}\n}\n";
const JOB_MODEL_SOURCE: &str = "#[test]\nfn focus_job_follows_the_last_mention() {}\n";

/// The workspace with one source file changed.
fn code_change() -> MemRepo {
    workspace().changed("xtask/src/pr_meta.rs", XTASK_SOURCE)
}

/// The workspace with only documentation changed.
fn docs_change() -> MemRepo {
    workspace().changed("docs/BUILD.md", "# Build\n")
}

/// A description that follows the template, with this type and Evidence section.
fn body(kind: &str, evidence: &str) -> String {
    format!(
        "## Evidence\n\n{evidence}\n\n## Type\n\ntype: {kind}\n\n## What\n\nIt adds a rule.\n\n\
         ## Why\n\nThe owner asked for it.\n\n## Tests at which layer\n\nUnit.\n\n\
         ## Seen failing before the fix\n\nThe fixture failed first.\n\n## Scenario\n\nNone.\n\n\
         ## Suggested guidance changes\n\nNone.\n\n## Architectural changes\n\nNone.\n\n\
         ## Playbook\n\nUnaffected.\n"
    )
}

/// The repository's own template with only the type chosen, as someone would leave it.
fn template_with_type(kind: &str) -> String {
    TEMPLATE
        .lines()
        .map(
            |line| {
                if line.starts_with("type:") { format!("type: {kind}") } else { line.to_owned() }
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(title: &str, body: &str, author: &str, repo: &MemRepo) -> Outcome {
    check(&Input { title, body, author }, repo).expect("the in-memory repository cannot fail")
}

fn run_person(body: &str, repo: &MemRepo) -> Outcome {
    run(GOOD_TITLE, body, PERSON, repo)
}

#[track_caller]
fn assert_passes(outcome: &Outcome) {
    assert!(outcome.findings.is_empty(), "expected a pass, got {:#?}", outcome.findings);
}

#[track_caller]
fn assert_fails_with(outcome: &Outcome, check: Check, needle: &str) {
    assert!(
        outcome.findings.iter().any(|f| f.check == check && f.problem.contains(needle)),
        "expected check {} to mention `{needle}`, got {:#?}",
        check.number(),
        outcome.findings
    );
}

#[test]
fn evidence_empty_or_placeholder_fails_for_every_type() {
    for kind in PrType::ALL {
        let placeholder = run_person(&template_with_type(kind.as_str()), &docs_change());
        assert_fails_with(&placeholder, Check::Description, "Evidence section is empty");
        let empty = run_person(&body(kind.as_str(), ""), &docs_change());
        assert_fails_with(&empty, Check::Description, "Evidence section is empty");
    }
}

#[test]
fn untouched_template_fails_on_type_too() {
    let outcome = run_person(TEMPLATE, &docs_change());
    assert_fails_with(&outcome, Check::Description, "Evidence section is empty");
    assert_fails_with(&outcome, Check::Description, "no type is chosen");
}

#[test]
fn type_line_alone_is_not_evidence() {
    let outcome = run_person(&body("feature", "type: feature"), &code_change());
    assert_fails_with(&outcome, Check::Description, "Evidence section is empty");
    assert_fails_with(&outcome, Check::Description, "`type:` line sits in the Evidence section");
}

#[test]
fn feature_naming_a_missing_test_fails() {
    let evidence = "tests: xtask::pr_meta::tests::no_such_test";
    let outcome = run_person(&body("feature", evidence), &code_change());
    assert_fails_with(&outcome, Check::Description, "does not exist at the head commit");
}

#[test]
fn scenario_id_without_a_folder_fails() {
    let evidence = format!("tests: {XTASK_TEST}\nscenarios: core/missing-scenario");
    let outcome = run_person(&body("feature", &evidence), &code_change());
    assert_fails_with(
        &outcome,
        Check::Description,
        "scenario `core/missing-scenario` does not exist",
    );
}

#[test]
fn fix_without_a_regression_fails() {
    let repo = workspace().changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n");
    let outcome =
        run("Fix the focus job", &body("fix", &format!("tests: {CORE_IT_TEST}")), PERSON, &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
}

#[test]
fn fix_with_a_changed_test_passes() {
    let repo = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .changed("crates/eileen-core/tests/it/job_model.rs", JOB_MODEL_SOURCE);
    let outcome =
        run("Fix the focus job", &body("fix", &format!("tests: {CORE_IT_TEST}")), PERSON, &repo);
    assert_passes(&outcome);
}

#[test]
fn deleting_a_test_is_not_a_regression() {
    let repo = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .file("crates/eileen-core/tests/it/old.rs", "#[test]\nfn old() {}\n")
        .deleted("crates/eileen-core/tests/it/old.rs");
    let outcome =
        run("Fix the focus job", &body("fix", &format!("tests: {CORE_IT_TEST}")), PERSON, &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
}

#[test]
fn fix_that_does_not_say_how_it_failed_fails() {
    let described =
        body("fix", &format!("tests: {XTASK_TEST}")).replace("The fixture failed first.", "");
    let outcome = run("Fix the title rule", &described, PERSON, &code_change());
    assert_fails_with(&outcome, Check::Regression, "seen failing before");
}

#[test]
fn bad_title_fails() {
    let outcome = run(
        "added the rule.",
        &body("feature", &format!("tests: {XTASK_TEST}")),
        PERSON,
        &code_change(),
    );
    assert_fails_with(&outcome, Check::Title, "period");
    assert_fails_with(&outcome, Check::Title, "capital letter");
    assert_fails_with(&outcome, Check::Title, "past tense");
}

#[test]
fn xtask_test_on_the_tests_line_passes() {
    let evidence = format!("The title rule has its own fixture.\ntests: {XTASK_TEST}");
    assert_passes(&run_person(&body("feature", &evidence), &code_change()));
}

#[test]
fn hyphenated_prose_is_not_an_id() {
    let evidence = format!(
        "Covers the follow-up window, barge-in and text like core/made-up-id in prose.\n\
         tests: {XTASK_TEST}"
    );
    let outcome = run_person(&body("feature", &evidence), &code_change());
    assert_passes(&outcome);
    assert!(outcome.summary.contains("0 scenarios"), "prose is not read: {}", outcome.summary);
}

#[test]
fn integration_test_named_by_target_passes() {
    let evidence = format!("tests: {CORE_IT_TEST}");
    assert_passes(&run_person(&body("feature", &evidence), &code_change()));
}

#[test]
fn dependabot_pin_or_lockfile_bump_passes() {
    let dependabot_body = "Bumps [actions/checkout](https://github.com/actions/checkout) from \
                           7.0.1 to 7.0.2.\n<details>\n<summary>Release notes</summary>\n</details>\n";
    let pin = workspace().changed_with_lines(
        ".github/workflows/ci.yml",
        &[
            "      - uses: actions/checkout@3d3c42e5 # v7.0.1",
            "      - uses: actions/checkout@9f8e7d6c # v7.0.2",
        ],
    );
    let title = "Bump actions/checkout from 7.0.1 to 7.0.2 in the github-actions group";
    assert_passes(&run(title, dependabot_body, DEPENDABOT, &pin));
    let lockfile = workspace().changed("Cargo.lock", "version = 4\n");
    assert_passes(&run(
        "Bump serde from 1.0.229 to 1.0.230",
        dependabot_body,
        DEPENDABOT,
        &lockfile,
    ));
}

#[test]
fn dependabot_changing_more_than_pins_fails() {
    let repo = workspace().changed_with_lines(
        ".github/workflows/ci.yml",
        &["      - run: cargo test", "      - run: true"],
    );
    let outcome = run("Bump the github-actions group", "Bumps things.", DEPENDABOT, &repo);
    assert_fails_with(&outcome, Check::Description, "may change only");
}

#[test]
fn docs_type_changing_rust_fails() {
    let outcome =
        run_person(&body("docs", "Rewrote the guide and read it through."), &code_change());
    assert_fails_with(&outcome, Check::Description, "not docs-only");
}

#[test]
fn docs_type_with_only_docs_passes() {
    let outcome =
        run_person(&body("docs", "Rewrote the guide and read it through."), &docs_change());
    assert_passes(&outcome);
}

#[test]
fn scenario_type_outside_scenarios_fails() {
    let repo = workspace()
        .changed("scenarios/core/open-app/scenario.toml", "id = \"open-app\"\n")
        .changed("xtask/src/lib.rs", "mod pr_meta;\n");
    let outcome = run_person(&body("scenario", "Replayed it in scenario mode; it passes."), &repo);
    assert_fails_with(&outcome, Check::Description, "not scenarios-only");
}

#[test]
fn deps_type_by_a_person_changing_source_fails() {
    let repo = workspace()
        .changed("Cargo.lock", "version = 4\n")
        .changed("xtask/src/lib.rs", "mod pr_meta;\n");
    let outcome = run_person(&body("deps", "cargo deny check passes."), &repo);
    assert_fails_with(&outcome, Check::Description, "type deps may change only");
}

#[test]
fn amendment_changing_source_needs_tests() {
    let repo = workspace().changed("crates/eileen-core/src/lib.rs", "pub fn step() {}\n");
    let outcome = run_person(&body("amendment", "Applies the review's contract fix."), &repo);
    assert_fails_with(&outcome, Check::Description, "lists no test or scenario");
    let docs_only = run_person(&body("amendment", "Corrects section 6.2's order."), &docs_change());
    assert_passes(&docs_only);
}

#[test]
fn template_order_is_enforced() {
    let good = body("feature", &format!("tests: {XTASK_TEST}"));
    let swapped =
        good.replacen("## Evidence", "## Evidence-tmp", 1).replacen("## Type", "## Evidence", 1);
    let swapped = swapped.replacen("## Evidence-tmp", "## Type", 1);
    let outcome = run_person(&swapped, &code_change());
    assert_fails_with(&outcome, Check::Description, "does not start with `## Evidence`");
    let preamble = format!("Some words first.\n\n{good}");
    assert_fails_with(
        &run_person(&preamble, &code_change()),
        Check::Description,
        "text comes before",
    );
    let no_why = good.replace("## Why\n\nThe owner asked for it.\n\n", "");
    assert_fails_with(
        &run_person(&no_why, &code_change()),
        Check::Description,
        "no `## Why` section",
    );
}

#[test]
fn crlf_description_from_the_browser_passes() {
    let evidence = format!("Typed in the browser.\ntests: {XTASK_TEST}");
    let crlf = body("feature", &evidence).replace('\n', "\r\n");
    assert_passes(&run_person(&crlf, &code_change()));
}

#[test]
fn fenced_heading_and_field_are_prose() {
    let evidence = format!("tests: {XTASK_TEST}\n```text\n## Type\ntests: not::an::id\n```");
    assert_passes(&run_person(&body("feature", &evidence), &code_change()));
}

#[test]
fn evidence_fields_are_checked() {
    let evidence = format!("tests: {XTASK_TEST}\ntests: {XTASK_TEST}\nreplay: rec_123\nnumbers:");
    let outcome = run_person(&body("feature", &evidence), &code_change());
    assert_fails_with(&outcome, Check::Description, "`tests:` field appears more than once");
    assert_fails_with(&outcome, Check::Description, "is not `rec_<ulid>`");
    assert_fails_with(&outcome, Check::Description, "`numbers:` field is empty");
    let good_replay = format!("tests: {XTASK_TEST}\nreplay: rec_01JZ8Q3V6R4N2K7T9W5X0Y1M3B");
    assert_passes(&run_person(&body("feature", &good_replay), &code_change()));
}

#[test]
fn commit_subjects_are_advice_only() {
    let repo = code_change().commit("added the rule");
    let outcome = run_person(&body("feature", &format!("tests: {XTASK_TEST}")), &repo);
    assert_passes(&outcome);
    assert!(
        outcome.advice.iter().any(|advice| advice.contains("added the rule")),
        "the commit subject gets advice: {:?}",
        outcome.advice
    );
}

#[test]
fn report_names_check_place_fix_and_reproduce() {
    let outcome = run_person(&body("feature", ""), &code_change());
    let mut out = Vec::new();
    render(&outcome, "body.md", "cargo xtask pr-meta --body-file 'body.md'", true, &mut out)
        .expect("writing to memory cannot fail");
    let report = String::from_utf8(out).expect("the report is UTF-8");
    assert!(
        report.contains("check 8 failed at body.md:1: the Evidence section is empty"),
        "{report}"
    );
    assert!(report.contains("\n  fix: "), "every finding has a fix: {report}");
    assert!(
        report.contains("::error title=pr-meta check 8::"),
        "an annotation on Actions: {report}"
    );
    assert!(report.contains("reproduce: cargo xtask pr-meta --body-file 'body.md'"), "{report}");
}

#[test]
fn reproduce_command_quotes_the_title() {
    let options = PrMetaOptions {
        body_file: "pr body.md".into(),
        title: "Fix the author's check".to_owned(),
        base: "origin/main".to_owned(),
        head: "HEAD".to_owned(),
        author: String::new(),
    };
    assert_eq!(
        reproduce_command(&options, "abc", "def"),
        r"cargo xtask pr-meta --body-file 'pr body.md' --title 'Fix the author'\''s check' --base abc --head def",
        "single quotes survive a POSIX shell"
    );
}

#[test]
fn git_backed_run_reads_the_branch() {
    let git = ScratchGit::new("pr-meta");
    git.write("Cargo.toml", "[workspace]\nresolver = \"3\"\nmembers = [\"tool\"]\n");
    git.write("tool/Cargo.toml", "[package]\nname = \"tool\"\n");
    git.write("tool/src/lib.rs", "pub fn parse() {}\n");
    git.commit("Start");
    git.branch("topic");
    git.write(
        "tool/src/lib.rs",
        "pub fn parse() { guard(); }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn rejects_empty() {}\n}\n",
    );
    git.commit("Fix the empty input");
    let repo = GitRepo::open(git.path(), "main", "topic").expect("both revisions resolve");

    let fix = body("fix", "tests: tool::tests::rejects_empty");
    let input = Input { title: "Fix the empty input", body: &fix, author: PERSON };
    let outcome = check(&input, &repo).expect("git answers");
    assert!(
        outcome.findings.is_empty(),
        "the fix and its test are on the branch: {:#?}",
        outcome.findings
    );

    let wrong = body("feature", "tests: tool::tests::accepts_everything");
    let input = Input { title: "Add parsing", body: &wrong, author: PERSON };
    let outcome = check(&input, &repo).expect("git answers");
    assert!(
        outcome.findings.iter().any(|f| f.problem.contains("accepts_everything")),
        "a test that is not at the head fails: {:#?}",
        outcome.findings
    );
}

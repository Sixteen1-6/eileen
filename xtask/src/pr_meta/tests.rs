//! Fixture tests for `pr-meta`: one per rule of checks 7, 8 and 19 that BUILD.md section 6.2
//! (WP0 acceptance 11) names, and one per failure branch of the template and type rules, over an
//! in-memory repository, plus runs against scratch git repositories.

use super::*;
use crate::repo::mem::{MemRepo, ScratchGit};

const TEMPLATE: &str = include_str!("../../../.github/PULL_REQUEST_TEMPLATE.md");
const GOOD_TITLE: &str = "Add the title rule";
const PERSON: &str = "a-contributor";
const XTASK_TEST: &str = "xtask::pr_meta::tests::title_rule_holds";
const CORE_IT_TEST: &str = "eileen-core::it::job_model::focus_job_follows_the_last_mention";
const TURN_TEST: &str = "eileen-core::turn::tests::barge_in_pauses";

/// A workspace like this repository's: xtask with a unit test, and a core crate with an inline
/// test module in `turn.rs` and an integration test target `it` holding `job_model.rs`.
fn workspace() -> MemRepo {
    MemRepo::default()
        .file("Cargo.toml", "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\n")
        .file("xtask/Cargo.toml", "[package]\nname = \"xtask\"\n")
        .file("xtask/src/lib.rs", "mod pr_meta;\n")
        .file("xtask/src/pr_meta.rs", XTASK_SOURCE)
        .file("crates/eileen-core/Cargo.toml", "[package]\nname = \"eileen-core\"\n")
        .file("crates/eileen-core/src/lib.rs", "//! The core.\npub fn step() {}\npub mod turn;\n")
        .file("crates/eileen-core/src/turn.rs", TURN_SOURCE)
        .file("crates/eileen-core/tests/it/main.rs", "mod job_model;\n")
        .file("crates/eileen-core/tests/it/job_model.rs", JOB_MODEL_SOURCE)
        .file("scenarios/core/hello/scenario.toml", "id = \"hello\"\n")
}

const XTASK_SOURCE: &str = "pub(crate) fn check() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    \
                            fn title_rule_holds() {}\n}\n";
const JOB_MODEL_SOURCE: &str = "#[test]\nfn focus_job_follows_the_last_mention() {}\n";
/// `turn.rs`: production code on lines 1 to 3, the test module on lines 5 to 13, its helper
/// on line 7 and the test itself on lines 9 to 12.
const TURN_SOURCE: &str = "\
pub fn step(paused: bool) -> bool {
    !paused
}

#[cfg(test)]
mod tests {
    fn paused() -> bool { true }

    #[test]
    fn barge_in_pauses() {
        assert!(!super::step(paused()));
    }
}
";

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
    check(&Input { title: Some(title), body, author }, repo)
        .expect("the in-memory repository cannot fail")
}

fn run_person(body: &str, repo: &MemRepo) -> Outcome {
    run(GOOD_TITLE, body, PERSON, repo)
}

fn run_fix(evidence: &str, repo: &MemRepo) -> Outcome {
    run("Fix the barge-in pause", &body("fix", evidence), PERSON, repo)
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
fn feature_naming_a_function_that_is_not_a_test_fails() {
    let outcome = run_person(&body("feature", "tests: xtask::pr_meta::check"), &code_change());
    assert_fails_with(&outcome, Check::Description, "is not a test");
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
    let outcome = run_fix(&format!("tests: {CORE_IT_TEST}"), &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
}

#[test]
fn fix_with_a_changed_test_passes() {
    let repo = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .changed("crates/eileen-core/tests/it/job_model.rs", JOB_MODEL_SOURCE);
    assert_passes(&run_fix(&format!("tests: {CORE_IT_TEST}"), &repo));
}

#[test]
fn fix_editing_only_code_beside_an_unchanged_inline_test_fails() {
    let repo = workspace().edited("crates/eileen-core/src/turn.rs", TURN_SOURCE, &[2]);
    let outcome = run_fix(&format!("tests: {TURN_TEST}"), &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
}

#[test]
fn fix_changing_the_listed_inline_test_passes() {
    let repo = workspace().edited("crates/eileen-core/src/turn.rs", TURN_SOURCE, &[2, 11]);
    assert_passes(&run_fix(&format!("tests: {TURN_TEST}"), &repo));
}

#[test]
fn fix_changing_the_test_module_around_the_listed_test_passes() {
    let repo = workspace().edited("crates/eileen-core/src/turn.rs", TURN_SOURCE, &[2, 7]);
    assert_passes(&run_fix(&format!("tests: {TURN_TEST}"), &repo));
}

#[test]
fn deleting_a_test_is_not_a_regression() {
    let repo = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .file("crates/eileen-core/tests/it/old.rs", "#[test]\nfn old() {}\n")
        .deleted("crates/eileen-core/tests/it/old.rs");
    let outcome = run_fix(&format!("tests: {CORE_IT_TEST}"), &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
}

#[test]
fn scenario_folders_count_as_regressions_but_their_readme_and_schema_do_not() {
    let repo = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .changed("scenarios/README.md", "# Scenarios\n")
        .changed("scenarios/core/README.md", "# Core scenarios\n")
        .changed("scenarios/_schema/v1/scenario.schema.json", "{}\n");
    let outcome = run_fix(&format!("tests: {CORE_IT_TEST}"), &repo);
    assert_fails_with(&outcome, Check::Regression, "adds or changes no regression");
    let scenario = workspace()
        .changed("crates/eileen-core/src/lib.rs", "pub fn step() { fixed(); }\n")
        .changed("scenarios/core/hello/scenario.toml", "id = \"hello\"\nsteps = 2\n");
    assert_passes(&run_fix("scenarios: core/hello", &scenario));
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
fn without_a_title_the_head_commit_subject_is_checked() {
    let described = body("feature", &format!("tests: {XTASK_TEST}"));
    let untitled =
        |repo: &MemRepo| check(&Input { title: None, body: &described, author: PERSON }, repo);
    let good = untitled(&code_change().commit("Add the title rule")).expect("a commit exists");
    assert_passes(&good);
    assert!(
        good.summary.contains("\"Add the title rule\" (the head commit's subject"),
        "the summary says which title was checked: {}",
        good.summary
    );
    let bad =
        untitled(&code_change().commit("Start").commit("added the rule.")).expect("commits exist");
    assert_fails_with(&bad, Check::Title, "past tense");
    assert!(
        bad.advice.iter().all(|advice| !advice.contains("added the rule.")),
        "the subject checked as the title is not repeated as advice: {:?}",
        bad.advice
    );
    let error = untitled(&code_change()).expect_err("no commit and no title leaves nothing");
    assert!(error.0.contains("--title"), "the error says how to go on: {error}");
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
    let binary = workspace().changed_with_lines("docs/diagram.png", &[]);
    let outcome = run("Bump the github-actions group", "Bumps things.", DEPENDABOT, &binary);
    assert_fails_with(&outcome, Check::Description, "docs/diagram.png");
}

#[test]
fn deps_type_with_a_binary_change_fails() {
    for path in ["scenarios/core/hello/clip.wav", "models/vad.onnx"] {
        let repo = workspace().changed("Cargo.lock", "version = 4\n").changed_with_lines(path, &[]);
        let outcome = run_person(&body("deps", "cargo deny check passes."), &repo);
        assert_fails_with(&outcome, Check::Description, path);
    }
}

#[test]
fn deps_type_with_a_mode_or_blank_line_change_to_a_workflow_fails() {
    let mode = workspace().changed_with_lines(".github/workflows/ci.yml", &[]);
    let outcome = run_person(&body("deps", "cargo deny check passes."), &mode);
    assert_fails_with(&outcome, Check::Description, "no text line changed");
    let blank = workspace().changed_with_lines(
        ".github/workflows/ci.yml",
        &["      - uses: a/b@111 # v1", "      - uses: a/b@222 # v2", ""],
    );
    let outcome = run_person(&body("deps", "cargo deny check passes."), &blank);
    assert_fails_with(&outcome, Check::Description, "lines other than `uses:`");
}

#[test]
fn deps_type_with_a_blank_or_uses_line_in_rust_fails() {
    for lines in [&[""][..], &["    uses: n,"][..]] {
        let repo = workspace().changed_with_lines("xtask/src/lib.rs", lines);
        let outcome = run_person(&body("deps", "cargo deny check passes."), &repo);
        assert_fails_with(&outcome, Check::Description, "xtask/src/lib.rs (not a manifest");
    }
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
fn a_heading_given_twice_fails() {
    let twice = format!("{}\n## Why\n\nAgain.\n", body("feature", &format!("tests: {XTASK_TEST}")));
    let outcome = run_person(&twice, &code_change());
    assert_fails_with(&outcome, Check::Description, "`## Why` appears more than once");
}

#[test]
fn a_heading_out_of_order_fails() {
    let good = body("feature", &format!("tests: {XTASK_TEST}"));
    let moved = good
        .replace("## Scenario\n\nNone.\n\n", "")
        .replace("## Why\n", "## Scenario\n\nNone.\n\n## Why\n");
    let outcome = run_person(&moved, &code_change());
    assert_fails_with(&outcome, Check::Description, "`## Scenario` is out of the template's order");
}

#[test]
fn type_not_second_fails() {
    let good = body("feature", &format!("tests: {XTASK_TEST}"));
    let late = good.replace(
        "## Type\n\ntype: feature\n\n## What\n\nIt adds a rule.\n\n",
        "## What\n\nIt adds a rule.\n\n## Type\n\ntype: feature\n\n",
    );
    let outcome = run_person(&late, &code_change());
    assert_fails_with(&outcome, Check::Description, "`## Type` does not come right after");
}

#[test]
fn a_malformed_type_section_fails_with_its_reason() {
    let good = body("feature", &format!("tests: {XTASK_TEST}"));
    let cases = [
        ("", "the Type section is empty"),
        ("type: feature\nAnd it is small.", "holds more than its one `type:` line"),
        ("kind: feature", "is not `type: <value>`"),
        ("type: bugfix", "`bugfix` is not a type"),
    ];
    for (type_section, reason) in cases {
        let described = good.replace("type: feature", type_section);
        assert_fails_with(&run_person(&described, &code_change()), Check::Description, reason);
    }
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
fn a_comment_marker_quoted_in_code_is_text() {
    let inline = format!("The template's `<!--` comments are removed.\ntests: {XTASK_TEST}");
    assert_passes(&run_person(&body("feature", &inline), &code_change()));
    let fenced = format!("tests: {XTASK_TEST}\n```html\n<!-- the template's guidance\n```");
    assert_passes(&run_person(&body("feature", &fenced), &code_change()));
    let after_code =
        format!("Shown as `x`. <!-- a comment\ntests: not::an::id\n-->\ntests: {XTASK_TEST}");
    assert_passes(&run_person(&body("feature", &after_code), &code_change()));
}

#[test]
fn evidence_fields_are_checked() {
    let evidence = format!(
        "tests: {XTASK_TEST}\ntests: {XTASK_TEST}\nreplay: rec_123\nincident: inc_123\nnumbers:"
    );
    let outcome = run_person(&body("feature", &evidence), &code_change());
    assert_fails_with(&outcome, Check::Description, "`tests:` field appears more than once");
    assert_fails_with(&outcome, Check::Description, "is not `rec_<ulid>`");
    assert_fails_with(&outcome, Check::Description, "`incident: inc_123` is not `inc_<ulid>`");
    assert_fails_with(&outcome, Check::Description, "`numbers:` field is empty");
    let good_ids = format!(
        "tests: {XTASK_TEST}\nreplay: rec_01JZ8Q3V6R4N2K7T9W5X0Y1M3B\n\
         incident: inc_01JZ8Q3V6R4N2K7T9W5X0Y1M3B"
    );
    assert_passes(&run_person(&body("feature", &good_ids), &code_change()));
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
    let mut options = PrMetaOptions {
        body_file: "pr body.md".into(),
        title: Some("Fix the author's check".to_owned()),
        base: "origin/main".to_owned(),
        head: "HEAD".to_owned(),
        author: String::new(),
    };
    assert_eq!(
        reproduce_command(&options, "abc", "def"),
        r"cargo xtask pr-meta --body-file 'pr body.md' --title 'Fix the author'\''s check' --base abc --head def",
        "single quotes survive a POSIX shell"
    );
    options.title = None;
    assert_eq!(
        reproduce_command(&options, "abc", "def"),
        "cargo xtask pr-meta --body-file 'pr body.md' --base abc --head def",
        "a run without a title reproduces without one"
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
    let input = Input { title: Some("Fix the empty input"), body: &fix, author: PERSON };
    let outcome = check(&input, &repo).expect("git answers");
    assert!(
        outcome.findings.is_empty(),
        "the fix and its test are on the branch: {:#?}",
        outcome.findings
    );

    let wrong = body("feature", "tests: tool::tests::accepts_everything");
    let input = Input { title: Some("Add parsing"), body: &wrong, author: PERSON };
    let outcome = check(&input, &repo).expect("git answers");
    assert!(
        outcome.findings.iter().any(|f| f.problem.contains("accepts_everything")),
        "a test that is not at the head fails: {:#?}",
        outcome.findings
    );
}

#[test]
fn git_backed_fix_must_touch_the_listed_test() {
    let git = ScratchGit::new("pr-meta-fix");
    let tests = "\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn rejects_empty() {}\n}\n";
    git.write("Cargo.toml", "[workspace]\nresolver = \"3\"\nmembers = [\"tool\"]\n");
    git.write("tool/Cargo.toml", "[package]\nname = \"tool\"\n");
    git.write("tool/src/lib.rs", &format!("pub fn parse() {{}}\n{tests}"));
    git.commit("Start with a test");
    git.branch("topic");
    git.write("tool/src/lib.rs", &format!("pub fn parse() {{ guard(); }}\n{tests}"));
    git.commit("Fix the empty input");
    let fix = body("fix", "tests: tool::tests::rejects_empty");
    let input = Input { title: Some("Fix the empty input"), body: &fix, author: PERSON };

    let repo = GitRepo::open(git.path(), "main", "topic").expect("both revisions resolve");
    let outcome = check(&input, &repo).expect("git answers");
    assert!(
        outcome.findings.iter().any(|f| f.check == Check::Regression),
        "only production code changed beside the old test: {:#?}",
        outcome.findings
    );

    let changed_test = tests.replace("fn rejects_empty() {}", "fn rejects_empty() { check(); }");
    git.write("tool/src/lib.rs", &format!("pub fn parse() {{ guard(); }}\n{changed_test}"));
    git.commit("Show the empty input failing");
    let repo = GitRepo::open(git.path(), "main", "topic").expect("both revisions resolve");
    let outcome = check(&input, &repo).expect("git answers");
    assert!(outcome.findings.is_empty(), "the test changed too: {:#?}", outcome.findings);
}

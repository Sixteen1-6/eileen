use serde_json::json;

use super::{
    Expectation, ExpectedOutcome, Lane, Quarantine, Scenario, ScenarioMeta, ScenarioOrigin,
    ScenarioStatus, StatusKind, StatusProblem, Step, status_problems,
};
use crate::ids::{Date, ScenarioId};

/// The example of BUILD.md section 3.3.14, verbatim.
const EXAMPLE: &str = r#"
schema = 1
id = "resolve-goto-timecode"
title = "Go to a time in the Resolve timeline"
area = "apps/resolve"
tags = ["resolve", "navigation"]
origin = "synthetic"
outcome = "pass"

[intent]
utterances = ["Open Resolve and go to 15:30", "uh go to fifteen thirty in resolve"]
voice = "kokoro:af_heart"            # used when a runner needs audio; the text is the source of truth

[environment]
requires = ["resolve-studio>=21.0"]
lanes = ["sim", "desktop"]

[fixtures]
apps = [{ launch = "resolve", project_template = "_fixtures/eileen-test" }]

[[steps]]
say = { utterance = 0 }

[[steps]]
wait_for = { event = "turn.ended", timeout_ms = 20000 }

[[expect]]
kind = "resolve_playhead"
timecode = "01:15:30:00"
tolerance_frames = 0

[[expect]]
kind = "call"
tool = "apps_resolve"
args_contain = { op = { goto = { offset_s = 930.0 } } }

[timing]
"turn.ended -> speaking.started" = { max_ms = 2500, ci_factor = 3.0 }

[sim]
inputs = "replay/inputs.jsonl"
expected = "replay/expected.trace"
"#;

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

fn meta(status: ScenarioStatus, quarantine: Option<Quarantine>) -> ScenarioMeta {
    ScenarioMeta { id: ScenarioId::new("flaky-one").unwrap(), status, quarantine }
}

fn stale(since: &str, expires: &str, issue: &str) -> ScenarioStatus {
    ScenarioStatus::Stale {
        since_commit: "4a75463".to_owned(),
        since: date(since),
        issue: issue.to_owned(),
        expires: date(expires),
    }
}

fn quarantine(since: &str, expires: &str) -> Quarantine {
    Quarantine {
        issue: "https://github.com/Sixteen1-6/eileen/issues/1".to_owned(),
        since: date(since),
        expires: date(expires),
    }
}

const ISSUE: &str = "https://github.com/Sixteen1-6/eileen/issues/2";

#[test]
fn the_plan_s_example_scenario_parses() {
    let scenario: Scenario = toml::from_str(EXAMPLE).unwrap();
    assert_eq!(scenario.id.as_str(), "resolve-goto-timecode", "id");
    assert_eq!(scenario.origin, ScenarioOrigin::Synthetic, "origin");
    assert_eq!(scenario.outcome, ExpectedOutcome::Pass, "outcome");
    assert_eq!(scenario.environment.lanes, [Lane::Sim, Lane::Desktop], "lanes");
    assert_eq!(scenario.steps.first(), Some(&Step::Say { utterance: 0 }), "first step");
    assert_eq!(scenario.status, ScenarioStatus::Active, "status defaults to active");
    let Some(Expectation::Call { args_contain, .. }) = scenario.expect.get(1) else {
        panic!("the second expectation is a call: {:?}", scenario.expect);
    };
    assert_eq!(args_contain, &json!({ "op": { "goto": { "offset_s": 930.0 } } }), "args");
    let budget = &scenario.timing["turn.ended -> speaking.started"];
    assert_eq!(budget.max_ms, 2500, "timing");
}

#[test]
fn steps_without_fields_are_written_as_empty_tables() {
    let text =
        format!("{EXAMPLE}\n[[steps]]\napprove = {{}}\n\n[[steps]]\nphysical_input = {{}}\n");
    let scenario: Scenario = toml::from_str(&text).unwrap();
    assert_eq!(scenario.steps.get(2), Some(&Step::Approve), "approve");
    assert_eq!(scenario.steps.get(3), Some(&Step::PhysicalInput), "physical input");
}

#[test]
fn unknown_fields_and_origins_are_refused() {
    let typo = EXAMPLE.replace("tags = ", "tag = ");
    assert!(toml::from_str::<Scenario>(&typo).is_err(), "a misspelt field");
    let user = EXAMPLE.replace("origin = \"synthetic\"", "origin = \"user\"");
    assert!(toml::from_str::<Scenario>(&user).is_err(), "origin user");
    let shared = EXAMPLE.replace("origin = \"synthetic\"", "origin = \"synthetic-from-user\"");
    let parsed: Scenario = toml::from_str(&shared).unwrap();
    assert_eq!(parsed.origin, ScenarioOrigin::SyntheticFromUser, "kebab-case origin");
}

#[test]
fn an_active_scenario_has_no_status_problems() {
    let today = date("2026-10-21");
    assert_eq!(status_problems(&meta(ScenarioStatus::Active, None), today), [], "active");
    let within = meta(
        stale("2026-10-01", "2026-10-31", ISSUE),
        Some(quarantine("2026-08-01", "2026-10-30")),
    );
    assert_eq!(status_problems(&within, today), [], "both within their caps");
}

#[test]
fn an_expired_status_or_quarantine_is_a_problem() {
    let today = date("2026-10-21");
    let stale_meta = meta(stale("2026-09-25", "2026-10-20", ISSUE), None);
    assert_eq!(
        status_problems(&stale_meta, today),
        [StatusProblem::Expired { what: StatusKind::Stale, expires: date("2026-10-20") }],
        "stale"
    );
    let quarantined = meta(ScenarioStatus::Active, Some(quarantine("2026-08-01", "2026-10-20")));
    assert_eq!(
        status_problems(&quarantined, today),
        [StatusProblem::Expired { what: StatusKind::Quarantine, expires: date("2026-10-20") }],
        "quarantine"
    );
    let last_day = meta(stale("2026-09-25", "2026-10-21", ISSUE), None);
    assert_eq!(status_problems(&last_day, today), [], "the expiry day itself still counts");
}

#[test]
fn a_limit_longer_than_its_cap_is_a_problem() {
    let today = date("2026-10-21");
    let stale_meta = meta(stale("2026-10-21", "2026-11-21", ISSUE), None);
    assert_eq!(
        status_problems(&stale_meta, today),
        [StatusProblem::TooLong { what: StatusKind::Stale, days: 31, cap: 30 }],
        "31 days stale"
    );
    let at_cap = meta(stale("2026-10-21", "2026-11-20", ISSUE), None);
    assert_eq!(status_problems(&at_cap, today), [], "30 days stale");
    let quarantined = meta(ScenarioStatus::Active, Some(quarantine("2026-10-21", "2027-01-20")));
    assert_eq!(
        status_problems(&quarantined, today),
        [StatusProblem::TooLong { what: StatusKind::Quarantine, days: 91, cap: 90 }],
        "91 days quarantined"
    );
    let ninety = meta(ScenarioStatus::Active, Some(quarantine("2026-10-21", "2027-01-19")));
    assert_eq!(status_problems(&ninety, today), [], "90 days quarantined");
}

#[test]
fn a_since_after_today_and_a_missing_issue_are_problems() {
    let today = date("2026-10-21");
    let early = meta(stale("2026-10-22", "2026-11-01", "  "), None);
    assert_eq!(
        status_problems(&early, today),
        [
            StatusProblem::SinceInFuture { what: StatusKind::Stale, since: date("2026-10-22") },
            StatusProblem::NoIssue { what: StatusKind::Stale },
        ],
        "both problems, in order"
    );
}

//! The registry's steps, names, slugs, checkouts, pick, focus, successors and progress.

use std::collections::BTreeMap;

use eileen_contracts::ids::{JobId, TurnId, WallTime};
use eileen_contracts::jobs::{
    Cost, Depth, FanoutMode, Job, JobKind, JobSpec, JobStatus, Route, Taint,
};
use eileen_contracts::tools::brain::JobRef;

use super::pick::{kind_named, plain_words, route_named};
use super::{Pick, Registry, Step, absolute, fnv1a, legal, name_of, repo_key, slug_of};

const ALL: [JobStatus; 8] = [
    JobStatus::Queued,
    JobStatus::Running,
    JobStatus::Blocked,
    JobStatus::AwaitingApproval,
    JobStatus::Completed,
    JobStatus::Failed,
    JobStatus::Cancelled,
    JobStatus::Interrupted,
];

#[test]
fn the_steps_take_exactly_the_edges_of_the_graph() {
    let steps = [
        Step::Start,
        Step::Block,
        Step::AwaitApproval { deadline: WallTime(0) },
        Step::Resume,
        Step::Complete,
        Step::Fail,
        Step::Cancel,
        Step::Interrupt,
    ];
    for from in ALL {
        for to in ALL {
            let by_step = steps.iter().any(|step| step.leaves(from) && step.status() == to);
            assert_eq!(by_step, legal(from, to), "{from:?} to {to:?}");
        }
        if from.is_terminal() {
            assert!(steps.iter().all(|step| !step.leaves(from)), "{from:?} is final");
        }
    }
}

#[test]
fn names_have_two_to_four_words() {
    assert_eq!(
        name_of(Some("the hello file please now"), JobKind::General),
        "the hello file please"
    );
    assert_eq!(name_of(Some("render"), JobKind::AppTask), "the render", "one word");
    assert_eq!(name_of(Some(" The "), JobKind::Research), "the research job", "an article");
    assert_eq!(name_of(None, JobKind::Coding), "the coding job", "no hint");
    assert_eq!(name_of(Some("   "), JobKind::Coding), "the coding job", "a blank hint");
}

#[test]
fn slugs_are_short_lower_case_ascii_with_no_leading_article() {
    assert_eq!(slug_of("The hello file!"), "hello-file", "the article goes");
    assert_eq!(slug_of("Fix Ünïcode parsing"), "fix-n-code-parsing", "ASCII only");
    assert_eq!(slug_of("日本語"), "", "nothing to keep");
    let long = slug_of("synchronise every calendar entry between devices tonight");
    assert_eq!(long, "synchronise-every-calendar-entry-between", "cut at a word");
    assert_eq!(slug_of(&"x".repeat(90)).len(), super::SLUG_MAX, "one long word is cut");
}

#[test]
fn one_repository_named_two_ways_has_one_key() {
    assert_eq!(repo_key("D:\\Code\\Demo\\"), repo_key(" d:/code/demo"), "the same repository");
    assert_ne!(repo_key("D:\\code\\demo"), repo_key("D:\\code\\other"), "another one");
    assert_eq!(repo_key("/home/u/demo/"), repo_key("/home/u/demo"), "a trailing slash");
    assert_ne!(repo_key("/home/u/Demo"), repo_key("/home/u/demo"), "Linux keeps case");
    assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325, "the offset basis");
    assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c, "the published value for \"a\"");
}

#[test]
fn only_an_absolute_path_with_no_dot_parts_is_a_checkout() {
    let checkouts =
        ["D:\\code\\demo", " d:/code/demo/ ", "\\\\server\\share\\demo", "/home/u/demo"];
    for repo in checkouts {
        assert!(absolute(repo), "{repo:?} is a checkout");
    }
    let names = ["demo", "learning", "..\\..", "..\\x", "run/x", "D:demo", "D:\\code\\..\\x"];
    for repo in names.into_iter().chain(["/home/./u", "\\\\.\\pipe\\x", "", "   "]) {
        assert!(!absolute(repo), "{repo:?} is not");
    }
}

/// A job the registry names for itself, number `n`, accepted at `n` ms.
fn job(registry: &Registry, n: u64, hint: &str, kind: JobKind, parent: Option<JobId>) -> Job {
    let at = WallTime(i64::try_from(n).unwrap() * 1_000);
    let naming = registry.naming(Some(hint), kind, parent, at).unwrap();
    let spec = JobSpec {
        kind,
        task: hint.to_owned(),
        title_hint: Some(hint.to_owned()),
        depth: Depth::Considered,
        route: None,
        model: None,
        fanout: if hint == "flaky login tests" { 2 } else { 1 },
        mode: FanoutMode::Compete,
        subtasks: Vec::new(),
        subquestions: Vec::new(),
        repo: None,
        resume_session: None,
        skill: None,
        slots: BTreeMap::new(),
        topic: None,
        anticipated: false,
        authorized_by: TurnId::from_parts(1, 1),
        parent,
        chrome: false,
    };
    Job {
        id: JobId::from_parts(n, u128::from(n)),
        name: naming.name,
        slug: naming.slug,
        ordinal: naming.ordinal,
        spec,
        status: JobStatus::Queued,
        route: Route::Claude,
        model: "mid".to_owned(),
        session: None,
        created: at,
        updated: at,
        last_progress: None,
        progress_pct: None,
        salience_pct: 50,
        worktree: None,
        branch: None,
        snapshot_ref: None,
        cost: Cost::default(),
        grants: Vec::new(),
        taint: Taint::default(),
        focus: false,
        override_words: None,
        approval_deadline: None,
    }
}

/// A report, a price check (research), a render (an app task) and a fan-out of two coding
/// agents, accepted in that order.
fn four_jobs() -> (Registry, [JobId; 6]) {
    let mut registry = Registry::default();
    let plain = [
        (1, "the report", JobKind::General),
        (2, "the price check", JobKind::Research),
        (3, "render", JobKind::AppTask),
        (4, "flaky login tests", JobKind::Coding),
    ];
    for (n, hint, kind) in plain {
        let job = job(&registry, n, hint, kind, None);
        registry.accept(job).unwrap();
    }
    let parent = JobId::from_parts(4, 4);
    for n in [5, 6] {
        let child = job(&registry, n, "", JobKind::Coding, Some(parent));
        registry.accept(child).unwrap();
    }
    let ids = [1, 2, 3, 4, 5, 6].map(|n| JobId::from_parts(n, u128::from(n)));
    (registry, ids)
}

fn name(words: &str) -> JobRef {
    JobRef::Name(words.to_owned())
}

#[test]
fn words_name_a_job_by_name_slug_ordinal_kind_and_route_in_that_order() {
    let (registry, [report, price, render, fan, one, two]) = four_jobs();
    let pick = |target: JobRef| registry.pick(Some(&target), None);
    assert_eq!(pick(name("The Report")), Pick::One(report), "a name in any case");
    assert_eq!(pick(name("report")), Pick::One(report), "a name without its article");
    assert_eq!(pick(JobRef::Slug("price-check".to_owned())), Pick::One(price), "a slug");
    assert_eq!(pick(JobRef::Id(render)), Pick::One(render), "an id");
    assert_eq!(pick(JobRef::Ordinal(2)), Pick::One(two), "an ordinal");
    assert_eq!(pick(name("agent one")), Pick::One(one), "a child's name");
    assert_eq!(pick(name("the render")), Pick::One(render), "the render by name");
    assert_eq!(pick(name("the research job")), Pick::One(price), "a kind word");
    assert_eq!(pick(name("the coding job")), Pick::One(fan), "a kind word names no child");
    assert_eq!(
        pick(name("Claude's job")),
        Pick::Ask(vec![fan, price, report]),
        "a route word that fits several jobs, none in focus, asks, newest first, never the render"
    );
    assert_eq!(pick(name("the other file")), Pick::Nothing, "words that fit no job");
    assert_eq!(pick(JobRef::Ordinal(3)), Pick::Nothing, "no third agent");
}

#[test]
fn a_pronoun_names_the_announced_job_then_the_focus_job_then_the_newest() {
    let (mut registry, [report, price, render, fan, _, _]) = four_jobs();
    assert_eq!(registry.pick(None, None), Pick::One(fan), "the newest outside a fan-out");
    assert_eq!(registry.focus_on(price, WallTime(10)), [price], "the price check in focus");
    for words in [None, Some(name("it")), Some(name("that one")), Some(name("the job"))] {
        let picked = registry.pick(words.as_ref(), None);
        assert_eq!(picked, Pick::One(price), "{words:?}: the focus job");
    }
    let said = registry.pick(Some(&name("it")), Some(render));
    assert_eq!(said, Pick::One(render), "it, right after an announcement");
    assert_eq!(
        registry.pick(Some(&name("Claude's job")), None),
        Pick::One(price),
        "the focus job among several that fit"
    );
    registry.step(report, Step::Start, WallTime(11)).unwrap();
    registry.step(report, Step::Complete, WallTime(12)).unwrap();
    let finished = registry.pick(Some(&name("it")), Some(report));
    assert_eq!(finished, Pick::One(price), "a finished announced job is passed over");
    assert_eq!(registry.pick(Some(&name("the report")), None), Pick::Nothing, "finished");
}

#[test]
fn the_focus_is_held_by_one_job_and_moves_off_a_finished_one() {
    let (mut registry, [report, price, _, fan, one, _]) = four_jobs();
    assert_eq!(registry.focused(), None, "nothing in focus yet");
    assert!(registry.refocus(WallTime(9)).contains(&fan), "the newest outside a fan-out");
    let moved = registry.focus_on(price, WallTime(10));
    assert_eq!(moved, [price, fan], "the old focus goes as the new one comes");
    assert!(registry.focus_on(price, WallTime(11)).is_empty(), "already in focus");
    assert!(registry.refocus(WallTime(11)).is_empty(), "the focus job runs on");
    registry.step(price, Step::Cancel, WallTime(12)).unwrap();
    let moved = registry.refocus(WallTime(13));
    assert_eq!(moved, [price, fan], "a finished job gives it up, to the newest");
    assert_eq!(registry.focused(), Some(fan), "one focus job");
    assert_eq!(registry.focus_on(one, WallTime(14)), [fan, one], "a child can be followed");
    registry.step(report, Step::Cancel, WallTime(15)).unwrap();
    assert!(registry.focus_on(report, WallTime(16)).is_empty(), "never a finished job");
    let count = registry.jobs().iter().filter(|job| job.focus).count();
    assert_eq!(count, 1, "exactly one job holds the focus");
}

#[test]
fn a_route_word_and_the_focus_pass_over_an_apps_own_render() {
    let mut registry = Registry::default();
    for (n, hint, kind) in [(1, "the report", JobKind::General), (2, "render", JobKind::AppTask)] {
        let job = job(&registry, n, hint, kind, None);
        registry.accept(job).unwrap();
    }
    let [report, render] = [1, 2].map(|n| JobId::from_parts(n, u128::from(n)));
    assert_eq!(
        registry.pick(Some(&name("Claude's job")), None),
        Pick::One(report),
        "not the render"
    );
    assert_eq!(registry.refocus(WallTime(10)), [report], "the focus passes over the newer render");
    registry.step(report, Step::Cancel, WallTime(11)).unwrap();
    assert_eq!(registry.pick(Some(&name("Claude's job")), None), Pick::Nothing, "no route runs it");
    let moved = registry.refocus(WallTime(12));
    assert_eq!(moved, [report, render], "the render takes it once nothing else is open");
}

#[test]
fn an_ordinal_or_an_agents_name_picks_the_agent_of_the_fan_out_in_focus() {
    let mut registry = Registry::default();
    for (n, hint) in [(1, "flaky login tests"), (4, "the parser fix")] {
        let parent = job(&registry, n, hint, JobKind::Coding, None);
        registry.accept(parent).unwrap();
        let of = JobId::from_parts(n, u128::from(n));
        for child in [n + 1, n + 2] {
            let child = job(&registry, child, "", JobKind::Coding, Some(of));
            registry.accept(child).unwrap();
        }
    }
    let [first, first_one, first_two, second, _, second_two] =
        [1, 2, 3, 4, 5, 6].map(|n| JobId::from_parts(n, u128::from(n)));
    let agent_two = [JobRef::Ordinal(2), name("agent two")];
    for target in &agent_two {
        let picked = registry.pick(Some(target), None);
        assert_eq!(
            picked,
            Pick::Ask(vec![second_two, first_two]),
            "{target:?}: no focus, a question"
        );
    }
    for (focus, wanted, why) in [
        (first, first_two, "the first fan-out in focus"),
        (second, second_two, "the second fan-out in focus"),
        (first_one, first_two, "a sibling of the agent followed"),
    ] {
        registry.focus_on(focus, WallTime(10));
        for target in &agent_two {
            assert_eq!(registry.pick(Some(target), None), Pick::One(wanted), "{target:?}: {why}");
        }
    }
}

#[test]
fn a_successor_keeps_the_name_and_ordinal_with_a_slug_of_its_own() {
    let (registry, [report, _, _, _, _, two]) = four_jobs();
    let next = registry.successor(report, WallTime(20)).unwrap();
    assert_eq!((next.name.as_str(), next.ordinal), ("the report", None), "the same name");
    assert_eq!(next.slug, "report-2", "a slug of its own");
    let next = registry.successor(two, WallTime(20)).unwrap();
    assert_eq!((next.name.as_str(), next.ordinal), ("agent two", Some(2)), "the same agent");
}

#[test]
fn progress_is_kept_once_per_change_and_never_past_a_hundred() {
    let (mut registry, [_, _, render, _, _, _]) = four_jobs();
    assert!(registry.progress(render, 60, WallTime(1)).is_some(), "a new percent");
    assert!(registry.progress(render, 60, WallTime(2)).is_none(), "the same percent");
    let pct = registry.progress(render, 250, WallTime(3)).and_then(|job| job.progress_pct);
    assert_eq!(pct, Some(100), "at most a hundred");
}

#[test]
fn plain_words_drop_articles_filler_and_possessives() {
    assert_eq!(plain_words("Claude's job"), ["claude"], "a possessive");
    assert_eq!(plain_words("the  Render, please"), ["render"], "an article and filler");
    assert_eq!(plain_words("Claude\u{2019}s task"), ["claude"], "a curly apostrophe");
    assert_eq!(kind_named(&plain_words("the deep answer")), Some(JobKind::DeepAnswer), "two");
    assert_eq!(kind_named(&plain_words("the hello file")), None, "not a kind word");
    assert_eq!(route_named(&plain_words("the local model")), Some(Route::Local), "a route");
}

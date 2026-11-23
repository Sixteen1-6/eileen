use eileen_contracts::ids::MonoTime;

use super::{
    Decision, Facts, Name, Window, content_words, decide, is_backchannel, levenshtein, soundex,
    sounds_like, strip_name, words,
};

fn names() -> Vec<String> {
    vec!["Eileen".to_owned()]
}

const fn s(n: u64) -> MonoTime {
    MonoTime(n * 1_000_000_000)
}

#[test]
fn words_are_lower_case_without_edge_punctuation() {
    assert_eq!(
        words("Eileen, what's the TIME? mm-hm..."),
        ["eileen", "what's", "the", "time", "mm-hm"],
        "inner apostrophes and hyphens stay"
    );
}

#[test]
fn soundex_codes_follow_the_classic_rules() {
    assert_eq!(soundex("Robert"), Some(('r', [1, 6, 3])), "Robert is R163");
    assert_eq!(soundex("Rupert"), Some(('r', [1, 6, 3])), "Rupert sounds the same");
    assert_eq!(soundex("Ashcraft"), Some(('a', [2, 6, 1])), "h does not separate equal codes");
    assert_eq!(soundex("Eileen"), soundex("Aileen"), "leading vowels count as one letter");
    assert_eq!(soundex("1234"), None, "no letters");
}

#[test]
fn levenshtein_counts_edits() {
    assert_eq!(levenshtein("kitten", "sitting"), 3, "the classic pair");
    assert_eq!(levenshtein("", "abc"), 3, "insertions");
    assert_eq!(levenshtein("same", "same"), 0, "equal");
}

#[test]
fn the_name_matches_close_spellings_and_not_other_names() {
    for heard in ["eileen", "aileen", "eilean"] {
        assert!(sounds_like(heard, "eileen"), "{heard} is the name");
    }
    for other in ["ellen", "irene", "alien", "elena"] {
        assert!(!sounds_like(other, "eileen"), "{other} is not");
    }
}

#[test]
fn the_name_is_taken_off_the_start_or_the_end() {
    let led = strip_name("Eileen, what time is it?", &names());
    assert_eq!(led.name, Name::Leads, "name first");
    assert_eq!(led.rest, "what time is it", "the rest");
    let trailed = strip_name("What time is it, Aileen?", &names());
    assert_eq!(trailed.name, Name::Ends, "name last, misheard: it does not lead");
    assert!(trailed.named(), "but it names her");
    assert_eq!(trailed.rest, "what time is it", "the rest");
    let unnamed = strip_name("I told Eileen about it", &names());
    assert_eq!(unnamed.name, Name::Absent, "a mention in the middle does not address her");
    assert!(!unnamed.named(), "not named");
    let aliases = vec!["Eileen".to_owned(), "Ay Lean".to_owned()];
    let alias = strip_name("ay lean stop", &aliases);
    assert_eq!(alias.name, Name::Leads, "a two-word calibration alias");
    assert_eq!(alias.rest, "stop", "the rest");
}

#[test]
fn backchannels_are_short_and_from_the_list() {
    for said in ["mm-hm", "Yeah.", "right", "yeah yeah"] {
        assert!(is_backchannel(said), "{said}");
    }
    for said in ["stop", "yeah but wait", "", "right now please"] {
        assert!(!is_backchannel(said), "{said:?}");
    }
}

#[test]
fn content_words_drop_short_and_stop_words() {
    assert_eq!(
        content_words("I started the render, and the render is at ten percent."),
        ["started", "render", "ten", "percent"],
        "each once"
    );
}

#[test]
fn the_gate_applies_rules_one_three_and_four_in_order() {
    let facts = |named, in_window, command| Facts {
        name: if named { Name::Leads } else { Name::Absent },
        late: false,
        in_window,
        command,
    };
    assert_eq!(decide(facts(true, false, false), || false), Decision::Named, "rule 1");
    assert_eq!(decide(facts(false, true, true), || false), Decision::Window, "rule 3, command");
    assert_eq!(decide(facts(false, true, false), || true), Decision::Window, "rule 3, follow-up");
    assert_eq!(decide(facts(false, true, false), || false), Decision::Greyed, "room talk");
    assert_eq!(decide(facts(false, false, true), || false), Decision::Command, "rule 4");
    assert_eq!(
        decide(facts(false, false, false), || panic!("asked outside the window")),
        Decision::Greyed,
        "the follow-up is asked only inside the window"
    );
    let trailed = Facts { name: Name::Ends, late: false, in_window: false, command: false };
    assert_eq!(decide(trailed, || false), Decision::Named, "rule 1 takes a name at the end");
    assert!(!Decision::Greyed.addressed(), "greyed is not addressed");
    assert!(Decision::Typed.addressed(), "typing is addressed");
}

#[test]
fn a_turn_heard_late_is_addressed_only_when_it_begins_with_the_name() {
    let late = |name, in_window, command| Facts { name, late: true, in_window, command };
    let never = || -> bool { panic!("a turn heard late takes no follow-up") };
    assert_eq!(decide(late(Name::Leads, false, false), never), Decision::NamedLate, "name-led");
    assert_eq!(
        decide(late(Name::Leads, true, true), never),
        Decision::NamedLate,
        "name-led inside the window, a command"
    );
    assert_eq!(
        decide(late(Name::Ends, true, true), never),
        Decision::Late,
        "a name only at the end does not lead"
    );
    assert_eq!(decide(late(Name::Absent, true, true), never), Decision::Late, "not rule 3");
    assert_eq!(decide(late(Name::Absent, false, true), never), Decision::Late, "not rule 4");
    assert_eq!(decide(late(Name::Absent, true, false), never), Decision::Late, "no follow-up");
    assert!(Decision::NamedLate.addressed(), "addressed by the name");
    assert!(!Decision::Late.addressed(), "greyed");
    assert_eq!(
        (Decision::NamedLate.name(), Decision::Late.name()),
        ("name", "greyed"),
        "the span's decision is the rule's, and its reason says the turn was heard late"
    );
    assert!(Decision::Late.reason().contains("heard late"), "{}", Decision::Late.reason());
}

#[test]
fn the_window_is_eight_seconds_or_the_full_window_while_engaged() {
    let mut window = Window::default();
    assert!(!window.is_open(s(0)), "closed at first");
    assert_eq!(window.spoke(s(10), 8, 45), s(18), "eight seconds after Eileen spoke");
    assert!(window.is_open(s(17)), "open");
    assert!(!window.expire(s(17)), "not yet run out");
    assert!(window.expire(s(18)), "run out");
    assert!(!window.is_open(s(18)), "closed");
    window.addressed(s(100));
    assert_eq!(window.spoke(s(110), 8, 45), s(155), "engaged: the full window");
    assert_eq!(window.closes_at(), Some(s(155)), "closes at");
    assert_eq!(window.spoke(s(200), 8, 45), s(208), "no longer engaged");
}

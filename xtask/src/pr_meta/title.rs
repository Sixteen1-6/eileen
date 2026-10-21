//! Check 7: the pull request title is a valid squash subject. It is imperative, capitalised,
//! under 72 characters and has no trailing period, because the squash merge uses it as the commit
//! subject on `main` (owner answer 2).

/// The title must be shorter than this many characters.
pub(crate) const MAX_TITLE_CHARS: usize = 72;

/// Base verbs that end in "ing" and so are not gerunds.
const ING_VERBS: [&str; 14] = [
    "bring", "cling", "ding", "fling", "ping", "ring", "sing", "sling", "spring", "sting",
    "string", "swing", "wing", "wring",
];

/// Base verbs that end in "ed" (beyond those ending in "eed", which are all allowed).
const ED_VERBS: [&str; 2] = ["embed", "shed"];

/// The problems with a subject line, each as `(problem, fix)`. Empty means it is valid.
pub(crate) fn subject_problems(subject: &str) -> Vec<(String, String)> {
    let subject = subject.trim();
    let Some(first_char) = subject.chars().next() else {
        return vec![("the title is empty".to_owned(), "write an imperative summary".to_owned())];
    };
    let mut problems = Vec::new();
    let length = subject.chars().count();
    if length >= MAX_TITLE_CHARS {
        problems.push((
            format!("the title is {length} characters long"),
            format!("shorten it to under {MAX_TITLE_CHARS}; details belong in the description"),
        ));
    }
    if subject.ends_with('.') {
        problems.push((
            "the title ends with a period".to_owned(),
            "remove the final period".to_owned(),
        ));
    }
    if !first_char.is_uppercase() {
        problems.push((
            "the title does not start with a capital letter".to_owned(),
            "start with a capitalised verb, like `Add`, `Fix` or `Move`".to_owned(),
        ));
    }
    let first_word = subject.split_whitespace().next().unwrap_or_default();
    if let Some(form) = non_imperative_form(first_word) {
        problems.push((
            format!("the title starts with `{first_word}`, which reads as {form}"),
            "start with the imperative, as in `Add`, not `Added` or `Adds`".to_owned(),
        ));
    }
    problems
}

/// Names the verb form of a first word that is plainly not the imperative: a gerund, a past
/// tense or a third-person form. This is a heuristic on the word's ending with short lists of
/// base verbs that share those endings, so it errs towards passing.
fn non_imperative_form(word: &str) -> Option<&'static str> {
    let word: String =
        word.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
    if word.len() < 4 {
        return None;
    }
    if word.ends_with("ing") && !ING_VERBS.contains(&word.as_str()) {
        return Some("a gerund");
    }
    if word.ends_with("ed") && !word.ends_with("eed") && !ED_VERBS.contains(&word.as_str()) {
        return Some("the past tense");
    }
    let s_kept = ["ss", "us", "is", "as"].iter().any(|ending| word.ends_with(ending));
    if word.ends_with('s') && !s_kept {
        return Some("the third person");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imperative_capitalised_short_titles_pass() {
        for title in [
            "Add the CI bootstrap and pr-meta",
            "Fix barge-in when the brain is silent",
            "Speed up the Windows lane",
            "Embed the template in the fixture tests",
            "Bring the lockfile up to date",
            "Process scenario files in order",
            "Bump serde from 1.0.229 to 1.0.230",
        ] {
            assert_eq!(subject_problems(title), Vec::new(), "`{title}` is a valid subject");
        }
    }

    #[test]
    fn bad_titles_fail_with_the_reason() {
        let cases = [
            ("add the bootstrap", "capital"),
            ("Add the bootstrap.", "period"),
            ("Added the bootstrap", "past tense"),
            ("Adding the bootstrap", "gerund"),
            ("Adds the bootstrap", "third person"),
            ("", "empty"),
        ];
        for (title, reason) in cases {
            let problems = subject_problems(title);
            assert!(
                problems.iter().any(|(problem, _)| problem.contains(reason)),
                "`{title}` should fail for `{reason}`, got {problems:?}"
            );
        }
    }

    #[test]
    fn title_must_be_under_72_characters() {
        let at_limit = format!("Add {}", "x".repeat(MAX_TITLE_CHARS - 4));
        assert_eq!(at_limit.chars().count(), MAX_TITLE_CHARS, "the fixture is exactly 72 long");
        assert!(
            subject_problems(&at_limit)
                .iter()
                .any(|(problem, _)| problem.contains("72 characters")),
            "72 characters is not under 72"
        );
        let under = format!("Add {}", "x".repeat(MAX_TITLE_CHARS - 5));
        assert_eq!(subject_problems(&under), Vec::new(), "71 characters passes");
    }
}

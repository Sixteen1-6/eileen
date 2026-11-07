//! Which unfinished job a request names (BUILD.md section 2.5's pronoun rules), first match
//! wins: an explicit id, slug, ordinal or name; a kind word; a route word; then, for a pronoun
//! or no words at all, the job just announced, the focus job and the most recent.

use eileen_contracts::ids::JobId;
use eileen_contracts::jobs::{Job, JobKind, Route};
use eileen_contracts::tools::brain::JobRef;

use super::{ARTICLES, Pick, Registry};

impl Registry {
    /// The unfinished job `target` names, the newest first: by id, by slug, by spoken name in
    /// any case, or by a fan-out child's ordinal. A job with no ordinal is never "the first
    /// one".
    #[must_use]
    pub fn resolve(&self, target: &JobRef) -> Option<JobId> {
        let mut open = self.jobs.iter().rev().filter(|job| !job.status.is_terminal());
        let found = match target {
            JobRef::Id(id) => open.find(|job| job.id == *id),
            JobRef::Slug(slug) => open.find(|job| job.slug == *slug),
            JobRef::Name(name) => open.find(|job| job.name.eq_ignore_ascii_case(name.trim())),
            JobRef::Ordinal(ordinal) => open.find(|job| job.ordinal == Some(*ordinal)),
        };
        found.map(|job| job.id)
    }

    /// Which unfinished job a request names (section 2.5), first match wins: an explicit id,
    /// slug, ordinal or name (its words with the articles dropped, in any case); a kind word
    /// ("the render", "the coding job"); a route word ("Claude's job"); then, for a pronoun or
    /// no words at all, the job announced just before (`announced`, which the caller gives only
    /// right after an announcement), the focus job and the most recent job outside a fan-out.
    /// A name, kind word or route word that fits several jobs picks the focus job among them,
    /// or asks which. Kind and route words name only jobs outside a fan-out, whose children are
    /// named by ordinal.
    #[must_use]
    pub fn pick(&self, target: Option<&JobRef>, announced: Option<JobId>) -> Pick {
        let words = match target {
            None => Vec::new(),
            Some(JobRef::Name(name)) => plain_words(name),
            Some(other) => return self.resolve(other).map_or(Pick::Nothing, Pick::One),
        };
        let open = |job: &&Job| !job.status.is_terminal();
        let top = |job: &&Job| open(job) && job.spec.parent.is_none();
        if !words.is_empty() && !PRONOUNS.contains(&words.join(" ").as_str()) {
            let named: Vec<&Job> = self
                .jobs
                .iter()
                .rev()
                .filter(open)
                .filter(|j| plain_words(&j.name) == words)
                .collect();
            if !named.is_empty() {
                return choose(&named);
            }
            if let Some(kind) = kind_named(&words) {
                let found: Vec<&Job> =
                    self.jobs.iter().rev().filter(top).filter(|j| j.spec.kind == kind).collect();
                return choose(&found);
            }
            if let Some(route) = route_named(&words) {
                let found: Vec<&Job> =
                    self.jobs.iter().rev().filter(top).filter(|j| j.route == route).collect();
                return choose(&found);
            }
            return Pick::Nothing;
        }
        let announced = announced.filter(|id| self.get(*id).is_some_and(|job| open(&job)));
        let newest = || self.jobs.iter().rev().find(top).map(|job| job.id);
        announced.or_else(|| self.focused()).or_else(newest).map_or(Pick::Nothing, Pick::One)
    }
}

/// Words that name no job by themselves, once the articles and `FILLER` are dropped: the
/// pronoun rules decide.
const PRONOUNS: [&str; 5] = ["it", "that", "this", "that one", "this one"];

/// Words dropped before a name, kind word or route word is matched.
const FILLER: [&str; 5] = ["job", "jobs", "task", "please", "now"];

/// One job of `found` (newest first): the only one, or the focus job among several; otherwise
/// a question.
fn choose(found: &[&Job]) -> Pick {
    match found {
        [] => Pick::Nothing,
        [one] => Pick::One(one.id),
        many => many.iter().find(|job| job.focus).map_or_else(
            || Pick::Ask(many.iter().map(|job| job.id).collect()),
            |focus| Pick::One(focus.id),
        ),
    }
}

/// `text` in lower-case ASCII words, with a possessive "'s", the articles and `FILLER`
/// dropped: "Claude's job" is `["claude"]`, "The Render" is `["render"]`.
pub(super) fn plain_words(text: &str) -> Vec<String> {
    let text = text.replace('\u{2019}', "'").to_lowercase();
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.strip_suffix("'s").unwrap_or(word).trim_matches('\'').to_owned())
        .filter(|word| !word.is_empty())
        .filter(|word| !ARTICLES.contains(&word.as_str()) && !FILLER.contains(&word.as_str()))
        .collect()
}

/// The job kind `words` name, when they are one of its kind words.
pub(super) fn kind_named(words: &[String]) -> Option<JobKind> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["render" | "renders" | "rendering" | "export"] => Some(JobKind::AppTask),
        ["coding" | "code" | "coder"] => Some(JobKind::Coding),
        ["research" | "researcher"] => Some(JobKind::Research),
        ["computer"] => Some(JobKind::ComputerUse),
        ["answer"] | ["deep", "answer"] => Some(JobKind::DeepAnswer),
        _ => None,
    }
}

/// The route `words` name, when they are its spoken name.
pub(super) fn route_named(words: &[String]) -> Option<Route> {
    let said = words.join(" ");
    Route::ALL.into_iter().find(|route| route.display_name().to_lowercase() == said)
}

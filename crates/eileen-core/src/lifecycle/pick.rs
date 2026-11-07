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

    /// Which unfinished job a request names (section 2.5), first match wins: an explicit id or
    /// slug; an ordinal or a name (its words with the articles dropped, in any case); a kind
    /// word ("the render", "the coding job"); a route word ("Claude's job"); then, for a
    /// pronoun or no words at all, the job announced just before (`announced`, which the
    /// caller gives only right after an announcement), the focus job and the most recent job
    /// outside a fan-out. An ordinal, name, kind word or route word that fits several jobs
    /// picks the one in focus among them (the focus job, a child of it, or a sibling of a
    /// focused child), or asks which. Kind and route words name only jobs outside a fan-out,
    /// whose children are named by ordinal, and a route word never names an app's own work or
    /// an action, which no route runs.
    #[must_use]
    pub fn pick(&self, target: Option<&JobRef>, announced: Option<JobId>) -> Pick {
        let open = |job: &&Job| !job.status.is_terminal();
        let top = |job: &&Job| open(job) && job.spec.parent.is_none();
        let focus = self.focused().and_then(|id| self.get(id));
        let words = match target {
            None => Vec::new(),
            Some(JobRef::Name(name)) => plain_words(name),
            Some(JobRef::Ordinal(ordinal)) => {
                let found: Vec<&Job> = self
                    .jobs
                    .iter()
                    .rev()
                    .filter(open)
                    .filter(|j| j.ordinal == Some(*ordinal))
                    .collect();
                return choose(&found, focus);
            }
            Some(other) => return self.resolve(other).map_or(Pick::Nothing, Pick::One),
        };
        if !words.is_empty() && !PRONOUNS.contains(&words.join(" ").as_str()) {
            let named: Vec<&Job> = self
                .jobs
                .iter()
                .rev()
                .filter(open)
                .filter(|j| plain_words(&j.name) == words)
                .collect();
            if !named.is_empty() {
                return choose(&named, focus);
            }
            if let Some(kind) = kind_named(&words) {
                let found: Vec<&Job> =
                    self.jobs.iter().rev().filter(top).filter(|j| j.spec.kind == kind).collect();
                return choose(&found, focus);
            }
            if let Some(route) = route_named(&words) {
                let runs = |j: &&Job| !matches!(j.spec.kind, JobKind::AppTask | JobKind::Act);
                let found: Vec<&Job> = self
                    .jobs
                    .iter()
                    .rev()
                    .filter(top)
                    .filter(runs)
                    .filter(|j| j.route == route)
                    .collect();
                return choose(&found, focus);
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

/// One job of `found` (newest first): the only one, or among several the one in focus, which
/// is the focus job `focus`, a child of it, its parent, or a sibling of it in a fan-out;
/// otherwise a question.
fn choose(found: &[&Job], focus: Option<&Job>) -> Pick {
    let in_focus = |job: &&&Job| {
        focus.is_some_and(|focus| {
            job.id == focus.id
                || job.spec.parent == Some(focus.id)
                || focus.spec.parent == Some(job.id)
                || (focus.spec.parent.is_some() && job.spec.parent == focus.spec.parent)
        })
    };
    match found {
        [] => Pick::Nothing,
        [one] => Pick::One(one.id),
        many => many.iter().find(in_focus).map_or_else(
            || Pick::Ask(many.iter().map(|job| job.id).collect()),
            |chosen| Pick::One(chosen.id),
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

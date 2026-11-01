//! Announcements in their minimal form (BUILD.md section 2.5, "Announcements"): a job's
//! completion, or its failure, waits in a bounded queue and is spoken, attributed to the route
//! that did the work, once the user and Eileen have been quiet for a second.

use std::collections::VecDeque;

use eileen_contracts::ids::{JobId, MonoTime};

/// The most announcements held; a new one past it drops the oldest (invariant 10).
pub(crate) const CAPACITY: usize = 4;

/// How long the conversation must be quiet before an announcement, ns ("end of the user's
/// turn plus 1 s of silence").
pub(crate) const QUIET_NS: u64 = 1_000_000_000;

/// The most characters of a worker's summary spoken.
const SUMMARY_CHARS: usize = 160;

/// One line waiting to be spoken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Announcement {
    /// The job it is about.
    pub(crate) job: JobId,
    /// What Eileen says.
    pub(crate) text: String,
}

/// When the next announcement may be spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Due {
    /// Nothing waits, or the conversation is not quiet.
    Never,
    /// Now.
    Now,
    /// At this instant, if it stays quiet.
    At(MonoTime),
}

/// The bounded queue and how long the conversation has been quiet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Queue {
    items: VecDeque<Announcement>,
    quiet_since: Option<MonoTime>,
}

impl Queue {
    /// Queues an announcement, dropping the oldest when full.
    pub(crate) fn push(&mut self, announcement: Announcement) {
        while self.items.len() >= CAPACITY {
            self.items.pop_front();
        }
        self.items.push_back(announcement);
    }

    /// Records whether the conversation is quiet at `now`.
    pub(crate) const fn note_quiet(&mut self, quiet: bool, now: MonoTime) {
        if quiet {
            if self.quiet_since.is_none() {
                self.quiet_since = Some(now);
            }
        } else {
            self.quiet_since = None;
        }
    }

    /// When the next announcement may be spoken.
    pub(crate) fn due(&self, now: MonoTime) -> Due {
        let Some(since) = self.quiet_since.filter(|_| !self.items.is_empty()) else {
            return Due::Never;
        };
        let at = MonoTime(since.0.saturating_add(QUIET_NS));
        if now >= at { Due::Now } else { Due::At(at) }
    }

    /// Takes the next announcement.
    pub(crate) fn pop(&mut self) -> Option<Announcement> {
        self.items.pop_front()
    }
}

/// What Eileen says when a job completes: its name, then the worker's summary attributed to
/// the route ("Claude says: ..."), cut to its first sentence.
pub(crate) fn completed(name: &str, route: &str, summary: &str) -> String {
    let summary = first_sentence(summary);
    if summary.is_empty() {
        format!("{} is done.", capitalised(name))
    } else {
        format!("{} is done. {route} says: {summary}", capitalised(name))
    }
}

/// What Eileen says when a job fails.
pub(crate) fn failed(name: &str, why: &str) -> String {
    format!("{} failed: {why}.", capitalised(name))
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// The first sentence of `text`, at most `SUMMARY_CHARS` characters, cut at a word, ending
/// with a full stop.
fn first_sentence(text: &str) -> String {
    let text = text.trim();
    let mut sentence = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            break;
        }
        sentence.push(c);
        let ends = chars.peek().is_none_or(|next| next.is_whitespace());
        if matches!(c, '.' | '!' | '?') && ends {
            break;
        }
    }
    let mut sentence: String = sentence.trim().to_owned();
    if sentence.chars().count() > SUMMARY_CHARS {
        let cut: String = sentence.chars().take(SUMMARY_CHARS).collect();
        sentence = match cut.rsplit_once(' ') {
            Some((head, _)) => head.to_owned(),
            None => cut,
        };
    }
    if !sentence.is_empty() && !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    sentence
}

#[cfg(test)]
mod tests {
    use eileen_contracts::ids::{JobId, MonoTime};

    use super::{Announcement, CAPACITY, Due, QUIET_NS, Queue, completed, failed};

    fn line(n: u64) -> Announcement {
        Announcement { job: JobId::from_parts(n, u128::from(n)), text: format!("line {n}") }
    }

    #[test]
    fn the_queue_is_bounded_and_drops_the_oldest() {
        let mut queue = Queue::default();
        for n in 0..10 {
            queue.push(line(n));
        }
        let kept: Vec<Announcement> = std::iter::from_fn(|| queue.pop()).collect();
        assert_eq!(kept.len(), CAPACITY, "bounded");
        assert_eq!(kept.first(), Some(&line(6)), "the newest four are kept, oldest first");
    }

    #[test]
    fn an_announcement_waits_for_a_second_of_quiet() {
        let mut queue = Queue::default();
        queue.note_quiet(true, MonoTime(0));
        assert_eq!(queue.due(MonoTime(0)), Due::Never, "nothing waits");
        queue.push(line(1));
        assert_eq!(
            queue.due(MonoTime(10)),
            Due::At(MonoTime(QUIET_NS)),
            "a second after quiet began"
        );
        queue.note_quiet(false, MonoTime(500));
        assert_eq!(queue.due(MonoTime(600)), Due::Never, "the user spoke");
        queue.note_quiet(true, MonoTime(700));
        queue.note_quiet(true, MonoTime(800));
        assert_eq!(queue.due(MonoTime(700 + QUIET_NS)), Due::Now, "quiet for a second since 700");
    }

    #[test]
    fn a_completion_is_attributed_and_cut_to_one_sentence() {
        assert_eq!(
            completed("the hello file", "Claude", "Created hello.txt with hi. Then I checked it."),
            "The hello file is done. Claude says: Created hello.txt with hi.",
            "the first sentence, attributed"
        );
        assert_eq!(completed("the job", "Claude", "  "), "The job is done.", "no summary");
        assert_eq!(
            completed("the job", "Claude", "made a file"),
            "The job is done. Claude says: made a file.",
            "a full stop is added"
        );
        assert_eq!(
            failed("the job", "the worker stopped"),
            "The job failed: the worker stopped.",
            "failure"
        );
    }
}

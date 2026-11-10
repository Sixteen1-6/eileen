//! The addressee gate in its minimal form (BUILD.md section 2.8, rules 1, 3 and 4): the name,
//! the conversation window, and a full built-in command. The score of rule 5, the speech
//! level, suspension while other audio plays and the call rule come with the full core.
//!
//! The name matches phonetically: a word matches when its Soundex code equals the name's
//! (two leading vowels count as one letter, since recognizers swap them) and its normalised
//! Levenshtein distance from the name is under one third, so "Aileen" is Eileen and "Ellen"
//! is not. The calibration's name aliases match the same way.

use eileen_contracts::ids::MonoTime;

/// Words that, said alone during a barge-in, are a backchannel and resume playback.
const BACKCHANNELS: &[&str] =
    &["mm-hm", "mhm", "mm", "mmm", "uh-huh", "yeah", "yep", "right", "okay", "ok", "sure", "uh"];

/// Words left out of the content words of Eileen's last turn.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "but", "for", "from", "has", "have", "her", "his", "into", "its",
    "it's", "not", "now", "of", "on", "or", "our", "she", "that", "the", "their", "them", "then",
    "there", "they", "this", "to", "was", "were", "what", "when", "with", "you", "your",
];

/// Nanoseconds in a second.
const NS_PER_S: u64 = 1_000_000_000;

/// An utterance's words, lower case, with punctuation stripped from each end of each word
/// (an apostrophe or hyphen inside a word stays).
pub(crate) fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|word| !word.is_empty())
        .collect()
}

/// An utterance with the assistant's name taken off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stripped {
    /// Whether it began or ended with the name (rule 1).
    pub(crate) named: bool,
    /// Its other words, lower case, for the resolver.
    pub(crate) rest: String,
}

/// Takes the name, or one of its aliases, off the start or the end of `text`.
pub(crate) fn strip_name(text: &str, names: &[String]) -> Stripped {
    let said = words(text);
    for name in names {
        let name_words = words(name);
        let count = name_words.len();
        if count == 0 || said.len() < count {
            continue;
        }
        let lead = said.get(..count).is_some_and(|lead| matches_name(lead, &name_words));
        if lead {
            return Stripped { named: true, rest: said.get(count..).unwrap_or_default().join(" ") };
        }
        let tail_at = said.len() - count;
        let tail = said.get(tail_at..).is_some_and(|tail| matches_name(tail, &name_words));
        if tail {
            return Stripped {
                named: true,
                rest: said.get(..tail_at).unwrap_or_default().join(" "),
            };
        }
    }
    Stripped { named: false, rest: said.join(" ") }
}

fn matches_name(said: &[String], name: &[String]) -> bool {
    said.len() == name.len() && said.iter().zip(name).all(|(word, part)| sounds_like(word, part))
}

/// Whether `word` sounds like `name`: equal, or the same Soundex code and a normalised
/// Levenshtein distance under one third.
pub(crate) fn sounds_like(word: &str, name: &str) -> bool {
    if word == name {
        return true;
    }
    let (Some(a), Some(b)) = (soundex(word), soundex(name)) else { return false };
    let longest = word.chars().count().max(name.chars().count());
    a == b && levenshtein(word, name).saturating_mul(3) < longest
}

/// The Soundex code of a word's letters: its first letter (any vowel counts as `a`) and
/// three digits. `None` for a word with no letters.
pub(crate) fn soundex(word: &str) -> Option<(char, [u8; 3])> {
    let mut letters =
        word.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_lowercase());
    let first = letters.next()?;
    let lead = if is_vowel(first) { 'a' } else { first };
    let mut digits = [0_u8; 3];
    let mut filled = 0_usize;
    let mut last = code(first);
    for letter in letters {
        let digit = code(letter);
        if digit != 0
            && digit != last
            && let Some(slot) = digits.get_mut(filled)
        {
            *slot = digit;
            filled += 1;
        }
        if letter != 'h' && letter != 'w' {
            last = digit;
        }
    }
    Some((lead, digits))
}

const fn is_vowel(letter: char) -> bool {
    matches!(letter, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// A letter's Soundex digit; 0 for vowels, `h`, `w` and anything else.
const fn code(letter: char) -> u8 {
    match letter {
        'b' | 'f' | 'p' | 'v' => 1,
        'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => 2,
        'd' | 't' => 3,
        'l' => 4,
        'm' | 'n' => 5,
        'r' => 6,
        _ => 0,
    }
}

/// The Levenshtein distance between two words, by characters.
pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row.first().copied().unwrap_or(i);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let above = row.get(j + 1).copied().unwrap_or(usize::MAX);
            let left = row.get(j).copied().unwrap_or(usize::MAX);
            let cost = usize::from(ca != *cb);
            let best = diagonal
                .saturating_add(cost)
                .min(above.saturating_add(1))
                .min(left.saturating_add(1));
            diagonal = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = best;
            }
        }
    }
    row.last().copied().unwrap_or(0)
}

/// Whether the words, said alone, are a backchannel ("mm-hm", "yeah", "right").
pub(crate) fn is_backchannel(rest: &str) -> bool {
    let said = words(rest);
    !said.is_empty() && said.len() <= 2 && said.iter().all(|w| BACKCHANNELS.contains(&w.as_str()))
}

/// The content words of something Eileen said, for the follow-up feature: lower case, short
/// and stop words dropped, each once, at most 24.
pub(crate) fn content_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in words(text) {
        let keep = word.chars().count() > 2 && !STOP_WORDS.contains(&word.as_str());
        if keep && !out.contains(&word) && out.len() < 24 {
            out.push(word);
        }
    }
    out
}

/// What the gate decided, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Rule 1: it began or ended with the name.
    Named,
    /// Rule 3: inside the conversation window, a full built-in command or a follow-up that
    /// resolves against Eileen's last turn.
    Window,
    /// Rule 4: a full built-in command, outside the window.
    Command,
    /// Typed in Eileen's own UI or sent from the command line: no gate.
    Typed,
    /// Nothing addressed it: shown greyed, never stored, never a bus event.
    Greyed,
}

impl Decision {
    /// Whether the turn was addressed.
    pub(crate) const fn addressed(self) -> bool {
        !matches!(self, Self::Greyed)
    }

    /// The decision as the `addressee` span records it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Named => "name",
            Self::Window => "window",
            Self::Command => "command",
            Self::Typed => "typed",
            Self::Greyed => "greyed",
        }
    }

    /// Why, in a few words, for the `addressee` span.
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::Named => "it began or ended with the name",
            Self::Window => "inside the conversation window",
            Self::Command => "a full built-in command",
            Self::Typed => "typed or sent, not spoken",
            Self::Greyed => "not addressed",
        }
    }
}

/// What the gate knows about one spoken utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    /// It began or ended with the name.
    pub(crate) named: bool,
    /// The conversation window is open (or Eileen is speaking).
    pub(crate) in_window: bool,
    /// It is a full match of a built-in command.
    pub(crate) command: bool,
}

/// Rules 1, 3 and 4, first match wins. `follow_up` is asked only inside the window, for an
/// unnamed utterance that is not a command.
pub(crate) fn decide(facts: Facts, follow_up: impl FnOnce() -> bool) -> Decision {
    if facts.named {
        Decision::Named
    } else if facts.in_window && (facts.command || follow_up()) {
        Decision::Window
    } else if facts.command {
        Decision::Command
    } else {
        Decision::Greyed
    }
}

/// The conversation window: open 8 s (`conversation.follow_up_s`) after Eileen spoke, and
/// `conversation.window_s` after it while the user is engaged (they addressed her within that
/// long).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Window {
    until: Option<MonoTime>,
    last_addressed: Option<MonoTime>,
}

impl Window {
    /// A window resumed from a snapshot.
    pub(crate) const fn resumed(until: Option<MonoTime>) -> Self {
        Self { until, last_addressed: None }
    }

    /// When it closes (or closed), for the snapshot.
    pub(crate) const fn closes_at(&self) -> Option<MonoTime> {
        self.until
    }

    /// Whether it is open at `now`.
    pub(crate) fn is_open(&self, now: MonoTime) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// The user addressed Eileen at `now`.
    pub(crate) const fn addressed(&mut self, now: MonoTime) {
        self.last_addressed = Some(now);
    }

    /// Eileen finished speaking at `now`: the window opens, or stays open, until the instant
    /// returned.
    pub(crate) fn spoke(&mut self, now: MonoTime, follow_up_s: u16, window_s: u16) -> MonoTime {
        let full_ns = u64::from(window_s).saturating_mul(NS_PER_S);
        let engaged = self.last_addressed.is_some_and(|at| now.0.saturating_sub(at.0) <= full_ns);
        let seconds = if engaged { window_s.max(follow_up_s) } else { follow_up_s };
        let until = MonoTime(now.0.saturating_add(u64::from(seconds).saturating_mul(NS_PER_S)));
        let until = self.until.filter(|old| *old > until && now < *old).unwrap_or(until);
        self.until = Some(until);
        until
    }

    /// Closes it if it has run out by `now`; true when it closed.
    pub(crate) fn expire(&mut self, now: MonoTime) -> bool {
        if self.until.is_some_and(|until| until <= now) {
            self.until = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests;

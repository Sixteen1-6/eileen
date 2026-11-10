//! Cutting the brain's streamed text into clauses to speak (BUILD.md section 2.8, the TTS row,
//! and section 2.3, "Turn output"): a first clause of three to six words, so speech starts
//! early, then whole clauses. A line starting with `---` (`---hud---`, `---ignore---`) ends the
//! spoken part; what follows it is for the screen.

/// Words after which the first clause is cut even with no punctuation.
const FIRST_MAX_WORDS: usize = 6;
/// Words a first clause needs before a comma may end it.
const FIRST_MIN_WORDS: usize = 3;
/// Words a later clause needs before a comma may end it.
const LATER_COMMA_WORDS: usize = 6;
/// Words after which a later clause is cut even with no punctuation.
const LATER_MAX_WORDS: usize = 30;
/// The most text held back waiting for a boundary, in bytes; past it the text is cut anyway.
const MAX_BUFFER: usize = 4_096;

/// Streamed text in, clauses out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Cutter {
    buffer: String,
    /// A clause has been cut already.
    started: bool,
    /// A `---` line ended the spoken part.
    ended: bool,
}

impl Cutter {
    /// Adds streamed text and returns the clauses it completed.
    pub(crate) fn push(&mut self, text: &str) -> Vec<String> {
        if self.ended {
            return Vec::new();
        }
        self.buffer.push_str(text);
        if let Some(at) = marker(&self.buffer) {
            self.buffer.truncate(at);
            self.ended = true;
        }
        let mut clauses = Vec::new();
        while let Some(at) = self.next_cut() {
            let Some((head, rest)) = self.buffer.split_at_checked(at) else { break };
            let clause = head.trim().to_owned();
            let keep_from = self.buffer.len() - rest.trim_start().len();
            self.buffer.replace_range(..keep_from, "");
            if !clause.is_empty() {
                self.started = true;
                clauses.push(clause);
            }
        }
        clauses
    }

    /// The text left at the end of the turn, as a last clause.
    pub(crate) fn finish(&mut self) -> Option<String> {
        let held = self.held_marker().unwrap_or(self.buffer.len());
        let rest = self.buffer.get(..held).unwrap_or_default().trim().to_owned();
        self.buffer.clear();
        self.ended = true;
        (!rest.is_empty()).then_some(rest)
    }

    /// Where the next clause ends, if one is complete.
    fn next_cut(&self) -> Option<usize> {
        let limit = self.held_marker().unwrap_or(self.buffer.len());
        let text = self.buffer.get(..limit)?;
        let (comma_words, max_words) = if self.started {
            (LATER_COMMA_WORDS, LATER_MAX_WORDS)
        } else {
            (FIRST_MIN_WORDS, FIRST_MAX_WORDS)
        };
        let mut words = 0_usize;
        let mut in_word = false;
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            let following = chars.peek().map(|(_, n)| *n);
            if c.is_whitespace() {
                if in_word {
                    words += 1;
                    in_word = false;
                    if words >= max_words {
                        return Some(at);
                    }
                }
                if c == '\n' {
                    return Some(at + 1);
                }
                continue;
            }
            in_word = true;
            let followed_by_space = following.is_some_and(char::is_whitespace);
            let end = at + c.len_utf8();
            if followed_by_space && matches!(c, '.' | '!' | '?' | ';' | ':') {
                return Some(end);
            }
            if followed_by_space && c == ',' && words + 1 >= comma_words {
                return Some(end);
            }
        }
        (text.len() > MAX_BUFFER).then_some(limit)
    }

    /// Where a possible `---` marker starts in the held text: a line that begins with `-`
    /// waits until it is clearly not a marker.
    fn held_marker(&self) -> Option<usize> {
        let line_start = self.buffer.rfind('\n').map_or(0, |at| at + 1);
        let line = self.buffer.get(line_start..)?;
        (line.starts_with('-') && line.len() < 3).then_some(line_start)
    }
}

/// Where a line starting with `---` begins.
fn marker(text: &str) -> Option<usize> {
    if text.starts_with("---") {
        return Some(0);
    }
    text.find("\n---").map(|at| at + 1)
}

#[cfg(test)]
mod tests {
    use super::Cutter;

    #[test]
    fn the_first_clause_is_three_to_six_words() {
        let mut cutter = Cutter::default();
        assert_eq!(
            cutter.push("It is a quarter past three in the afternoon"),
            ["It is a quarter past three"],
            "cut after six words with no punctuation"
        );
        let mut cutter = Cutter::default();
        assert_eq!(
            cutter.push("Yes, I can, "),
            ["Yes, I can,"],
            "a comma ends it from three words"
        );
        let mut cutter = Cutter::default();
        assert_eq!(cutter.push("Sure. And"), ["Sure."], "a full stop ends it at once");
    }

    #[test]
    fn later_clauses_end_at_punctuation_or_a_newline() {
        let mut cutter = Cutter::default();
        let mut clauses = cutter.push("Hello there. The ");
        clauses.extend(cutter.push("render is at half, and it should finish soon. Then"));
        clauses.extend(cutter.finish());
        assert_eq!(
            clauses,
            ["Hello there.", "The render is at half, and it should finish soon.", "Then"],
            "a short comma clause stays whole; the rest comes at the end"
        );
    }

    #[test]
    fn deltas_split_anywhere_give_the_same_clauses() {
        let text = "First of all, thanks. Second: the job is done.\nThird line here";
        let mut whole = Cutter::default();
        let mut expected = whole.push(text);
        expected.extend(whole.finish());
        let mut pieces = Cutter::default();
        let mut got = Vec::new();
        for c in text.chars() {
            got.extend(pieces.push(&c.to_string()));
        }
        got.extend(pieces.finish());
        assert_eq!(got, expected, "one character at a time");
    }

    #[test]
    fn a_marker_line_ends_the_spoken_part() {
        let mut cutter = Cutter::default();
        let mut clauses = cutter.push("Here it is.\n--");
        clauses.extend(cutter.push("-hud---\n# Table"));
        clauses.extend(cutter.push(" more"));
        clauses.extend(cutter.finish());
        assert_eq!(clauses, ["Here it is."], "nothing after the marker is spoken");
        let mut ignored = Cutter::default();
        assert!(ignored.push("---ignore---").is_empty(), "an ignore answer says nothing");
        assert_eq!(ignored.finish(), None, "nothing at the end either");
    }
}

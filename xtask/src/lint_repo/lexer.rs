//! A small Rust lexer for the code checks: it separates code from comments and string and
//! character literals, so a `Command::new` in a doc comment or an `#[ignore]` inside a string
//! does not count as code, while check 6 still reads doc comments and strings.
//!
//! [`lex`] returns the code with every comment and literal body blanked (newlines kept, so
//! byte offsets and lines stay put and braces inside literals never count), and the comments
//! and string literals themselves with the line each starts on.

/// What a piece of text outside the code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKind {
    /// A `//` or `/* */` comment that is not a doc comment.
    Comment,
    /// A `///`, `//!`, `/** */` or `/*! */` doc comment.
    Doc,
    /// A string literal of any form (`"..."`, `b"..."`, `c"..."`, raw), delimiters included.
    Str,
}

/// A comment or a string literal, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Text {
    /// Its kind.
    pub(crate) kind: TextKind,
    /// The text as written, markers and quotes included.
    pub(crate) body: String,
    /// The 1-based line it starts on.
    pub(crate) line: usize,
}

/// A source file, lexed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Lexed {
    /// The source with every comment and literal body blanked to spaces, newlines kept.
    pub(crate) code: String,
    /// The comments and string literals, in order.
    pub(crate) texts: Vec<Text>,
}

/// Lexes `source`. Malformed input (an unterminated literal or comment) runs to the end of the
/// file, which is what the compiler would reject anyway.
pub(crate) fn lex(source: &str) -> Lexed {
    let chars: Vec<char> = source.chars().collect();
    let mut lexer = Lexer { chars: &chars, at: 0, line: 1, out: Lexed::default() };
    lexer.run();
    lexer.out
}

struct Lexer<'a> {
    chars: &'a [char],
    at: usize,
    line: usize,
    out: Lexed,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match (c, self.peek(1)) {
                ('/', Some('/')) => self.line_comment(),
                ('/', Some('*')) => self.block_comment(),
                ('"', _) => self.string(0, 0),
                ('\'', _) => self.char_or_lifetime(),
                ('b' | 'c' | 'r', _) if !self.after_ident() => self.maybe_prefixed_string(c),
                _ => self.code(c),
            }
        }
    }

    /// Whether the character before the cursor continues an identifier, so a `b`, `c` or `r`
    /// here is part of a name rather than a literal's prefix.
    fn after_ident(&self) -> bool {
        self.at
            .checked_sub(1)
            .and_then(|before| self.chars.get(before))
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    }

    fn code(&mut self, c: char) {
        self.out.code.push(c);
        if c == '\n' {
            self.line += 1;
        }
        self.at += 1;
    }

    /// Takes `count` characters as the body of a comment or literal: they go to the text and
    /// are blanked in the code.
    fn take(&mut self, count: usize, text: &mut String) {
        for _ in 0..count {
            let Some(c) = self.peek(0) else { return };
            text.push(c);
            if c == '\n' {
                self.out.code.push('\n');
                self.line += 1;
            } else {
                self.out.code.push(' ');
            }
            self.at += 1;
        }
    }

    fn push(&mut self, kind: TextKind, text: String, line: usize) {
        self.out.texts.push(Text { kind, body: text, line });
    }

    fn line_comment(&mut self) {
        let line = self.line;
        let mut text = String::new();
        while self.peek(0).is_some_and(|c| c != '\n') {
            self.take(1, &mut text);
        }
        let doc = (text.starts_with("///") && !text.starts_with("////")) || text.starts_with("//!");
        self.push(if doc { TextKind::Doc } else { TextKind::Comment }, text, line);
    }

    fn block_comment(&mut self) {
        let line = self.line;
        let mut text = String::new();
        self.take(2, &mut text);
        let mut depth = 1_usize;
        while depth > 0 {
            match (self.peek(0), self.peek(1)) {
                (None, _) => break,
                (Some('/'), Some('*')) => {
                    depth += 1;
                    self.take(2, &mut text);
                }
                (Some('*'), Some('/')) => {
                    depth -= 1;
                    self.take(2, &mut text);
                }
                _ => self.take(1, &mut text),
            }
        }
        let doc = (text.starts_with("/**") && !text.starts_with("/***") && text != "/**/")
            || text.starts_with("/*!");
        self.push(if doc { TextKind::Doc } else { TextKind::Comment }, text, line);
    }

    /// A `b`, `c` or `r` that may start a byte, C or raw string (`br`, `cr` included).
    fn maybe_prefixed_string(&mut self, first: char) {
        let prefix = if first != 'r' && self.peek(1) == Some('r') { 2 } else { 1 };
        let raw = first == 'r' || prefix == 2;
        if raw {
            let mut hashes = 0;
            while self.peek(prefix + hashes) == Some('#') {
                hashes += 1;
            }
            if self.peek(prefix + hashes) == Some('"') {
                self.string(prefix, hashes + 1);
                return;
            }
        } else if self.peek(1) == Some('"') {
            self.string(1, 0);
            return;
        } else if first == 'b' && self.peek(1) == Some('\'') {
            self.code('b');
            self.char_or_lifetime();
            return;
        }
        self.code(first);
    }

    /// A string literal starting at the cursor: `prefix` characters before the hashes and
    /// quote, then for a raw string `raw - 1` hashes (`raw` is 0 for an escaped string).
    fn string(&mut self, prefix: usize, raw: usize) {
        let line = self.line;
        let mut text = String::new();
        let hashes = raw.saturating_sub(1);
        // The prefix, the hashes and the opening quote stay visible in the code.
        for _ in 0..=prefix + hashes {
            if let Some(c) = self.peek(0) {
                text.push(c);
                self.code(c);
            }
        }
        loop {
            match self.peek(0) {
                None => break,
                Some('\\') if raw == 0 => self.take(2, &mut text),
                Some('"') if (1..=hashes).all(|n| self.peek(n) == Some('#')) => {
                    for _ in 0..=hashes {
                        if let Some(c) = self.peek(0) {
                            text.push(c);
                            self.code(c);
                        }
                    }
                    break;
                }
                Some(_) => self.take(1, &mut text),
            }
        }
        self.push(TextKind::Str, text, line);
    }

    /// A character literal (`'a'`, `'\n'`, `'\u{2a}'`) is blanked like a string; a lifetime
    /// or label (`'a`) is code.
    fn char_or_lifetime(&mut self) {
        let literal = match (self.peek(1), self.peek(2)) {
            (Some('\\'), _) => true,
            (Some(c), Some('\'')) => c != '\'',
            _ => false,
        };
        if !literal {
            self.code('\'');
            return;
        }
        let mut body = String::new();
        self.code('\'');
        loop {
            match self.peek(0) {
                None | Some('\n') => break,
                Some('\\') => self.take(2, &mut body),
                Some('\'') => {
                    self.code('\'');
                    break;
                }
                Some(_) => self.take(1, &mut body),
            }
        }
    }
}

/// Whether `c` can be part of an identifier.
pub(crate) fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The byte offsets in `code` where `word` occurs as a whole token: not inside a longer
/// identifier on either side.
pub(crate) fn find_word(code: &str, word: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = code.get(from..).and_then(|rest| rest.find(word)) {
        let start = from + at;
        let end = start + word.len();
        let before = code.get(..start).and_then(|s| s.chars().next_back());
        let after = code.get(end..).and_then(|s| s.chars().next());
        let starts_word = word.chars().next().is_some_and(|c| !is_ident(c))
            || before.is_none_or(|c| !is_ident(c));
        let ends_word = word.chars().next_back().is_some_and(|c| !is_ident(c))
            || after.is_none_or(|c| !is_ident(c));
        if starts_word && ends_word {
            found.push(start);
        }
        from = end;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{TextKind, find_word, lex};

    #[test]
    fn comments_and_strings_are_blanked_in_the_code_with_lines_kept() {
        let source = "let a = \"x { y\"; // c }\n/* b\n} */ fn f() {}\n";
        let lexed = lex(source);
        assert_eq!(lexed.code.len(), source.len(), "same length");
        assert_eq!(lexed.code.matches('{').count(), 1, "only the code's brace: {:?}", lexed.code);
        assert_eq!(lexed.code.matches('\n').count(), 3, "newlines kept");
        let kinds: Vec<_> = lexed.texts.iter().map(|t| (t.kind, t.line)).collect();
        assert_eq!(
            kinds,
            [(TextKind::Str, 1), (TextKind::Comment, 1), (TextKind::Comment, 2)],
            "a string, a line comment and a block comment"
        );
    }

    #[test]
    fn doc_comments_are_told_from_plain_ones() {
        let lexed = lex("//! inner\n/// outer\n//// plain\n/** block doc */\n/*! inner */\n/**/\n");
        let kinds: Vec<_> = lexed.texts.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                TextKind::Doc,
                TextKind::Doc,
                TextKind::Comment,
                TextKind::Doc,
                TextKind::Doc,
                TextKind::Comment
            ],
            "the doc forms and the plain ones"
        );
    }

    #[test]
    fn raw_byte_and_c_strings_are_literals_and_lifetimes_are_code() {
        let source = "r#\"a \" b\"# br\"x\" c\"y\" b'{' '}' 'a fn x<'a>() {} r#type bar\"z\"";
        let lexed = lex(source);
        let strings: Vec<_> = lexed.texts.iter().map(|t| t.body.as_str()).collect();
        assert_eq!(strings, ["r#\"a \" b\"#", "br\"x\"", "c\"y\"", "\"z\""]);
        assert_eq!(lexed.code.matches('{').count(), 1, "char literals blanked: {}", lexed.code);
        assert!(lexed.code.contains("<'a>"), "a lifetime stays code");
        assert!(lexed.code.contains("r#type"), "a raw identifier stays code");
    }

    #[test]
    fn nested_block_comments_and_escapes_end_where_rust_ends_them() {
        let lexed = lex("/* a /* b */ c */ x \"q\\\"r\" y");
        assert_eq!(lexed.texts.len(), 2, "one comment, one string");
        assert!(lexed.code.trim_start().starts_with('x'), "code after the comment");
        assert!(lexed.code.trim_end().ends_with('y'), "code after the string");
    }

    #[test]
    fn words_match_whole_tokens_only() {
        assert_eq!(find_word("unsafe { unsafe_code } xunsafe", "unsafe"), [0], "one keyword");
        assert_eq!(find_word("a::Command::new(x)", "Command::new"), [3], "a path");
    }
}

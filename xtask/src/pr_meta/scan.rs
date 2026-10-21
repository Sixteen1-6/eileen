//! A small Rust scanner that finds every `fn <name>` in a source file and the inline modules
//! around it, skipping comments, strings and character literals. It is enough to place a test by
//! its module path without compiling, which is all `pr-meta` needs (BUILD.md check 8).

/// A function definition and the inline modules (`mod name { ... }`) that enclose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FnSite {
    pub(crate) name: String,
    pub(crate) modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Ident(String),
    Open,
    Close,
    Other,
}

/// Every function definition in `source`, in order.
pub(crate) fn fn_sites(source: &str) -> Vec<FnSite> {
    let tokens = tokenize(source);
    let mut sites = Vec::new();
    let mut modules: Vec<(String, usize)> = Vec::new();
    let mut depth = 0_usize;
    let mut pending_module: Option<String> = None;
    let mut window = tokens.iter().peekable();
    while let Some(token) = window.next() {
        match token {
            Token::Ident(word) if word == "mod" || word == "fn" => {
                if let Some(Token::Ident(name)) = window.peek() {
                    if word == "fn" {
                        let enclosing = modules.iter().map(|(module, _)| module.clone()).collect();
                        sites.push(FnSite { name: name.clone(), modules: enclosing });
                    } else {
                        pending_module = Some(name.clone());
                    }
                }
            }
            Token::Open => {
                depth += 1;
                if let Some(name) = pending_module.take() {
                    modules.push((name, depth));
                }
            }
            Token::Close => {
                if modules.last().is_some_and(|(_, at)| *at == depth) {
                    modules.pop();
                }
                depth = depth.saturating_sub(1);
            }
            // The module's own name is an identifier; `mod name;` then ends on a token that is
            // not a brace, which makes it a file module that opens no scope.
            Token::Ident(_) => {}
            Token::Other => pending_module = None,
        }
    }
    sites
}

/// Splits source into identifiers, braces and everything else, dropping whitespace, comments,
/// string literals, character literals and lifetimes.
fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        at = match c {
            '/' if next == Some('/') => skip_line(&chars, at),
            '/' if next == Some('*') => skip_block_comment(&chars, at),
            '"' => skip_string(&chars, at + 1),
            '\'' => skip_quote(&chars, at),
            '{' => {
                tokens.push(Token::Open);
                at + 1
            }
            '}' => {
                tokens.push(Token::Close);
                at + 1
            }
            c if c.is_alphabetic() || c == '_' => read_word(&chars, at, &mut tokens),
            c if c.is_whitespace() => at + 1,
            _ => {
                tokens.push(Token::Other);
                at + 1
            }
        };
    }
    tokens
}

/// Reads an identifier or keyword, or a string literal with a prefix (`b"`, `r"`, `r#"`, `br"`,
/// `c"`, `cr"`), or a raw identifier (`r#name`, read as `name`).
fn read_word(chars: &[char], start: usize, tokens: &mut Vec<Token>) -> usize {
    let end =
        start + chars.iter().skip(start).take_while(|c| c.is_alphanumeric() || **c == '_').count();
    let word: String = chars.iter().skip(start).take(end - start).collect();
    let next = chars.get(end).copied();
    let prefix_of_string = matches!(word.as_str(), "b" | "c" | "r" | "br" | "cr");
    match next {
        Some('"') if prefix_of_string => {
            if word.contains('r') {
                skip_raw_string(chars, end, 0)
            } else {
                skip_string(chars, end + 1)
            }
        }
        Some('\'') if word == "b" => skip_quote(chars, end),
        Some('#') if word.ends_with('r') && prefix_of_string => {
            let hashes = chars.iter().skip(end).take_while(|c| **c == '#').count();
            match chars.get(end + hashes) {
                Some('"') => skip_raw_string(chars, end + hashes, hashes),
                Some(_) | None if word == "r" && hashes == 1 => read_word(chars, end + 1, tokens),
                Some(_) | None => {
                    tokens.push(Token::Ident(word));
                    end
                }
            }
        }
        Some(_) | None => {
            tokens.push(Token::Ident(word));
            end
        }
    }
}

fn skip_line(chars: &[char], start: usize) -> usize {
    start + chars.iter().skip(start).take_while(|c| **c != '\n').count()
}

/// Skips a block comment, which nests in Rust.
fn skip_block_comment(chars: &[char], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut at = start;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        if c == '/' && next == Some('*') {
            depth += 1;
            at += 2;
        } else if c == '*' && next == Some('/') {
            depth = depth.saturating_sub(1);
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    at
}

/// Skips the rest of a string literal whose opening quote is just before `start`.
fn skip_string(chars: &[char], start: usize) -> usize {
    let mut at = start;
    while let Some(&c) = chars.get(at) {
        match c {
            '\\' => at += 2,
            '"' => return at + 1,
            _ => at += 1,
        }
    }
    at
}

/// Skips a raw string whose opening quote is at `quote`, closed by a quote and `hashes` hashes.
fn skip_raw_string(chars: &[char], quote: usize, hashes: usize) -> usize {
    let mut at = quote + 1;
    while let Some(&c) = chars.get(at) {
        let closing = c == '"'
            && chars.iter().skip(at + 1).take(hashes).filter(|h| **h == '#').count() == hashes;
        if closing {
            return at + 1 + hashes;
        }
        at += 1;
    }
    at
}

/// Skips a character literal (`'a'`, `'\n'`, `'\u{1f}'`) or a lifetime or label (`'a`).
fn skip_quote(chars: &[char], quote: usize) -> usize {
    match (chars.get(quote + 1), chars.get(quote + 2)) {
        (Some('\\'), _) => {
            // The escaped character sits at quote + 2, so `'\''` closes at quote + 3.
            let rest = chars.iter().skip(quote + 3).take_while(|c| **c != '\'').count();
            quote + 3 + rest + 1
        }
        (Some(_), Some('\'')) => quote + 3,
        (Some(_) | None, _) => quote + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(name: &str, modules: &[&str]) -> FnSite {
        FnSite { name: name.to_owned(), modules: modules.iter().map(|m| (*m).to_owned()).collect() }
    }

    #[test]
    fn finds_functions_inside_nested_inline_modules() {
        let source = r##"
            //! Crate doc with fn not_code() inside a comment.
            mod files;
            pub fn top() {}
            #[cfg(test)]
            mod tests {
                use super::*;
                /* a block /* nested */ comment with fn hidden() */
                #[test]
                fn outer_test() { let s = "fn in_string() { mod fake {"; let c = '{'; }
                mod inner {
                    fn deep<'a>(x: &'a str) -> &'a str { x }
                }
                fn after_inner() {}
            }
            fn last() { let raw = r#"mod no { fn nope() }"#; let q = '\''; let n = '\n'; }
            fn after_quotes() {}
        "##;
        assert_eq!(
            fn_sites(source),
            vec![
                site("top", &[]),
                site("outer_test", &["tests"]),
                site("deep", &["tests", "inner"]),
                site("after_inner", &["tests"]),
                site("last", &[]),
                site("after_quotes", &[]),
            ],
            "comments, strings, chars and lifetimes are skipped; modules nest and close"
        );
    }

    #[test]
    fn file_modules_do_not_open_a_scope() {
        let source = "mod a;\nmod b;\nfn x() { if true { } }\nmod c { fn y() {} }\nfn z() {}\n";
        assert_eq!(
            fn_sites(source),
            vec![site("x", &[]), site("y", &["c"]), site("z", &[])],
            "`mod a;` declares a file module and leaves the scope alone"
        );
    }

    #[test]
    fn byte_strings_and_raw_identifiers_are_handled() {
        let source =
            "fn a() { let b = b\"{\"; let q = b'{'; }\nfn r#match() {}\nmod r#type { fn t() {} }\n";
        assert_eq!(
            fn_sites(source),
            vec![site("a", &[]), site("match", &[]), site("t", &["type"])],
            "byte literals hide braces and raw identifiers read as their name"
        );
    }
}

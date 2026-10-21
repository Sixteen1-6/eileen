//! A small Rust scanner that finds every `fn <name>` in a source file: the inline modules around
//! it, whether it is a test, and the lines it and its test module span. It skips comments,
//! strings and character literals. That is enough to place a test by its module path and to see
//! whether a diff touches it without compiling, which is all `pr-meta` needs (BUILD.md checks 8
//! and 19).

/// A run of source lines, 1-based, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lines {
    pub(crate) first: usize,
    pub(crate) last: usize,
}

impl Lines {
    /// Whether `line` is in the run.
    pub(crate) const fn contains(self, line: usize) -> bool {
        self.first <= line && line <= self.last
    }
}

/// A function definition, the inline modules (`mod name { ... }`) that enclose it, and where it
/// is in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FnSite {
    pub(crate) name: String,
    pub(crate) modules: Vec<String>,
    /// It carries an outer attribute whose path ends in `test`: `#[test]`, `#[tokio::test]`,
    /// or `#[test]` inside `proptest!`.
    pub(crate) is_test: bool,
    /// From its first outer attribute (or the `fn` keyword) to the end of its body.
    pub(crate) lines: Lines,
    /// The outermost enclosing test module: an inline module whose `#[cfg(...)]` condition
    /// names `test`, or one named `tests`.
    pub(crate) test_module: Option<Lines>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Ident(String),
    Punct(char),
    Open,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: Kind,
    line: usize,
}

/// An outer attribute, `#[...]`, waiting for the item it belongs to.
#[derive(Debug, Clone)]
struct Attribute {
    /// The last segment of its path: `test` for `#[tokio::test(...)]`.
    last: String,
    /// Whether the identifier `test` appears in its arguments, as in `#[cfg(test)]`.
    mentions_test: bool,
}

/// An item whose name has been read and whose body (or closing `;`) has not.
#[derive(Debug)]
enum Pending {
    /// A function, by its index in the sites, and the bracket nesting at its name, so a `;`
    /// inside its signature (`[u8; 4]`) does not end it.
    Fn { site: usize, nesting: usize },
    /// A module, which an opening brace makes inline and a `;` makes a file module.
    Mod { name: String, is_test: bool, first: usize },
}

/// An inline module whose closing brace has not been read.
#[derive(Debug)]
struct OpenModule {
    name: String,
    depth: usize,
    is_test: bool,
    /// Its index in the list of module line runs.
    index: usize,
}

/// Every function definition in `source`, in order.
pub(crate) fn fn_sites(source: &str) -> Vec<FnSite> {
    let tokens = tokenize(source);
    let mut scanner = Scanner { end_of_file: source.lines().count().max(1), ..Scanner::default() };
    let mut at = 0;
    while let Some(token) = tokens.get(at) {
        at += 1;
        match &token.kind {
            Kind::Punct('#') => {
                if let Some((attribute, next)) = read_attribute(&tokens, at) {
                    if let Some(attribute) = attribute {
                        scanner.attributes_from.get_or_insert(token.line);
                        scanner.attributes.push(attribute);
                    }
                    at = next;
                }
            }
            Kind::Ident(word) if word == "fn" || word == "mod" => {
                if let Some(Token { kind: Kind::Ident(name), .. }) = tokens.get(at) {
                    at += 1;
                    scanner.item(word == "fn", name, token.line);
                }
            }
            Kind::Open => scanner.open(),
            Kind::Close => scanner.close(token.line),
            Kind::Punct(';') => scanner.semicolon(token.line),
            Kind::Punct('(' | '[') => scanner.nesting += 1,
            Kind::Punct(')' | ']') => scanner.nesting = scanner.nesting.saturating_sub(1),
            // After `mod name` only a brace or a semicolon may follow.
            Kind::Ident(_) | Kind::Punct(_) => {
                if matches!(scanner.pending, Some(Pending::Mod { .. })) {
                    scanner.pending = None;
                }
            }
        }
    }
    scanner.finish()
}

/// What the scanner knows between tokens.
#[derive(Debug, Default)]
struct Scanner {
    /// The last line of the source, where an item that never closes ends.
    end_of_file: usize,
    sites: Vec<FnSite>,
    /// For each site, the index in `module_lines` of its outermost test module.
    site_test_module: Vec<Option<usize>>,
    /// The lines of every inline module, in the order they open.
    module_lines: Vec<Lines>,
    /// The inline modules open at this point.
    modules: Vec<OpenModule>,
    /// The function bodies open at this point: the site and the brace depth inside the body.
    bodies: Vec<(usize, usize)>,
    /// The outer attributes read since the last item, and the line of the first.
    attributes: Vec<Attribute>,
    attributes_from: Option<usize>,
    pending: Option<Pending>,
    /// Brace depth.
    depth: usize,
    /// Parenthesis and bracket depth.
    nesting: usize,
}

impl Scanner {
    /// `fn <name>` or `mod <name>` at `line`, which takes the attributes read before it.
    fn item(&mut self, is_fn: bool, name: &str, line: usize) {
        let first = self.attributes_from.take().unwrap_or(line);
        let attached = std::mem::take(&mut self.attributes);
        if is_fn {
            self.sites.push(FnSite {
                name: name.to_owned(),
                modules: self.modules.iter().map(|module| module.name.clone()).collect(),
                is_test: attached.iter().any(|attribute| attribute.last == "test"),
                lines: Lines { first, last: self.end_of_file },
                test_module: None,
            });
            let outermost_test = self.modules.iter().find(|module| module.is_test);
            self.site_test_module.push(outermost_test.map(|module| module.index));
            let site = self.sites.len() - 1;
            self.pending = Some(Pending::Fn { site, nesting: self.nesting });
        } else {
            let is_test = name == "tests"
                || attached
                    .iter()
                    .any(|attribute| attribute.last == "cfg" && attribute.mentions_test);
            self.pending = Some(Pending::Mod { name: name.to_owned(), is_test, first });
        }
    }

    /// An opening brace: the body of a pending function or module, or any other block.
    fn open(&mut self) {
        self.depth += 1;
        match self.pending.take() {
            Some(Pending::Fn { site, .. }) => self.bodies.push((site, self.depth)),
            Some(Pending::Mod { name, is_test, first }) => {
                self.module_lines.push(Lines { first, last: self.end_of_file });
                let index = self.module_lines.len() - 1;
                self.modules.push(OpenModule { name, depth: self.depth, is_test, index });
            }
            None => {}
        }
        self.forget_attributes();
    }

    /// A closing brace at `line`, which may end a function body or an inline module.
    fn close(&mut self, line: usize) {
        let depth = self.depth;
        if let Some((site, _)) = self.bodies.pop_if(|(_, at_depth)| *at_depth == depth)
            && let Some(site) = self.sites.get_mut(site)
        {
            site.lines.last = line;
        }
        if let Some(module) = self.modules.pop_if(|module| module.depth == depth)
            && let Some(lines) = self.module_lines.get_mut(module.index)
        {
            lines.last = line;
        }
        self.depth = depth.saturating_sub(1);
        self.pending = None;
        self.forget_attributes();
    }

    /// A `;` at `line`, which ends a function with no body or declares a file module, unless it
    /// sits inside brackets in a signature.
    fn semicolon(&mut self, line: usize) {
        match self.pending {
            Some(Pending::Fn { site, nesting }) if nesting == self.nesting => {
                if let Some(site) = self.sites.get_mut(site) {
                    site.lines.last = line;
                }
                self.pending = None;
            }
            Some(Pending::Mod { .. }) => self.pending = None,
            Some(Pending::Fn { .. }) | None => {}
        }
        self.forget_attributes();
    }

    fn forget_attributes(&mut self) {
        self.attributes.clear();
        self.attributes_from = None;
    }

    /// The sites, each with the lines of its outermost test module.
    fn finish(mut self) -> Vec<FnSite> {
        for (site, module) in self.sites.iter_mut().zip(self.site_test_module) {
            site.test_module = module.and_then(|index| self.module_lines.get(index).copied());
        }
        self.sites
    }
}

/// Reads the attribute after a `#` at `tokens[at - 1]`: `[...]` is an outer attribute and
/// `![...]` an inner one, which belongs to the enclosing item and so gives no attribute. Returns
/// the attribute and the index after its closing bracket, or `None` when the `#` starts none.
fn read_attribute(tokens: &[Token], at: usize) -> Option<(Option<Attribute>, usize)> {
    let inner = matches!(tokens.get(at).map(|token| &token.kind), Some(Kind::Punct('!')));
    let open = if inner { at + 1 } else { at };
    if !matches!(tokens.get(open).map(|token| &token.kind), Some(Kind::Punct('['))) {
        return None;
    }
    let mut brackets = 0_usize;
    let mut path: Vec<&str> = Vec::new();
    let mut in_path = true;
    let mut mentions_test = false;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match &token.kind {
            Kind::Punct('[') => brackets += 1,
            Kind::Punct(']') => {
                brackets = brackets.saturating_sub(1);
                if brackets == 0 {
                    let last = path.last().copied().unwrap_or_default().to_owned();
                    let attribute = (!inner).then_some(Attribute { last, mentions_test });
                    return Some((attribute, index + 1));
                }
            }
            Kind::Ident(word) if in_path => path.push(word),
            Kind::Punct(':') if in_path => {}
            Kind::Ident(word) => mentions_test |= word == "test",
            Kind::Punct(_) | Kind::Open | Kind::Close => in_path = false,
        }
    }
    Some((None, tokens.len()))
}

/// Splits source into identifiers, braces and single punctuation characters, each with its
/// line, dropping whitespace, comments, string literals, character literals and lifetimes.
fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let newlines: Vec<usize> =
        chars.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(at, _)| at).collect();
    let line_of = |at: usize| newlines.partition_point(|&newline| newline < at) + 1;
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        let line = line_of(at);
        at = match c {
            '/' if next == Some('/') => skip_line(&chars, at),
            '/' if next == Some('*') => skip_block_comment(&chars, at),
            '"' => skip_string(&chars, at + 1),
            '\'' => skip_quote(&chars, at),
            c if c.is_alphabetic() || c == '_' => read_word(&chars, at, line, &mut tokens),
            // Digits are dropped with whitespace: no rule reads a number.
            c if c.is_whitespace() || c.is_ascii_digit() => at + 1,
            '{' => {
                tokens.push(Token { kind: Kind::Open, line });
                at + 1
            }
            '}' => {
                tokens.push(Token { kind: Kind::Close, line });
                at + 1
            }
            c => {
                tokens.push(Token { kind: Kind::Punct(c), line });
                at + 1
            }
        };
    }
    tokens
}

/// Reads an identifier or keyword, or a string literal with a prefix (`b"`, `r"`, `r#"`, `br"`,
/// `c"`, `cr"`), or a raw identifier (`r#name`, read as `name`).
fn read_word(chars: &[char], start: usize, line: usize, tokens: &mut Vec<Token>) -> usize {
    let end =
        start + chars.iter().skip(start).take_while(|c| c.is_alphanumeric() || **c == '_').count();
    let word: String = chars.iter().skip(start).take(end - start).collect();
    let next = chars.get(end).copied();
    let prefix_of_string = matches!(word.as_str(), "b" | "c" | "r" | "br" | "cr");
    match next {
        Some('"') if prefix_of_string => {
            return if word.contains('r') {
                skip_raw_string(chars, end, 0)
            } else {
                skip_string(chars, end + 1)
            };
        }
        Some('\'') if word == "b" => return skip_quote(chars, end),
        Some('#') if word.ends_with('r') && prefix_of_string => {
            let hashes = chars.iter().skip(end).take_while(|c| **c == '#').count();
            match chars.get(end + hashes) {
                Some('"') => return skip_raw_string(chars, end + hashes, hashes),
                Some(_) | None if word == "r" && hashes == 1 => {
                    return read_word(chars, end + 1, line, tokens);
                }
                Some(_) | None => {}
            }
        }
        Some(_) | None => {}
    }
    tokens.push(Token { kind: Kind::Ident(word), line });
    end
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

    /// Each site as `name` with its modules, `a::b::name`.
    fn paths(source: &str) -> Vec<String> {
        fn_sites(source)
            .into_iter()
            .map(|site| {
                let mut path = site.modules;
                path.push(site.name);
                path.join("::")
            })
            .collect()
    }

    fn site<'a>(sites: &'a [FnSite], name: &str) -> &'a FnSite {
        sites.iter().find(|site| site.name == name).expect("the fixture defines the fn")
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
            paths(source),
            vec![
                "top",
                "tests::outer_test",
                "tests::inner::deep",
                "tests::after_inner",
                "last",
                "after_quotes",
            ],
            "comments, strings, chars and lifetimes are skipped; modules nest and close"
        );
    }

    #[test]
    fn file_modules_do_not_open_a_scope() {
        let source = "mod a;\nmod b;\nfn x() { if true { } }\nmod c { fn y() {} }\nfn z() {}\n";
        assert_eq!(
            paths(source),
            vec!["x", "c::y", "z"],
            "`mod a;` declares a file module and leaves the scope alone"
        );
    }

    #[test]
    fn byte_strings_and_raw_identifiers_are_handled() {
        let source =
            "fn a() { let b = b\"{\"; let q = b'{'; }\nfn r#match() {}\nmod r#type { fn t() {} }\n";
        assert_eq!(
            paths(source),
            vec!["a", "match", "type::t"],
            "byte literals hide braces and raw identifiers read as their name"
        );
    }

    #[test]
    fn tests_are_told_from_other_functions_by_their_attributes() {
        let source = "\
pub fn step() {}
#[cfg(test)]
fn test_only_helper() {}
#[cfg(test)]
mod tests {
    #[test]
    fn plain() {}
    #[tokio::test(flavor = \"current_thread\")]
    async fn on_tokio() {}
    #[expect(clippy::unwrap_used, reason = \"a test\")]
    #[test]
    pub(crate) fn with_two_attributes() {}
    proptest::proptest! {
        #[test]
        fn property(x in 0..10_u8) { let _ = x; }
    }
    #[cfg_attr(test, expect(dead_code))]
    fn helper() {}
}
";
        let sites = fn_sites(source);
        let tests: Vec<&str> =
            sites.iter().filter(|site| site.is_test).map(|site| site.name.as_str()).collect();
        assert_eq!(
            tests,
            vec!["plain", "on_tokio", "with_two_attributes", "property"],
            "an attribute whose path ends in `test` marks a test; `cfg(test)` alone does not"
        );
    }

    #[test]
    fn functions_and_test_modules_know_their_lines() {
        let source = "\
pub fn step(buffer: [u8; 4]) {
    work(buffer);
}

#[cfg(all(test, windows))]
mod win_tests {
    fn fixture() -> u8 { 1 }

    #[test]
    fn barge_in_pauses() {
        assert_eq!(fixture(), 1);
    }
}

mod checks {
    #[test]
    fn not_in_a_test_module() {}
}

trait Port {
    fn tick(&self);
}
";
        let sites = fn_sites(source);
        let step = site(&sites, "step");
        assert_eq!(step.lines, Lines { first: 1, last: 3 }, "a `;` in the signature is skipped");
        assert_eq!(step.test_module, None, "production code is in no test module");
        let test = site(&sites, "barge_in_pauses");
        assert_eq!(test.lines, Lines { first: 9, last: 12 }, "from its attribute to its brace");
        assert_eq!(
            test.test_module,
            Some(Lines { first: 5, last: 13 }),
            "a module whose cfg names `test` is a test module, from its attribute"
        );
        assert_eq!(site(&sites, "fixture").test_module, test.test_module, "helpers share it");
        let elsewhere = site(&sites, "not_in_a_test_module");
        assert!(elsewhere.is_test && elsewhere.test_module.is_none(), "`checks` is no test module");
        assert_eq!(site(&sites, "tick").lines, Lines { first: 21, last: 21 }, "no body");
    }
}

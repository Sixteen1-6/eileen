//! Check 6: the house style over Markdown, doc comments and string literals.
//!
//! The character rule: no em dash (U+2014). The names rules (the product only ever "Eileen",
//! no earlier working names, workers named only by route, no vendor product name inside an
//! Eileen name, no film or trademark names) need a list, and the list must never appear in the
//! repository, so they are the denylist's job: `xtask/tidy/denylist.toml` holds a salt and the
//! salted SHA-256 of each normalised token and two- and three-word phrase
//! (`entry_hash` in the tests), seeded by the owner in WP0 phase B through `cargo xtask tidy
//! denylist-add`. A failure quotes the matched text from the scanned file, never the list.
//! Files that quote third-party text verbatim are exempt by name in `xtask/tidy/exempt.toml`.

use std::collections::{BTreeSet, HashMap};

use super::lexer::{TextKind, lex};
use super::tidy::{Tidy, is_listed};
use super::{Check, Finding, Tree, TreeError, has_extension, line_of};
use crate::sha256;

/// The em dash.
const EM_DASH: char = '\u{2014}';
/// The longest phrase the denylist holds, in words.
const MAX_PHRASE: usize = 3;

/// The salted hash of a denylist entry: SHA-256 over the salt, a newline and the phrase's
/// words, lowercased and joined by single spaces, as lowercase hex. The tests seed fixture
/// entries with it, and WP0 phase B's `tidy denylist-add` writes the real ones with it.
#[cfg(test)]
pub(crate) fn entry_hash(salt: &str, phrase: &str) -> String {
    let words: Vec<String> = words(phrase).into_iter().map(|(_, word)| word).collect();
    salted(salt, &words.join(" "))
}

/// The salted hash of a phrase already normalised (lowercased words joined by single spaces).
fn salted(salt: &str, normalised: &str) -> String {
    sha256::hex(&sha256::digest(format!("{salt}\n{normalised}").as_bytes()))
}

/// The words of `text` with their byte ranges: runs of letters and digits, lowercased.
fn words(text: &str) -> Vec<((usize, usize), String)> {
    let mut found = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (start, c.is_alphanumeric()) {
            (None, true) => start = Some(at),
            (Some(from), false) => {
                let word = text.get(from..at).unwrap_or_default();
                found.push(((from, at), word.to_lowercase()));
                start = None;
            }
            _ => {}
        }
    }
    found
}

/// The denylist for one run, remembering each phrase it has hashed: phrases repeat across a
/// tree far more often than they differ.
struct Denylist<'a> {
    salt: &'a str,
    denied: BTreeSet<&'a str>,
    verdicts: HashMap<String, bool>,
}

impl Denylist<'_> {
    /// Whether the normalised `phrase` is on the list.
    fn denies(&mut self, phrase: &str) -> bool {
        if let Some(&verdict) = self.verdicts.get(phrase) {
            return verdict;
        }
        let verdict = self.denied.contains(salted(self.salt, phrase).as_str());
        self.verdicts.insert(phrase.to_owned(), verdict);
        verdict
    }
}

/// Runs check 6.
pub(crate) fn check(
    tree: &dyn Tree,
    tidy: &Tidy,
    findings: &mut Vec<Finding>,
) -> Result<(), TreeError> {
    let mut denylist = Denylist {
        salt: &tidy.salt,
        denied: tidy.denied.iter().map(String::as_str).collect(),
        verdicts: HashMap::new(),
    };
    for path in tree.files() {
        if is_listed(&tidy.style_exempt, path) {
            continue;
        }
        let markdown = has_extension(path, "md");
        if !markdown && !has_extension(path, "rs") {
            continue;
        }
        let Some(text) = tree.read(path)? else { continue };
        if markdown {
            scan(path, &text, 1, &mut denylist, findings);
        } else {
            for piece in lex(&text).texts.into_iter().filter(|t| t.kind != TextKind::Comment) {
                scan(path, &piece.body, piece.line, &mut denylist, findings);
            }
        }
    }
    Ok(())
}

/// Scans one piece of text that starts on `first_line` of `path`.
fn scan(
    path: &str,
    text: &str,
    first_line: usize,
    denylist: &mut Denylist<'_>,
    findings: &mut Vec<Finding>,
) {
    for (at, _) in text.match_indices(EM_DASH) {
        findings.push(Finding::new(
            Check::Style,
            path,
            Some(first_line + line_of(text, at) - 1),
            "an em dash (U+2014)",
            "rewrite the sentence with a comma, a colon, parentheses or two sentences",
        ));
    }
    if denylist.denied.is_empty() {
        return;
    }
    let words = words(text);
    for start in 0..words.len() {
        let mut phrase = String::new();
        for (end, ((_, to), word)) in words.iter().enumerate().skip(start).take(MAX_PHRASE) {
            if end > start {
                phrase.push(' ');
            }
            phrase.push_str(word);
            if denylist.denies(&phrase) {
                let from = words.get(start).map_or(0, |((from, _), _)| *from);
                let matched = text.get(from..*to).unwrap_or_default();
                findings.push(Finding::new(
                    Check::Style,
                    path,
                    Some(first_line + line_of(text, from) - 1),
                    format!("\"{matched}\" is on the denylist"),
                    "name the product only Eileen and workers only by route; use no earlier \
                     working name, film or trademark name",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mem::MemTree;
    use super::super::{Check, run};
    use super::entry_hash;

    const SALT: &str = "00112233445566778899aabbccddeeff";

    fn found(tree: &MemTree) -> Vec<(String, Option<usize>, String)> {
        run(tree)
            .unwrap()
            .into_iter()
            .filter(|f| f.check == Check::Style)
            .map(|f| (f.path, f.line, f.problem))
            .collect()
    }

    #[test]
    fn an_em_dash_in_markdown_a_doc_comment_or_a_string_fails() {
        let dash = '\u{2014}';
        let mut tree = MemTree::clean();
        tree.set("docs/a.md", &format!("line one\nline {dash} two\n"));
        tree.set(
            "crates/demo/src/extra.rs",
            &format!("/// Docs {dash} here.\nfn f() -> &'static str {{\n    \"a {dash} b\"\n}}\n// plain {dash} comment\n"),
        );
        let found = found(&tree);
        let places: Vec<_> = found.iter().map(|(p, l, _)| (p.as_str(), *l)).collect();
        assert_eq!(
            places,
            [
                ("crates/demo/src/extra.rs", Some(1)),
                ("crates/demo/src/extra.rs", Some(3)),
                ("docs/a.md", Some(2))
            ],
            "the doc comment, the string and the Markdown, not the plain comment: {found:?}"
        );
    }

    #[test]
    fn an_exempt_file_is_not_scanned() {
        let mut tree = MemTree::clean();
        tree.set("docs/research/report.md", "quoted \u{2014} text\n");
        assert!(found(&tree).is_empty(), "docs/research is exempt");
    }

    #[test]
    fn a_denylisted_synthetic_name_fails_and_shows_the_files_text() {
        let mut tree = MemTree::clean();
        let entries = [entry_hash(SALT, "Zorblax"), entry_hash(SALT, "quintet  of   GLOOM")];
        tree.set(
            "xtask/tidy/denylist.toml",
            &format!("salt = \"{SALT}\"\nentries = [\"{}\", \"{}\"]\n", entries[0], entries[1]),
        );
        tree.set("docs/a.md", "Meet ZORBLAX, the\nQuintet of Gloom's leader.\n");
        tree.set("crates/demo/src/extra.rs", "const NAME: &str = \"zorblax\";\n");
        let found = found(&tree);
        let problems: Vec<_> = found.iter().map(|(p, l, m)| format!("{p}:{l:?} {m}")).collect();
        assert_eq!(
            problems,
            [
                "crates/demo/src/extra.rs:Some(1) \"zorblax\" is on the denylist",
                "docs/a.md:Some(1) \"ZORBLAX\" is on the denylist",
                "docs/a.md:Some(2) \"Quintet of Gloom\" is on the denylist",
            ],
            "a token and a phrase across words, quoted as written"
        );
    }

    #[test]
    fn entries_hash_their_normalised_words() {
        assert_eq!(
            entry_hash(SALT, "Quintet of Gloom"),
            entry_hash(SALT, " quintet-OF gloom "),
            "normalised"
        );
        assert_ne!(entry_hash(SALT, "quintet"), entry_hash("other salt", "quintet"), "salted");
        assert_eq!(entry_hash(SALT, "x").len(), 64, "hex SHA-256");
    }
}

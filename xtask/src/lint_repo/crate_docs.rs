//! Check 1: every workspace crate keeps its guidance in its crate doc, the `//!` comment at the
//! top of `src/lib.rs`, with the eight headings of BUILD.md section 4b.4 in order, at most three
//! bullets under `# Owns`, within 80 lines; a binary's `main.rs` (and each `src/bin/*.rs`) holds
//! one line pointing to it. A crate with no library keeps the crate doc in `main.rs`.

use super::{Check, Finding, Tree, TreeError, has_extension};

/// The required headings, in order.
pub(crate) const HEADINGS: [&str; 8] = [
    "Owns",
    "Does not own",
    "Invariants",
    "Boundaries",
    "Testing",
    "Traps",
    "Logs and tracing",
    "Do not change without a decision record",
];

/// The longest crate doc section 4b.4 allows.
const MAX_LINES: usize = 80;
/// The most bullets `# Owns` may have.
const MAX_OWNS: usize = 3;

/// Runs check 1 over every workspace member.
pub(crate) fn check(tree: &dyn Tree, findings: &mut Vec<Finding>) -> Result<(), TreeError> {
    for dir in members(tree)? {
        let lib = format!("{dir}/src/lib.rs");
        let main = format!("{dir}/src/main.rs");
        let bin_prefix = format!("{dir}/src/bin/");
        let mut binaries: Vec<String> = tree
            .files()
            .iter()
            .filter(|path| {
                path.strip_prefix(&bin_prefix)
                    .is_some_and(|name| has_extension(name, "rs") && !name.contains('/'))
            })
            .cloned()
            .collect();
        let doc_file = if tree.read(&lib)?.is_some() {
            if tree.read(&main)?.is_some() {
                binaries.insert(0, main);
            }
            lib
        } else if tree.read(&main)?.is_some() {
            main
        } else {
            continue;
        };
        if let Some(text) = tree.read(&doc_file)? {
            check_crate_doc(&doc_file, &text, findings);
        }
        for binary in binaries {
            if let Some(text) = tree.read(&binary)? {
                check_binary_doc(&binary, &text, findings);
            }
        }
    }
    Ok(())
}

/// The workspace members' folders, from the root manifest's `members` (a trailing `/*`
/// stands for every folder below it that holds a `Cargo.toml`).
pub(crate) fn members(tree: &dyn Tree) -> Result<Vec<String>, TreeError> {
    let Some(text) = tree.read("Cargo.toml")? else {
        return Ok(Vec::new());
    };
    let manifest =
        text.parse::<toml::Table>().map_err(|error| TreeError(format!("Cargo.toml: {error}")))?;
    let patterns: Vec<String> = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let mut dirs = Vec::new();
    for pattern in patterns {
        if let Some(parent) = pattern.strip_suffix("/*") {
            let prefix = format!("{parent}/");
            for path in tree.files() {
                if let Some(name) = path
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.strip_suffix("/Cargo.toml"))
                    .filter(|name| !name.contains('/'))
                {
                    dirs.push(format!("{parent}/{name}"));
                }
            }
        } else {
            dirs.push(pattern);
        }
    }
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

/// The leading `//!` lines of a file, with the marker and one following space removed.
fn crate_doc(text: &str) -> Vec<&str> {
    text.lines()
        .map_while(|line| line.strip_prefix("//!"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect()
}

fn check_crate_doc(path: &str, text: &str, findings: &mut Vec<Finding>) {
    let doc = crate_doc(text);
    let fail = |findings: &mut Vec<Finding>, line: Option<usize>, problem: String, fix: &str| {
        findings.push(Finding::new(Check::CrateDocs, path, line, problem, fix));
    };
    if doc.is_empty() {
        fail(
            findings,
            Some(1),
            "the crate has no crate doc at the top of the file".to_owned(),
            "start the file with the `//!` crate doc of BUILD.md section 4b.4",
        );
        return;
    }
    if doc.len() > MAX_LINES {
        fail(
            findings,
            Some(MAX_LINES + 1),
            format!("the crate doc has {} lines; section 4b.4 allows {MAX_LINES}", doc.len()),
            "shorten the crate doc; move detail into module docs",
        );
    }
    let found: Vec<(usize, &str)> = doc
        .iter()
        .enumerate()
        .filter_map(|(at, line)| line.strip_prefix("# ").map(|heading| (at + 1, heading.trim())))
        .filter(|(_, heading)| HEADINGS.contains(heading))
        .collect();
    let missing: Vec<&str> =
        HEADINGS.iter().copied().filter(|h| found.iter().all(|(_, f)| f != h)).collect();
    if !missing.is_empty() {
        fail(
            findings,
            None,
            format!("the crate doc lacks the headings {}", quoted(&missing)),
            "add each heading of BUILD.md section 4b.4 with its content",
        );
    }
    let order: Vec<&str> = found.iter().map(|(_, heading)| *heading).collect();
    let expected: Vec<&str> = HEADINGS.iter().copied().filter(|h| order.contains(h)).collect();
    if order != expected {
        fail(
            findings,
            found.first().map(|(line, _)| *line),
            format!("the crate doc's headings are out of order: {}", quoted(&order)),
            "put the headings in section 4b.4's order",
        );
    }
    if let Some(&(owns_at, _)) = found.iter().find(|(_, heading)| *heading == "Owns") {
        let bullets = doc
            .iter()
            .skip(owns_at)
            .take_while(|line| !line.starts_with("# "))
            .filter(|line| line.starts_with("- "))
            .count();
        if bullets > MAX_OWNS {
            fail(
                findings,
                Some(owns_at),
                format!("`# Owns` has {bullets} bullets; section 4b.4 allows {MAX_OWNS}"),
                "merge the bullets under `# Owns` into at most three",
            );
        }
    }
}

fn check_binary_doc(path: &str, text: &str, findings: &mut Vec<Finding>) {
    let lines = text.lines().filter(|line| line.trim_start().starts_with("//!")).count();
    if lines != 1 {
        findings.push(Finding::new(
            Check::CrateDocs,
            path,
            Some(1),
            format!(
                "a binary's doc is one line pointing to its library's crate doc; this has {lines}"
            ),
            "keep one `//!` line naming the library whose crate doc holds the guidance",
        ));
    }
}

fn quoted(items: &[&str]) -> String {
    items.iter().map(|item| format!("`# {item}`")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::super::mem::{GOOD_CRATE_DOC, MemTree};
    use super::super::{Check, run};

    fn check1(tree: &MemTree) -> Vec<String> {
        run(tree)
            .unwrap()
            .into_iter()
            .filter(|f| f.check == Check::CrateDocs)
            .map(|f| format!("{}: {}", f.path, f.problem))
            .collect()
    }

    #[test]
    fn a_crate_doc_missing_invariants_fails() {
        let mut tree = MemTree::clean();
        tree.set("crates/demo/src/lib.rs", &GOOD_CRATE_DOC.replace("//! # Invariants\n", ""));
        let found = check1(&tree);
        assert_eq!(found.len(), 1, "one finding: {found:?}");
        assert!(found[0].contains("`# Invariants`"), "it names the heading: {found:?}");
    }

    #[test]
    fn a_crate_doc_over_eighty_lines_fails() {
        let mut tree = MemTree::clean();
        let padding = "//! More.\n".repeat(60);
        let long = GOOD_CRATE_DOC.replacen("//!\n", &format!("//!\n{padding}"), 1);
        tree.set("crates/demo/src/lib.rs", &long);
        let found = check1(&tree);
        assert!(found.iter().any(|f| f.contains("allows 80")), "too long: {found:?}");
    }

    #[test]
    fn headings_out_of_order_and_four_owns_bullets_fail() {
        let mut tree = MemTree::clean();
        let swapped = GOOD_CRATE_DOC
            .replace("//! # Traps", "//! # TEMP")
            .replace("//! # Testing", "//! # Traps")
            .replace("//! # TEMP", "//! # Testing")
            .replace("//! - The demo.\n", "//! - One.\n//! - Two.\n//! - Three.\n//! - Four.\n");
        tree.set("crates/demo/src/lib.rs", &swapped);
        let found = check1(&tree);
        assert!(found.iter().any(|f| f.contains("out of order")), "order: {found:?}");
        assert!(found.iter().any(|f| f.contains("4 bullets")), "bullets: {found:?}");
    }

    #[test]
    fn a_binary_doc_of_more_than_one_line_fails() {
        let mut tree = MemTree::clean();
        tree.set("crates/demo/src/main.rs", "//! The binary.\n//! It does a lot.\nfn main() {}\n");
        tree.set("crates/demo/src/bin/tool.rs", "//! The tool; see the crate doc.\nfn main() {}\n");
        let found = check1(&tree);
        assert_eq!(found.len(), 1, "only main.rs: {found:?}");
        assert!(found[0].starts_with("crates/demo/src/main.rs"), "main.rs: {found:?}");
    }

    #[test]
    fn a_crate_without_a_crate_doc_fails_and_one_with_only_a_binary_keeps_it_there() {
        let mut tree = MemTree::clean();
        tree.set("crates/bare/Cargo.toml", "[package]\nname = \"bare\"\n");
        tree.set("crates/bare/src/lib.rs", "pub fn f() {}\n");
        tree.set("crates/tool/Cargo.toml", "[package]\nname = \"tool\"\n");
        tree.set("crates/tool/src/main.rs", GOOD_CRATE_DOC);
        let found = check1(&tree);
        assert_eq!(found.len(), 1, "only the bare crate: {found:?}");
        assert!(found[0].contains("no crate doc"), "{found:?}");
    }
}

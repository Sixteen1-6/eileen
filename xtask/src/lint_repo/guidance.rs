//! Checks 2 and 4: the guidance files of BUILD.md section 4b.4.
//!
//! Check 2 wants the root `AGENTS.md` (at most 150 lines) and a root `CLAUDE.md` that starts
//! with `@AGENTS.md`, and a nested `AGENTS.md` only in a folder `xtask/tidy/guidance.toml`
//! lists (section 4b.4's list), each with a "Why this folder has its own rules" heading and a
//! sibling `CLAUDE.md` whose first line is `@AGENTS.md`. Check 4 wants every
//! `cargo xtask <command>` a guidance file names to be one of xtask's commands.

use super::tidy::{Tidy, is_listed};
use super::{Check, Finding, Tree, TreeError, crate_docs, has_extension};
use crate::cli::COMMANDS;

/// The longest root `AGENTS.md` section 4b.4 allows.
const MAX_ROOT_LINES: usize = 150;
/// The heading every nested `AGENTS.md` carries.
const WHY_HEADING: &str = "why this folder has its own rules";
/// The first line of every `CLAUDE.md`.
const IMPORT: &str = "@AGENTS.md";

/// Runs check 2.
pub(crate) fn check_nested(
    tree: &dyn Tree,
    tidy: &Tidy,
    findings: &mut Vec<Finding>,
) -> Result<(), TreeError> {
    let fail = |findings: &mut Vec<Finding>, path: &str, line, problem: &str, fix: &str| {
        findings.push(Finding::new(Check::Guidance, path, line, problem, fix));
    };
    match tree.read("AGENTS.md")? {
        None => fail(
            findings,
            "AGENTS.md",
            None,
            "the root AGENTS.md is missing",
            "add the root AGENTS.md of BUILD.md section 4b.4",
        ),
        Some(text) if text.lines().count() > MAX_ROOT_LINES => fail(
            findings,
            "AGENTS.md",
            Some(MAX_ROOT_LINES + 1),
            &format!(
                "the root AGENTS.md has {} lines; at most {MAX_ROOT_LINES}",
                text.lines().count()
            ),
            "shorten it; detail belongs in crate docs, docs/ or the linked references",
        ),
        Some(_) => {}
    }
    check_import(tree, "CLAUDE.md", findings)?;
    for path in tree.files() {
        let Some((dir, name)) = path.rsplit_once('/') else { continue };
        if is_listed(&tidy.guidance_ignored, dir) {
            continue;
        }
        if name == "AGENTS.md" {
            if !tidy.nested.iter().any(|entry| entry.path.trim_end_matches('/') == dir) {
                fail(
                    findings,
                    path,
                    None,
                    "a nested AGENTS.md in a folder section 4b.4 does not list",
                    "move its rules into the crate doc or the root AGENTS.md, or add the folder to \
                     xtask/tidy/guidance.toml with the reason its rules differ (and say why in \
                     the pull request)",
                );
            }
            let has_why = tree.read(path)?.is_some_and(|text| {
                text.lines().any(|line| {
                    let heading = line.trim_start_matches('#');
                    heading.len() < line.len() && heading.trim().to_lowercase() == WHY_HEADING
                })
            });
            if !has_why {
                fail(
                    findings,
                    path,
                    None,
                    "a nested AGENTS.md without a \"Why this folder has its own rules\" heading",
                    "add the heading and say why this folder's rules differ",
                );
            }
            check_import(tree, &format!("{dir}/CLAUDE.md"), findings)?;
        } else if name == "CLAUDE.md" && tree.read(&format!("{dir}/AGENTS.md"))?.is_none() {
            fail(
                findings,
                path,
                None,
                "a nested CLAUDE.md with no sibling AGENTS.md",
                "put the rules in an AGENTS.md beside it and make this file `@AGENTS.md`",
            );
        }
    }
    Ok(())
}

/// A `CLAUDE.md` must exist and start with `@AGENTS.md`.
fn check_import(tree: &dyn Tree, path: &str, findings: &mut Vec<Finding>) -> Result<(), TreeError> {
    let problem = match tree.read(path)? {
        None => format!("{path} is missing"),
        Some(text) if text.lines().next().map(str::trim_end) == Some(IMPORT) => return Ok(()),
        Some(_) => format!("{path}'s first line is not `{IMPORT}`"),
    };
    findings.push(Finding::new(
        Check::Guidance,
        path,
        Some(1),
        problem,
        "make the first line `@AGENTS.md`, so Claude reads the shared guide (imports, not symlinks)",
    ));
    Ok(())
}

/// Whether a Markdown file is a guidance file check 4 reads.
fn is_guidance_markdown(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(name, "AGENTS.md" | "CLAUDE.md")
        || matches!(
            path,
            "README.md"
                | "CONTRIBUTING.md"
                | "ARCHITECTURE.md"
                | "SECURITY.md"
                | "docs/ARCHITECTURE.md"
                | "docs/DEV-SETUP.md"
        )
        || (has_extension(path, "md")
            && ["docs/wp/", ".claude/", ".github/"].iter().any(|dir| path.starts_with(dir)))
}

/// Runs check 4 over the guidance Markdown files and every crate doc.
pub(crate) fn check_commands(
    tree: &dyn Tree,
    tidy: &Tidy,
    findings: &mut Vec<Finding>,
) -> Result<(), TreeError> {
    for path in tree.files() {
        if is_guidance_markdown(path)
            && !is_listed(&tidy.guidance_ignored, path)
            && let Some(text) = tree.read(path)?
        {
            scan_commands(path, text.lines().enumerate(), findings);
        }
    }
    for dir in crate_docs::members(tree)? {
        for file in ["src/lib.rs", "src/main.rs"] {
            let path = format!("{dir}/{file}");
            if let Some(text) = tree.read(&path)? {
                let doc = text.lines().enumerate().take_while(|(_, l)| l.starts_with("//!"));
                scan_commands(&path, doc, findings);
            }
        }
    }
    Ok(())
}

fn scan_commands<'a>(
    path: &str,
    lines: impl Iterator<Item = (usize, &'a str)>,
    findings: &mut Vec<Finding>,
) {
    const NEEDLE: &str = "cargo xtask ";
    for (index, line) in lines {
        let mut rest = line;
        while let Some(at) = rest.find(NEEDLE) {
            let after = rest.get(at + NEEDLE.len()..).unwrap_or_default();
            let command: String =
                after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            if command.starts_with(|c: char| c.is_ascii_lowercase())
                && !COMMANDS.contains(&command.as_str())
            {
                findings.push(Finding::new(
                    Check::Commands,
                    path,
                    Some(index + 1),
                    format!("names `cargo xtask {command}`, which xtask does not have"),
                    "name an existing command (`cargo xtask help` lists them), or add the command",
                ));
            }
            rest = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mem::MemTree;
    use super::super::{Check, run};

    fn found(tree: &MemTree, check: Check) -> Vec<String> {
        run(tree)
            .unwrap()
            .into_iter()
            .filter(|f| f.check == check)
            .map(|f| format!("{}: {}", f.path, f.problem))
            .collect()
    }

    fn nested_ok(tree: &mut MemTree) {
        tree.set(
            "fuzz/AGENTS.md",
            "# Fuzzing\n\n## Why this folder has its own rules\n\nNightly.\n",
        );
        tree.set("fuzz/CLAUDE.md", "@AGENTS.md\n");
    }

    #[test]
    fn a_listed_nested_pair_passes() {
        let mut tree = MemTree::clean();
        nested_ok(&mut tree);
        assert_eq!(found(&tree, Check::Guidance), Vec::<String>::new(), "a listed folder");
    }

    #[test]
    fn a_nested_agents_md_in_an_unlisted_folder_fails() {
        let mut tree = MemTree::clean();
        tree.set("crates/demo/AGENTS.md", "# Demo\n\n## Why this folder has its own rules\n");
        tree.set("crates/demo/CLAUDE.md", "@AGENTS.md\n");
        let found = found(&tree, Check::Guidance);
        assert_eq!(found.len(), 1, "one finding: {found:?}");
        assert!(found[0].contains("does not list"), "{found:?}");
    }

    #[test]
    fn a_nested_agents_md_needs_its_claude_md_and_its_why_heading() {
        let mut tree = MemTree::clean();
        tree.set("fuzz/AGENTS.md", "# Fuzzing\n\nNightly.\n");
        let missing = found(&tree, Check::Guidance);
        assert!(missing.iter().any(|f| f.contains("fuzz/CLAUDE.md is missing")), "{missing:?}");
        assert!(missing.iter().any(|f| f.contains("Why this folder")), "{missing:?}");
        tree.set("fuzz/CLAUDE.md", "# Fuzzing\n@AGENTS.md\n");
        let wrong = found(&tree, Check::Guidance);
        assert!(wrong.iter().any(|f| f.contains("first line is not")), "{wrong:?}");
    }

    #[test]
    fn the_root_pair_is_checked() {
        let mut tree = MemTree::clean();
        tree.set("AGENTS.md", &"A line.\n".repeat(151));
        tree.set("CLAUDE.md", "# Claude\n");
        let found = found(&tree, Check::Guidance);
        assert!(found.iter().any(|f| f.contains("151 lines")), "too long: {found:?}");
        assert!(found.iter().any(|f| f.starts_with("CLAUDE.md")), "no import: {found:?}");
        let mut bare = MemTree::clean();
        bare.remove("AGENTS.md");
        bare.remove("CLAUDE.md");
        assert_eq!(run(&bare).unwrap().len(), 2, "both missing");
    }

    #[test]
    fn a_stray_claude_md_and_copies_for_other_projects() {
        let mut tree = MemTree::clean();
        tree.set("docs/CLAUDE.md", "@AGENTS.md\n");
        tree.set("templates/AGENTS.md", "# {{project}}\n");
        let found = found(&tree, Check::Guidance);
        assert_eq!(found.len(), 1, "templates/ is skipped: {found:?}");
        assert!(found[0].contains("no sibling AGENTS.md"), "{found:?}");
    }

    #[test]
    fn a_guidance_file_naming_a_missing_command_fails() {
        let mut tree = MemTree::clean();
        tree.set(
            "AGENTS.md",
            "# Guide\n\nRun `cargo xtask test -p x`, then\n`cargo xtask no-such-command`.\n",
        );
        tree.set(
            "crates/demo/src/lib.rs",
            &super::super::mem::GOOD_CRATE_DOC
                .replace("`cargo xtask test -p demo`", "`cargo xtask tset -p demo`"),
        );
        tree.set("docs/a.md", "Not guidance: `cargo xtask whatever`.\n");
        let found = found(&tree, Check::Commands);
        assert_eq!(found.len(), 2, "the root guide and the crate doc: {found:?}");
        assert!(found[0].contains("no-such-command"), "{found:?}");
        assert!(found[1].contains("`cargo xtask tset`"), "{found:?}");
    }
}

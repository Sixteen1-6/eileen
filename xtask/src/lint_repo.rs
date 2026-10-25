//! `cargo xtask lint-repo`: the fast repository checks of BUILD.md section 4b.3 that phase A
//! builds, each over the working tree in seconds and with no compilation:
//!
//! - check 1, every crate's guidance in its crate doc ([`crate_docs`]);
//! - check 2, nested `AGENTS.md` only where section 4b.4 lists one, and the root pair
//!   ([`guidance`]);
//! - check 4, every `cargo xtask <command>` a guidance file names exists ([`guidance`]);
//! - check 6, the house style's character rules and the hashed denylist ([`style`]);
//! - check 10, unsafe code only in `ffi` modules, and the workspace lints that hold the rest
//!   ([`code`]);
//! - check 14, the credential boundary, process spawns and the sanctioned
//!   `#[expect(clippy::disallowed_methods)]` sites ([`code`]);
//! - check 16, no ignored, panicking or sleeping tests, and modules over 800 lines only with a
//!   reason ([`code`]).
//!
//! Their data lives in `xtask/tidy/`, read through the same [`Tree`] as the code, so every
//! check runs over an in-memory tree in the tests.

mod code;
mod crate_docs;
mod guidance;
mod lexer;
#[cfg(test)]
mod mem;
mod style;
mod tidy;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Directories never walked: build output, git's own folder and installed packages.
const SKIPPED_DIRS: [&str; 3] = ["target", ".git", "node_modules"];

/// The checks `lint-repo` runs, named by their number in BUILD.md section 4b.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Check {
    /// Check 1: every crate's guidance is in its crate doc.
    CrateDocs,
    /// Check 2: nested guidance files only where section 4b.4 lists them.
    Guidance,
    /// Check 4: every `cargo xtask` command a guidance file names exists.
    Commands,
    /// Check 6: the house style.
    Style,
    /// Check 10: unsafe code only in `ffi` modules.
    Unsafe,
    /// Check 14: the credential boundary and process spawns.
    CredentialBoundary,
    /// Check 16: tests and module sizes.
    Tests,
}

impl Check {
    /// The check's number in section 4b.3.
    pub(crate) const fn number(self) -> u8 {
        match self {
            Self::CrateDocs => 1,
            Self::Guidance => 2,
            Self::Commands => 4,
            Self::Style => 6,
            Self::Unsafe => 10,
            Self::CredentialBoundary => 14,
            Self::Tests => 16,
        }
    }
}

/// One problem: the check, the file and line, what is wrong and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    /// The check that found it.
    pub(crate) check: Check,
    /// The file, from the repository root with `/` separators.
    pub(crate) path: String,
    /// The line (1-based), when the problem sits on one.
    pub(crate) line: Option<usize>,
    /// What is wrong.
    pub(crate) problem: String,
    /// How to fix it.
    pub(crate) fix: String,
}

impl Finding {
    /// A finding at `path`, on `line` when one is known.
    pub(crate) fn new(
        check: Check,
        path: &str,
        line: Option<usize>,
        problem: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self { check, path: path.to_owned(), line, problem: problem.into(), fix: fix.into() }
    }
}

/// The tree could not be read: the checks could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeError(pub(crate) String);

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the checks may know about the repository: its files and their text.
pub(crate) trait Tree {
    /// Every file, from the repository root with `/` separators, sorted.
    fn files(&self) -> &[String];
    /// A file's text, or `None` when it does not exist or is not UTF-8 (a binary file).
    fn read(&self, path: &str) -> Result<Option<String>, TreeError>;
}

/// The working tree on disk, walked once when opened.
#[derive(Debug)]
pub(crate) struct FsTree {
    root: PathBuf,
    files: Vec<String>,
}

impl FsTree {
    /// Walks the tree at `root`, skipping build output and `.git`. Symbolic links are not
    /// followed.
    pub(crate) fn open(root: &Path) -> Result<Self, TreeError> {
        let mut files = Vec::new();
        walk(root, "", &mut files)?;
        files.sort();
        Ok(Self { root: root.to_path_buf(), files })
    }
}

fn walk(root: &Path, prefix: &str, files: &mut Vec<String>) -> Result<(), TreeError> {
    let dir = root.join(prefix);
    let entries = fs::read_dir(&dir)
        .map_err(|error| TreeError(format!("cannot list {}: {error}", dir.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| TreeError(format!("cannot list {}: {error}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry
            .file_type()
            .map_err(|error| TreeError(format!("cannot stat {prefix}{name}: {error}")))?;
        let path = format!("{prefix}{name}");
        if kind.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(root, &format!("{path}/"), files)?;
            }
        } else if !kind.is_symlink() {
            files.push(path);
        }
    }
    Ok(())
}

impl Tree for FsTree {
    fn files(&self) -> &[String] {
        &self.files
    }

    fn read(&self, path: &str) -> Result<Option<String>, TreeError> {
        match fs::read(self.root.join(path)) {
            Ok(bytes) => Ok(String::from_utf8(bytes).ok()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(TreeError(format!("cannot read {path}: {error}"))),
        }
    }
}

/// Runs every check over `tree` and returns the findings, sorted by check, file and line.
pub(crate) fn run(tree: &dyn Tree) -> Result<Vec<Finding>, TreeError> {
    let tidy = tidy::Tidy::load(tree)?;
    let mut findings = Vec::new();
    crate_docs::check(tree, &mut findings)?;
    guidance::check_nested(tree, &tidy, &mut findings)?;
    guidance::check_commands(tree, &tidy, &mut findings)?;
    style::check(tree, &tidy, &mut findings)?;
    code::check(tree, &tidy, &mut findings)?;
    findings.sort_by(|a, b| (a.check, &a.path, a.line).cmp(&(b.check, &b.path, b.line)));
    Ok(findings)
}

/// Runs `cargo xtask lint-repo` over the tree at `root` and writes its report.
pub(crate) fn main(root: &Path, out: &mut dyn Write) -> io::Result<ExitCode> {
    let outcome = FsTree::open(root).and_then(|tree| Ok((tree.files.len(), run(&tree)?)));
    match outcome {
        Ok((files, findings)) => {
            let annotate = std::env::var_os("GITHUB_ACTIONS").is_some_and(|value| value == "true");
            render(files, &findings, annotate, out)?;
            Ok(if findings.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Err(error) => {
            writeln!(out, "lint-repo: could not run: {error}")?;
            Ok(ExitCode::from(2))
        }
    }
}

/// Writes the report: each finding with its check, place and fix, and the command that
/// reproduces it. On GitHub Actions each finding is also an error annotation on its line.
fn render(
    files: usize,
    findings: &[Finding],
    annotate: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    writeln!(out, "lint-repo: checks 1, 2, 4, 6, 10, 14 and 16 over {files} files")?;
    for finding in findings {
        let number = finding.check.number();
        let place = match finding.line {
            Some(line) => format!("{}:{line}", finding.path),
            None => finding.path.clone(),
        };
        writeln!(out, "check {number} failed at {place}: {}", finding.problem)?;
        writeln!(out, "  fix: {}", finding.fix)?;
        if annotate {
            let message = format!("{} Fix: {}", finding.problem, finding.fix);
            let line = finding.line.map(|line| format!(",line={line}")).unwrap_or_default();
            writeln!(
                out,
                "::error file={}{line},title=lint-repo check {number}::{}",
                finding.path,
                escape_annotation(&message)
            )?;
        }
    }
    if findings.is_empty() {
        writeln!(out, "lint-repo: passed")
    } else {
        writeln!(out, "reproduce: cargo xtask lint-repo")?;
        writeln!(out, "lint-repo: {} problems", findings.len())
    }
}

fn escape_annotation(text: &str) -> String {
    text.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

/// The 1-based line of byte `offset` in `text`.
pub(crate) fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset).map_or(1, |before| before.matches('\n').count() + 1)
}

/// Whether `path` has the extension `extension` (without its dot), in any case.
pub(crate) fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path).extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// Whether `path` is the file or directory `dir` (written without a trailing `/`) or lies
/// under it.
pub(crate) fn under(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    dir.is_empty()
        || path == dir
        || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::mem::MemTree;
    use super::{Check, FsTree, Tree, render, run, under};

    #[test]
    fn the_clean_fixture_passes_every_check() {
        let tree = MemTree::clean();
        assert_eq!(run(&tree), Ok(Vec::new()), "the clean fixture has no findings");
    }

    #[test]
    fn a_report_names_the_check_the_place_the_fix_and_the_command() {
        let mut tree = MemTree::clean();
        tree.set("docs/a.md", "one\ntwo \u{2014} three\n");
        let findings = run(&tree).unwrap();
        assert_eq!(findings.first().map(|f| f.check), Some(Check::Style), "check 6");
        let mut out = Vec::new();
        render(3, &findings, true, &mut out).unwrap();
        let report = String::from_utf8(out).unwrap();
        assert!(report.contains("check 6 failed at docs/a.md:2"), "check and place: {report}");
        assert!(report.contains("  fix: "), "a fix: {report}");
        assert!(
            report.contains("::error file=docs/a.md,line=2,title=lint-repo check 6::"),
            "an annotation: {report}"
        );
        assert!(report.contains("reproduce: cargo xtask lint-repo"), "the command: {report}");
    }

    #[test]
    fn the_working_tree_is_walked_without_build_output() {
        let root = std::env::temp_dir().join(format!("xtask-lint-walk-{}", std::process::id()));
        let _stale: std::io::Result<()> = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("target/debug/x.md"), "built").unwrap();
        std::fs::write(root.join("docs/a.md"), "text").unwrap();
        std::fs::write(root.join("blob.bin"), [0xff_u8, 0xfe, 0x00]).unwrap();
        let tree = FsTree::open(&root).unwrap();
        assert_eq!(tree.files(), ["blob.bin".to_owned(), "docs/a.md".to_owned()], "no target/");
        assert_eq!(tree.read("docs/a.md").unwrap().as_deref(), Some("text"), "text is read");
        assert_eq!(tree.read("blob.bin").unwrap(), None, "a binary file reads as none");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn under_matches_whole_path_components() {
        assert!(under("docs/research/a.md", "docs/research"), "a file in the folder");
        assert!(under("docs/research/a.md", "docs/research/"), "with a trailing slash");
        assert!(!under("docs/researchers.md", "docs/research"), "not a longer name");
        assert!(under("a.rs", "a.rs"), "the file itself");
    }
}

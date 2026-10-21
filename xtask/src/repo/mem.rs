//! Test doubles: an in-memory repository for the rule tests, and a scratch git repository for
//! the tests of `GitRepo` itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Change, ChangeKind, FileDiff, GIT_LOCATION_ENV, Repo, RepoError};

/// A repository held in memory: the head tree, the changed files and their diffs.
#[derive(Debug, Default, Clone)]
pub(crate) struct MemRepo {
    files: BTreeMap<String, String>,
    changes: Vec<Change>,
    diffs: BTreeMap<String, FileDiff>,
    subjects: Vec<(String, String)>,
}

impl MemRepo {
    /// Puts a file in the head tree without marking it changed.
    pub(crate) fn file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(path.to_owned(), content.to_owned());
        self
    }

    /// Puts a file in the head tree and marks it added or modified, with every line added.
    pub(crate) fn changed(mut self, path: &str, content: &str) -> Self {
        let kind =
            if self.files.contains_key(path) { ChangeKind::Modified } else { ChangeKind::Added };
        let diff = FileDiff {
            lines: content.lines().map(str::to_owned).collect(),
            added_at: (1..=content.lines().count()).collect(),
        };
        self.files.insert(path.to_owned(), content.to_owned());
        self.diffs.insert(path.to_owned(), diff);
        self.changes.push(Change { path: path.to_owned(), kind });
        self
    }

    /// Puts a file in the head tree and marks it modified, with only these head lines
    /// (1-based) added or changed.
    pub(crate) fn edited(mut self, path: &str, content: &str, changed: &[usize]) -> Self {
        let lines: Vec<&str> = content.lines().collect();
        let diff = FileDiff {
            lines: changed
                .iter()
                .filter_map(|at| at.checked_sub(1).and_then(|index| lines.get(index)))
                .map(|&line| line.to_owned())
                .collect(),
            added_at: changed.to_vec(),
        };
        self.files.insert(path.to_owned(), content.to_owned());
        self.diffs.insert(path.to_owned(), diff);
        self.changes.push(Change { path: path.to_owned(), kind: ChangeKind::Modified });
        self
    }

    /// Marks a file modified with exactly these added and removed lines, at no particular
    /// head line. No lines stands for a binary or mode-only change.
    pub(crate) fn changed_with_lines(mut self, path: &str, lines: &[&str]) -> Self {
        let diff = FileDiff {
            lines: lines.iter().map(|&line| line.to_owned()).collect(),
            added_at: Vec::new(),
        };
        self.diffs.insert(path.to_owned(), diff);
        self.changes.push(Change { path: path.to_owned(), kind: ChangeKind::Modified });
        self
    }

    /// Marks a file deleted at the head.
    pub(crate) fn deleted(mut self, path: &str) -> Self {
        self.files.remove(path);
        self.changes.push(Change { path: path.to_owned(), kind: ChangeKind::Deleted });
        self
    }

    /// Adds a commit subject to `base..head`.
    pub(crate) fn commit(mut self, subject: &str) -> Self {
        let sha = format!("{:07x}", self.subjects.len());
        self.subjects.push((sha, subject.to_owned()));
        self
    }
}

impl Repo for MemRepo {
    fn read_head(&self, path: &str) -> Result<Option<String>, RepoError> {
        Ok(self.files.get(path).cloned())
    }

    fn list_dirs_head(&self, dir: &str) -> Result<Vec<String>, RepoError> {
        let dir = dir.trim_end_matches('/');
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut dirs: Vec<String> = self
            .files
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter_map(|rest| rest.split_once('/').map(|(first, _)| first.to_owned()))
            .collect();
        dirs.dedup();
        Ok(dirs)
    }

    fn grep_head(&self, needle: &str, dir: &str) -> Result<Vec<String>, RepoError> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        Ok(self
            .files
            .iter()
            .filter(|(path, _)| dir.is_empty() || path.starts_with(&prefix))
            .filter(|(_, content)| contains_word(content, needle))
            .map(|(path, _)| path.clone())
            .collect())
    }

    fn changed_files(&self) -> Result<Vec<Change>, RepoError> {
        Ok(self.changes.clone())
    }

    fn file_diff(&self, path: &str) -> Result<FileDiff, RepoError> {
        Ok(self.diffs.get(path).cloned().unwrap_or_default())
    }

    fn commit_subjects(&self) -> Result<Vec<(String, String)>, RepoError> {
        Ok(self.subjects.iter().rev().cloned().collect())
    }
}

/// Whether `needle` occurs in `haystack` with no word character on either side, as `git grep
/// -w` matches.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    haystack.match_indices(needle).any(|(at, found)| {
        let before = haystack.get(..at).and_then(|s| s.chars().next_back());
        let after = haystack.get(at + found.len()..).and_then(|s| s.chars().next());
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// A git repository in a fresh temporary directory, removed on drop.
#[derive(Debug)]
pub(crate) struct ScratchGit {
    dir: PathBuf,
}

impl ScratchGit {
    /// Creates and initialises a repository whose default branch is `main`.
    pub(crate) fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("eileen-xtask-{label}-{}-{n}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("a stale scratch directory can be removed");
        }
        std::fs::create_dir_all(&dir).expect("the scratch directory can be created");
        let git = Self { dir };
        git.run(&["init", "--quiet", "--initial-branch=main"]);
        git
    }

    /// The working tree.
    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    /// Writes a file, creating its directories.
    pub(crate) fn write(&self, path: &str, content: &str) {
        let full = self.dir.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent directories can be created");
        }
        std::fs::write(full, content).expect("the scratch file can be written");
    }

    /// Removes a file from the working tree and the index.
    pub(crate) fn remove(&self, path: &str) {
        self.run(&["rm", "--quiet", path]);
    }

    /// Commits everything in the working tree.
    pub(crate) fn commit(&self, subject: &str) {
        self.run(&["add", "--all"]);
        self.run(&["commit", "--quiet", "--no-verify", "-m", subject]);
    }

    /// Creates a branch at the current commit and checks it out.
    pub(crate) fn branch(&self, name: &str) {
        self.run(&["checkout", "--quiet", "-b", name]);
    }

    /// Checks out an existing branch.
    pub(crate) fn checkout(&self, name: &str) {
        self.run(&["checkout", "--quiet", name]);
    }

    /// Runs git in the scratch repository with a fixed identity and no user configuration that
    /// could change its behaviour (hooks, signing, line ending conversion).
    fn run(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "user.name=xtask test", "-c", "user.email=xtask-test@example.invalid"])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(["-c", "core.hooksPath=.git/no-hooks"])
            .args(args);
        for name in GIT_LOCATION_ENV {
            command.env_remove(name);
        }
        let output = command.output().expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for ScratchGit {
    fn drop(&mut self) {
        // Best effort: a leftover directory in the temporary folder is harmless, and a test that
        // already failed should report its own failure, not this one.
        std::fs::remove_dir_all(&self.dir).unwrap_or_default();
    }
}

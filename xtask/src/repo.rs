//! What the checks may know about the repository: files at the head commit, the files and lines
//! the pull request changes, and its commit subjects. `GitRepo` asks `git`; tests use
//! [`mem::MemRepo`].

#[cfg(test)]
pub(crate) mod mem;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// How a changed file changed between the merge base and the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeKind {
    /// The file is new.
    Added,
    /// The file exists on both sides with different content or mode.
    Modified,
    /// The file is gone at the head.
    Deleted,
}

/// One changed file. Renames appear as a deletion and an addition, so both paths count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Change {
    /// The path from the repository root, with `/` separators.
    pub(crate) path: String,
    /// How it changed.
    pub(crate) kind: ChangeKind,
}

/// A git command failed or printed something unexpected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoError(pub(crate) String);

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Read-only questions the pull request checks ask about the repository.
pub(crate) trait Repo {
    /// The content of a file at the head commit, or `None` when it does not exist there.
    fn read_head(&self, path: &str) -> Result<Option<String>, RepoError>;
    /// The names of the directories directly inside `dir` at the head commit.
    fn list_dirs_head(&self, dir: &str) -> Result<Vec<String>, RepoError>;
    /// The files under `dir` at the head commit that contain `needle` as whole words, like
    /// `git grep -w -F`.
    fn grep_head(&self, needle: &str, dir: &str) -> Result<Vec<String>, RepoError>;
    /// The files the pull request changes, from the merge base of base and head to the head.
    fn changed_files(&self) -> Result<Vec<Change>, RepoError>;
    /// The added and removed lines of one changed file, without their `+` or `-` marker.
    fn changed_lines(&self, path: &str) -> Result<Vec<String>, RepoError>;
    /// `(short sha, subject)` of each commit in `base..head`, newest first.
    fn commit_subjects(&self) -> Result<Vec<(String, String)>, RepoError>;
}

/// The repository as `git` sees it, between two resolved commits.
#[derive(Debug)]
pub(crate) struct GitRepo {
    root: PathBuf,
    base: String,
    head: String,
}

impl GitRepo {
    /// Resolves `base` and `head` to commits in the repository at `root`.
    pub(crate) fn open(root: &Path, base: &str, head: &str) -> Result<Self, RepoError> {
        let mut repo = Self { root: root.to_owned(), base: String::new(), head: String::new() };
        repo.base = repo.resolve(base, "base")?;
        repo.head = repo.resolve(head, "head")?;
        Ok(repo)
    }

    /// The resolved head commit.
    pub(crate) fn head(&self) -> &str {
        &self.head
    }

    /// The resolved base commit.
    pub(crate) fn base(&self) -> &str {
        &self.base
    }

    fn resolve(&self, rev: &str, role: &str) -> Result<String, RepoError> {
        let spec = format!("{rev}^{{commit}}");
        let output = self.git(&["rev-parse", "--verify", "--quiet", spec.as_str()])?;
        if !output.status.success() {
            return Err(RepoError(format!(
                "the {role} revision `{rev}` is not a commit in this clone (fetch it, or pass \
                 --{role} <sha>; a shallow clone needs `git fetch --unshallow`)"
            )));
        }
        Ok(stdout_text(&output).trim().to_owned())
    }

    fn git(&self, args: &[&str]) -> Result<Output, RepoError> {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.quotePath=false"])
            .args(args)
            .output()
            .map_err(|error| RepoError(format!("cannot run git: {error}")))
    }

    /// Runs git and fails unless it exits with one of `ok_codes`.
    fn git_ok(&self, args: &[&str], ok_codes: &[i32]) -> Result<Output, RepoError> {
        let output = self.git(args)?;
        match output.status.code() {
            Some(code) if ok_codes.contains(&code) => Ok(output),
            Some(_) | None => Err(RepoError(format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))),
        }
    }

    fn range(&self) -> String {
        format!("{}...{}", self.base, self.head)
    }
}

impl Repo for GitRepo {
    fn read_head(&self, path: &str) -> Result<Option<String>, RepoError> {
        let object = format!("{}:{path}", self.head);
        let exists = self.git(&["cat-file", "-e", &object])?;
        if !exists.status.success() {
            return Ok(None);
        }
        let output = self.git_ok(&["cat-file", "blob", &object], &[0])?;
        Ok(Some(stdout_text(&output).replace("\r\n", "\n")))
    }

    fn list_dirs_head(&self, dir: &str) -> Result<Vec<String>, RepoError> {
        let dir = dir.trim_end_matches('/');
        let pathspec = format!("{dir}/");
        let mut args = vec!["ls-tree", "-z", "-d", "--name-only", self.head.as_str()];
        if !dir.is_empty() {
            args.extend(["--", pathspec.as_str()]);
        }
        let output = self.git_ok(&args, &[0])?;
        Ok(nul_fields(&output)
            .filter_map(|path| path.rsplit('/').next().map(str::to_owned))
            .collect())
    }

    fn grep_head(&self, needle: &str, dir: &str) -> Result<Vec<String>, RepoError> {
        let mut args = vec!["grep", "-z", "-l", "-w", "-F", "-e", needle, self.head.as_str()];
        if !dir.is_empty() {
            args.extend(["--", dir]);
        }
        // git grep exits with 1 when nothing matches.
        let output = self.git_ok(&args, &[0, 1])?;
        let prefix = format!("{}:", self.head);
        Ok(nul_fields(&output)
            .map(|line| line.strip_prefix(&prefix).unwrap_or(line).to_owned())
            .collect())
    }

    fn changed_files(&self) -> Result<Vec<Change>, RepoError> {
        let range = self.range();
        let output = self.git_ok(&["diff", "--name-status", "--no-renames", "-z", &range], &[0])?;
        let mut fields = nul_fields(&output);
        let mut changes = Vec::new();
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            let kind = match status.chars().next() {
                Some('A') => ChangeKind::Added,
                Some('D') => ChangeKind::Deleted,
                Some(_) | None => ChangeKind::Modified,
            };
            changes.push(Change { path: path.to_owned(), kind });
        }
        Ok(changes)
    }

    fn changed_lines(&self, path: &str) -> Result<Vec<String>, RepoError> {
        let range = self.range();
        let args = [
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-color",
            "-U0",
            range.as_str(),
            "--",
            path,
        ];
        let output = self.git_ok(&args, &[0])?;
        Ok(diff_lines(&stdout_text(&output)))
    }

    fn commit_subjects(&self) -> Result<Vec<(String, String)>, RepoError> {
        let range = format!("{}..{}", self.base, self.head);
        let output = self.git_ok(&["log", "-z", "--format=%h%x1f%s", &range], &[0])?;
        Ok(nul_fields(&output)
            .filter_map(|record| record.split_once('\u{1f}'))
            .map(|(sha, subject)| (sha.trim().to_owned(), subject.to_owned()))
            .collect())
    }
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The NUL-separated fields of a `-z` git output, without empty ones.
fn nul_fields(output: &Output) -> impl Iterator<Item = &str> {
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .filter_map(|field| std::str::from_utf8(field).ok())
}

/// The added and removed lines of a one-file unified diff, without their markers. Everything
/// before the first hunk header is the file header and is skipped.
fn diff_lines(diff: &str) -> Vec<String> {
    diff.lines()
        .skip_while(|line| !line.starts_with("@@"))
        .filter(|line| !line.starts_with("@@"))
        .filter_map(|line| line.strip_prefix('+').or_else(|| line.strip_prefix('-')))
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::mem::ScratchGit;

    #[test]
    fn diff_lines_skip_headers_and_context_markers() {
        let diff = "diff --git a/x.yml b/x.yml\nindex 1..2 100644\n--- a/x.yml\n+++ b/x.yml\n\
                    @@ -3 +3 @@ jobs:\n-      - uses: a/b@111 # v1\n+      - uses: a/b@222 # v2\n\
                    \\ No newline at end of file\n";
        assert_eq!(
            diff_lines(diff),
            vec!["      - uses: a/b@111 # v1".to_owned(), "      - uses: a/b@222 # v2".to_owned()],
            "only the changed lines, markers removed"
        );
    }

    #[test]
    fn git_repo_reads_the_head_tree_and_the_merge_base_diff() {
        let git = ScratchGit::new("reads-head");
        git.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        git.write("crates/alpha/src/lib.rs", "fn kept() {}\n");
        git.write("docs/old.md", "old\n");
        git.commit("Start");
        git.branch("topic");
        git.write("crates/alpha/src/lib.rs", "fn kept() {}\n#[test]\nfn added_test() {}\n");
        git.remove("docs/old.md");
        git.write("docs/new.md", "new\n");
        git.commit("Change on the branch");
        git.checkout("main");
        git.write("unrelated.txt", "main moved on\n");
        git.commit("Main moves on");
        git.checkout("topic");

        let repo = GitRepo::open(git.path(), "main", "topic").expect("both revisions resolve");
        let mut changes = repo.changed_files().expect("git diff runs");
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            changes,
            vec![
                Change { path: "crates/alpha/src/lib.rs".to_owned(), kind: ChangeKind::Modified },
                Change { path: "docs/new.md".to_owned(), kind: ChangeKind::Added },
                Change { path: "docs/old.md".to_owned(), kind: ChangeKind::Deleted },
            ],
            "changes are counted from the merge base, so main's own commit is not among them"
        );
        assert_eq!(repo.list_dirs_head("crates").expect("ls-tree runs"), vec!["alpha".to_owned()]);
        assert_eq!(
            repo.grep_head("fn added_test", "crates/alpha").expect("git grep runs"),
            vec!["crates/alpha/src/lib.rs".to_owned()],
            "grep searches the head commit"
        );
        assert_eq!(
            repo.grep_head("fn missing", "crates").expect("no match is not an error"),
            Vec::<String>::new(),
            "git grep's exit code 1 means no match"
        );
        assert_eq!(repo.read_head("docs/old.md").expect("cat-file runs"), None, "deleted at head");
        assert_eq!(
            repo.changed_lines("crates/alpha/src/lib.rs").expect("diff runs"),
            vec!["#[test]".to_owned(), "fn added_test() {}".to_owned()],
            "added lines without markers"
        );
        let subjects = repo.commit_subjects().expect("git log runs");
        assert_eq!(subjects.len(), 1, "one commit on the branch since main: {subjects:?}");
        assert_eq!(subjects.first().map(|(_, s)| s.as_str()), Some("Change on the branch"));
    }

    #[test]
    fn git_repo_names_a_missing_base() {
        let git = ScratchGit::new("missing-base");
        git.write("README.md", "x\n");
        git.commit("Start");
        let error = GitRepo::open(git.path(), "origin/does-not-exist", "HEAD")
            .expect_err("an unknown base cannot resolve");
        assert!(
            error.0.contains("origin/does-not-exist"),
            "the message names the revision: {error}"
        );
    }
}

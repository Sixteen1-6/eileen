//! What the checks may know about the repository: files at the head commit, the files and lines
//! the pull request changes, and its commit subjects. `GitRepo` asks `git`; tests use
//! [`mem::MemRepo`].

#[cfg(test)]
pub(crate) mod mem;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The environment variables git exports to hooks that would point a git command at another
/// repository than the one `-C` names. Every git command xtask starts removes them, so a run
/// from a hook in a worktree reads the same repository as a run from the shell.
pub(crate) const GIT_LOCATION_ENV: [&str; 4] =
    ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"];

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

/// What one changed file's diff says, read from `git diff -U0`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileDiff {
    /// The added and removed lines, without their `+` or `-` marker. Empty for a binary or
    /// mode-only change, which has no text hunk.
    pub(crate) lines: Vec<String>,
    /// The head-side line numbers (1-based) of the added lines.
    pub(crate) added_at: Vec<usize>,
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
    /// The changed lines of one changed file, and where its added lines sit at the head.
    fn file_diff(&self, path: &str) -> Result<FileDiff, RepoError>;
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

    /// A git command for this repository: `-C` locates it, the location variables a hook
    /// inherits are removed, and the user settings that would change the output xtask parses
    /// (quoted paths, colour, signatures in `git log`) are fixed.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.root);
        command.args(["-c", "core.quotePath=false", "-c", "color.ui=never"]);
        command.args(["-c", "log.showSignature=false"]);
        command.args(args);
        for name in GIT_LOCATION_ENV {
            command.env_remove(name);
        }
        command
    }

    fn git(&self, args: &[&str]) -> Result<Output, RepoError> {
        self.command(args).output().map_err(|error| RepoError(format!("cannot run git: {error}")))
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
        Ok(nul_fields(&output)?
            .into_iter()
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
        Ok(nul_fields(&output)?
            .into_iter()
            .map(|line| line.strip_prefix(&prefix).unwrap_or(line).to_owned())
            .collect())
    }

    fn changed_files(&self) -> Result<Vec<Change>, RepoError> {
        let range = self.range();
        let output = self.git_ok(&["diff", "--name-status", "--no-renames", "-z", &range], &[0])?;
        name_status_changes(&nul_fields(&output)?)
    }

    fn file_diff(&self, path: &str) -> Result<FileDiff, RepoError> {
        let range = self.range();
        let args = [
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "-U0",
            range.as_str(),
            "--",
            path,
        ];
        let output = self.git_ok(&args, &[0])?;
        Ok(parse_diff(&stdout_text(&output)))
    }

    fn commit_subjects(&self) -> Result<Vec<(String, String)>, RepoError> {
        let range = format!("{}..{}", self.base, self.head);
        let output = self.git_ok(&["log", "-z", "--format=%h%x1f%s", &range], &[0])?;
        // Subjects are only reported, so an odd byte in one is shown, never fatal.
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .map(String::from_utf8_lossy)
            .filter_map(|record| {
                record
                    .split_once('\u{1f}')
                    .map(|(sha, subject)| (sha.trim().to_owned(), subject.to_owned()))
            })
            .collect())
    }
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The NUL-separated fields of a `-z` git output, without empty ones. A field that is not UTF-8
/// is an error, never skipped, because callers pair fields by position.
fn nul_fields(output: &Output) -> Result<Vec<&str>, RepoError> {
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            std::str::from_utf8(field).map_err(|_| {
                RepoError(format!(
                    "git reported a path that is not UTF-8: {}",
                    String::from_utf8_lossy(field)
                ))
            })
        })
        .collect()
}

/// The `(status, path)` pairs of `git diff --name-status --no-renames -z`, as changes.
fn name_status_changes(fields: &[&str]) -> Result<Vec<Change>, RepoError> {
    let (pairs, rest) = fields.as_chunks::<2>();
    if let [status] = rest {
        return Err(RepoError(format!("git diff --name-status printed `{status}` with no path")));
    }
    Ok(pairs
        .iter()
        .map(|[status, path]| {
            let kind = match status.chars().next() {
                Some('A') => ChangeKind::Added,
                Some('D') => ChangeKind::Deleted,
                Some(_) | None => ChangeKind::Modified,
            };
            Change { path: (*path).to_owned(), kind }
        })
        .collect())
}

/// Reads a one-file unified diff: the added and removed lines without their markers, and the
/// head-side line number of each added line, counted from each hunk header's `+<start>`.
/// Everything before the first hunk header is the file header and is skipped, so a binary or
/// mode-only change, which has no hunk, gives no lines.
fn parse_diff(diff: &str) -> FileDiff {
    let mut parsed = FileDiff::default();
    let mut head_line: Option<usize> = None;
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            head_line = hunk_head_start(header);
            continue;
        }
        let Some(at) = head_line.as_mut() else {
            continue;
        };
        if let Some(added) = line.strip_prefix('+') {
            parsed.lines.push(added.to_owned());
            parsed.added_at.push(*at);
            *at += 1;
        } else if let Some(removed) = line.strip_prefix('-') {
            parsed.lines.push(removed.to_owned());
        } else if line.starts_with(' ') {
            *at += 1;
        }
    }
    parsed
}

/// The head-side start line in the rest of a hunk header, `-a,b +c,d @@ ...`.
fn hunk_head_start(header: &str) -> Option<usize> {
    let head = header.split_whitespace().find_map(|part| part.strip_prefix('+'))?;
    head.split(',').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::mem::ScratchGit;

    #[test]
    fn diff_lines_skip_headers_and_count_head_lines() {
        let diff = "diff --git a/x.yml b/x.yml\nindex 1..2 100644\n--- a/x.yml\n+++ b/x.yml\n\
                    @@ -3 +3 @@ jobs:\n-      - uses: a/b@111 # v1\n+      - uses: a/b@222 # v2\n\
                    \\ No newline at end of file\n@@ -9,0 +10,2 @@\n+++ two plus signs\n+b\n";
        assert_eq!(
            parse_diff(diff),
            FileDiff {
                lines: vec![
                    "      - uses: a/b@111 # v1".to_owned(),
                    "      - uses: a/b@222 # v2".to_owned(),
                    "++ two plus signs".to_owned(),
                    "b".to_owned(),
                ],
                added_at: vec![3, 10, 11],
            },
            "only the changed lines, markers removed, added lines placed by their hunk"
        );
        let binary = "diff --git a/a.png b/a.png\nindex 1..2 100644\n\
                      Binary files a/a.png and b/a.png differ\n";
        assert_eq!(parse_diff(binary), FileDiff::default(), "a binary change has no text lines");
        let mode = "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n";
        assert_eq!(parse_diff(mode), FileDiff::default(), "a mode change has no text lines");
    }

    #[test]
    fn a_path_that_is_not_utf8_is_an_error_not_a_shifted_pair() {
        let output = Output {
            status: std::process::ExitStatus::default(),
            stdout: b"A\0bad-\xff.rs\0M\0crates/x/src/lib.rs\0".to_vec(),
            stderr: Vec::new(),
        };
        let error = nul_fields(&output).expect_err("an invalid byte must not be dropped");
        assert!(error.0.contains("not UTF-8"), "the message says why: {error}");
        assert_eq!(
            name_status_changes(&["A", "docs/a.md", "M", "crates/x/src/lib.rs"]),
            Ok(vec![
                Change { path: "docs/a.md".to_owned(), kind: ChangeKind::Added },
                Change { path: "crates/x/src/lib.rs".to_owned(), kind: ChangeKind::Modified },
            ]),
            "fields pair up as status and path"
        );
        assert!(name_status_changes(&["A"]).is_err(), "a status with no path is an error");
    }

    #[test]
    fn git_commands_ignore_the_hook_environment_and_signatures() {
        let repo = GitRepo { root: PathBuf::from("."), base: String::new(), head: String::new() };
        let command = repo.command(&["log"]);
        for name in GIT_LOCATION_ENV {
            assert!(
                command.get_envs().any(|(key, value)| key == name && value.is_none()),
                "{name} is removed, so `-C` decides the repository"
            );
        }
        let args: Vec<String> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.iter().any(|arg| arg == "log.showSignature=false"), "{args:?}");
        assert!(args.iter().any(|arg| arg == "color.ui=never"), "{args:?}");
    }

    #[test]
    fn git_repo_reads_the_head_tree_and_the_merge_base_diff() {
        let git = ScratchGit::new("reads-head");
        git.write("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
        git.write("crates/alpha/src/lib.rs", "fn kept() {}\n");
        git.write("docs/old.md", "old\n");
        git.write("assets/a.png", "PNG\u{0}one\n");
        git.commit("Start");
        git.branch("topic");
        git.write("crates/alpha/src/lib.rs", "fn kept() {}\n#[test]\nfn added_test() {}\n");
        git.remove("docs/old.md");
        git.write("docs/new.md", "new\n");
        git.write("assets/a.png", "PNG\u{0}two\n");
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
                Change { path: "assets/a.png".to_owned(), kind: ChangeKind::Modified },
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
            repo.file_diff("crates/alpha/src/lib.rs").expect("diff runs"),
            FileDiff {
                lines: vec!["#[test]".to_owned(), "fn added_test() {}".to_owned()],
                added_at: vec![2, 3],
            },
            "added lines without markers, at their head line numbers"
        );
        assert_eq!(
            repo.file_diff("assets/a.png").expect("diff runs"),
            FileDiff::default(),
            "git reports a binary change with no text lines"
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

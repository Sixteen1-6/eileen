//! The file-system root the machine's facts are read under.

use std::io;
use std::path::{Path, PathBuf};

/// The root that `proc/`, `sys/`, `etc/`, `usr/share/` and `var/lib/` are read under: `/` on a
/// real machine, a recorded or synthetic tree in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysRoot {
    root: PathBuf,
}

impl SysRoot {
    /// The real root, `/`.
    #[must_use]
    pub fn system() -> Self {
        Self { root: PathBuf::from("/") }
    }

    /// The tree rooted at `root`.
    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `rel` (a path with or without a leading slash, such as `proc/meminfo`) under the root.
    #[must_use]
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    /// The text of `rel`.
    pub(crate) fn read(&self, rel: &str) -> io::Result<String> {
        std::fs::read_to_string(self.path(rel))
    }

    /// The text of `rel` with surrounding whitespace removed, or `None` when it cannot be read.
    pub(crate) fn read_trimmed(&self, rel: &str) -> Option<String> {
        self.read(rel).ok().map(|text| text.trim().to_owned())
    }

    /// Whether `rel` exists, following symbolic links.
    pub(crate) fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    /// The entry names of the folder `rel`, sorted; empty when it cannot be read.
    pub(crate) fn entries(&self, rel: &str) -> Vec<String> {
        let Ok(dir) = std::fs::read_dir(self.path(rel)) else {
            return Vec::new();
        };
        let mut names: Vec<String> =
            dir.filter_map(|entry| entry.ok()?.file_name().into_string().ok()).collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::SysRoot;

    #[test]
    fn paths_join_under_the_root_with_or_without_a_leading_slash() {
        let root = SysRoot::at("/fixture");
        assert_eq!(root.path("proc/meminfo"), root.path("/proc/meminfo"), "same path");
        assert!(root.path("proc/meminfo").starts_with("/fixture"), "under the root");
        assert_eq!(SysRoot::system().path("proc/meminfo").to_str(), Some("/proc/meminfo"));
    }

    #[test]
    fn a_missing_folder_has_no_entries_and_a_missing_file_no_text() {
        let root = SysRoot::at("/nonexistent-eileen-root");
        assert!(root.entries("sys/class/drm").is_empty(), "no entries");
        assert!(root.read_trimmed("proc/meminfo").is_none(), "no text");
        assert!(!root.exists("dev/dxg"), "does not exist");
    }
}

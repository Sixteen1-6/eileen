//! An in-memory tree for the fixture tests, starting from a clean repository that passes every
//! check, so each test seeds exactly one violation.

use std::collections::BTreeMap;

use super::{Tree, TreeError};

/// A tree of fixture files.
#[derive(Debug, Clone, Default)]
pub(crate) struct MemTree {
    files: BTreeMap<String, String>,
    paths: Vec<String>,
}

/// A crate doc with every heading of BUILD.md section 4b.4.
pub(crate) const GOOD_CRATE_DOC: &str = "\
//! A demo crate for the fixtures.
//!
//! # Owns
//!
//! - The demo.
//!
//! # Does not own
//!
//! - Anything else.
//!
//! # Invariants
//!
//! - None (a test says so).
//!
//! # Boundaries
//!
//! - Called by nobody; may depend on nothing.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p demo`.
//!
//! # Traps
//!
//! - None.
//!
//! # Logs and tracing
//!
//! - None.
//!
//! # Do not change without a decision record
//!
//! - Nothing.

#![forbid(unsafe_code)]

/// The answer.
pub const ANSWER: u8 = 42;
";

impl MemTree {
    /// A small repository that passes every check: a workspace of one crate, the root
    /// guidance pair and the tidy files.
    pub(crate) fn clean() -> Self {
        let mut tree = Self::default();
        tree.set(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.lints.rust]\nunsafe_code = \"deny\"\n\n\
             [workspace.lints.clippy]\nundocumented_unsafe_blocks = \"deny\"\n\
             multiple_unsafe_ops_per_block = \"deny\"\n",
        );
        tree.set("crates/demo/Cargo.toml", "[package]\nname = \"demo\"\n");
        tree.set("crates/demo/src/lib.rs", GOOD_CRATE_DOC);
        tree.set(
            "crates/demo/src/main.rs",
            "//! The demo binary; its guidance is the crate doc.\n\nfn main() {}\n",
        );
        tree.set(
            "crates/demo/tests/it/main.rs",
            "//! Integration tests.\n\n#[test]\nfn it_works() {\n    assert_eq!(1, 1, \"one\");\n}\n",
        );
        tree.set("AGENTS.md", "# Guide\n\nRun `cargo xtask lint-repo` before pushing.\n");
        tree.set("CLAUDE.md", "@AGENTS.md\n\n# Claude only\n");
        tree.set("docs/a.md", "Plain prose, with a comma, a colon: and a hyphen-word.\n");
        tree.set(
            "xtask/tidy/denylist.toml",
            "salt = \"00112233445566778899aabbccddeeff\"\nentries = []\n",
        );
        tree.set(
            "xtask/tidy/exempt.toml",
            "[[exempt]]\npath = \"docs/research\"\nreason = \"quotes third-party text\"\n",
        );
        tree.set("xtask/tidy/allow.toml", "module = []\n");
        tree.set(
            "xtask/tidy/guidance.toml",
            "[[nested]]\ndir = \"fuzz\"\nreason = \"nightly toolchain\"\n\n\
             [[ignore]]\ndir = \"templates\"\nreason = \"copies for other projects\"\n",
        );
        tree.set(
            "xtask/tidy/disallowed-methods.toml",
            "[[exempt]]\npath = \"xtask\"\nreason = \"the development tool\"\n\n\
             [[site]]\npath = \"crates/demo/src/spawn.rs\"\nfunction = \"spawn_child\"\n\
             spawns = true\nreason = \"the demo's one spawn\"\n",
        );
        tree.set(
            "xtask/tidy/credential-boundary.toml",
            "paths = [\".vendor-cli/secret.json\"]\ncommands = [\"vendor mcp add\"]\n\
             extensions = [\"rs\", \"toml\", \"json\"]\n\n\
             [[exempt]]\npath = \"xtask/tidy\"\nreason = \"the list itself\"\n",
        );
        tree
    }

    /// Adds or replaces a file.
    pub(crate) fn set(&mut self, path: &str, text: &str) {
        self.files.insert(path.to_owned(), text.to_owned());
        self.paths = self.files.keys().cloned().collect();
    }

    /// Removes a file.
    pub(crate) fn remove(&mut self, path: &str) {
        self.files.remove(path);
        self.paths = self.files.keys().cloned().collect();
    }
}

impl Tree for MemTree {
    fn files(&self) -> &[String] {
        &self.paths
    }

    fn read(&self, path: &str) -> Result<Option<String>, TreeError> {
        Ok(self.files.get(path).cloned())
    }
}

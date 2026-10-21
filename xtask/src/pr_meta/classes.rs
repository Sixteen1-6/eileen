//! The path classes of BUILD.md section 4.6 that decide whether a type fits its files, and the
//! rule for what a `deps` pull request may touch.
//!
//! The `plan` job in `.github/workflows/ci.yml` computes the same classes with
//! `dorny/paths-filter`; the test `ci_filters_match_the_classifier` holds the two together.

/// A change to one of these selects every class, and so every lane: any `Cargo.toml`,
/// `Cargo.lock` or `rust-toolchain.toml`, and anything under `.cargo/`, `.config/` or `.github/`.
pub(crate) fn selects_every_class(path: &str) -> bool {
    is_dependency_manifest(path)
        || file_name(path) == "rust-toolchain.toml"
        || [".cargo/", ".config/", ".github/"].iter().any(|dir| path.starts_with(dir))
}

/// Markdown anywhere, or anything under `docs/`. The extension is compared case-sensitively,
/// as the plan job's `**/*.md` glob does.
fn is_docs(path: &str) -> bool {
    file_name(path).rsplit_once('.').is_some_and(|(_, extension)| extension == "md")
        || path.starts_with("docs/")
}

/// Anything under `scenarios/`.
fn is_scenario(path: &str) -> bool {
    path.starts_with("scenarios/")
}

/// A `Cargo.toml` or `Cargo.lock` anywhere in the tree.
pub(crate) fn is_dependency_manifest(path: &str) -> bool {
    matches!(file_name(path), "Cargo.toml" | "Cargo.lock")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The two classes the type rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Classes {
    /// Every changed file is Markdown or under `docs/`, and none selects every class.
    pub(crate) docs_only: bool,
    /// Every changed file is under `scenarios/`, and none selects every class.
    pub(crate) scenarios_only: bool,
}

/// Classifies a pull request by the paths it changes.
pub(crate) fn classify<'a>(paths: impl IntoIterator<Item = &'a str> + Clone) -> Classes {
    let narrow = !paths.clone().into_iter().any(selects_every_class);
    Classes {
        docs_only: narrow && paths.clone().into_iter().all(is_docs),
        scenarios_only: narrow && paths.into_iter().all(is_scenario),
    }
}

/// Whether a changed line of a workflow is an action pin, the only kind of line a `deps` pull
/// request may change outside the manifests and lockfiles. Blank lines are allowed too.
pub(crate) fn is_pin_line(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix('-').map_or(line, str::trim_start);
    line.is_empty() || line.starts_with("uses:")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `plan` job's filters, exactly as `.github/workflows/ci.yml` writes them. They mirror the
    /// predicates above: `every_class` is [`selects_every_class`], and a pull request is
    /// docs-only when no file is `not_docs` (and none is `every_class`), likewise for scenarios.
    const CI_EVERY_CLASS: &str = "\
          filters: |
            every_class:
              - 'Cargo.toml'
              - '**/Cargo.toml'
              - 'Cargo.lock'
              - '**/Cargo.lock'
              - 'rust-toolchain.toml'
              - '**/rust-toolchain.toml'
              - '.cargo/**'
              - '.config/**'
              - '.github/**'
";
    const CI_OUTSIDE: &str = "\
          predicate-quantifier: every
          filters: |
            not_docs:
              - '**'
              - '!**/*.md'
              - '!docs/**'
            not_scenarios:
              - '**'
              - '!scenarios/**'
";

    #[test]
    fn ci_filters_match_the_classifier() {
        let ci = include_str!("../../../.github/workflows/ci.yml").replace("\r\n", "\n");
        assert!(
            ci.contains(CI_EVERY_CLASS),
            "ci.yml's every_class filter changed; update classes.rs"
        );
        assert!(ci.contains(CI_OUTSIDE), "ci.yml's not_docs or not_scenarios filter changed");
        let samples = [
            ("Cargo.toml", true),
            ("crates/eileen-core/Cargo.toml", true),
            ("fuzz/Cargo.lock", true),
            ("rust-toolchain.toml", true),
            ("fuzz/rust-toolchain.toml", true),
            (".cargo/config.toml", true),
            (".config/nextest.toml", true),
            (".github/PULL_REQUEST_TEMPLATE.md", true),
            ("docs/BUILD.md", false),
            ("xtask/src/lib.rs", false),
            ("Cargo.toml.orig", false),
        ];
        for (path, every) in samples {
            assert_eq!(selects_every_class(path), every, "every_class for {path}");
        }
    }

    #[test]
    fn docs_only_means_markdown_or_docs_and_nothing_that_selects_every_class() {
        let docs = classify(["docs/BUILD.md", "README.md", "docs/research/data.csv"]);
        assert!(docs.docs_only && !docs.scenarios_only, "Markdown and docs/ files are docs-only");
        let template = classify([".github/PULL_REQUEST_TEMPLATE.md"]);
        assert!(!template.docs_only, "Markdown under .github/ selects every class");
        let code = classify(["docs/BUILD.md", "xtask/src/lib.rs"]);
        assert!(!code.docs_only, "a Rust file is not docs");
        let scenarios = classify(["scenarios/core/hello/scenario.toml", "scenarios/README.md"]);
        assert!(scenarios.scenarios_only, "files under scenarios/ are scenarios-only");
    }

    #[test]
    fn pin_lines_are_uses_lines() {
        assert!(is_pin_line("      - uses: actions/checkout@3d3c42e5 # v7.0.1"), "a step's uses");
        assert!(is_pin_line("    uses: ./.github/workflows/reusable.yml"), "a job's uses");
        assert!(is_pin_line(""), "a blank line");
        assert!(!is_pin_line("      - run: cargo test"), "a run step");
        assert!(!is_pin_line("        with:"), "a with key");
    }
}

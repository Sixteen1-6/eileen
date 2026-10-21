//! The path classes of BUILD.md section 4.6 that decide whether a type fits its files, and the
//! rule for what a `deps` pull request may touch.
//!
//! The `plan` job in `.github/workflows/ci.yml` computes the same classes with
//! `dorny/paths-filter`; the test `ci_filters_match_the_classifier` reads that job's filters,
//! evaluates them and holds the two together.

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

/// Whether a file may carry action pins: a workflow directly under `.github/workflows/`, or an
/// action's metadata file (`action.yml` or `action.yaml`) at the root or under
/// `.github/actions/`. Outside the manifests and lockfiles, these are the only files a `deps`
/// pull request may change.
pub(crate) fn may_hold_pins(path: &str) -> bool {
    let yaml = |name: &str| {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml")
    };
    let workflow = path
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| !name.contains('/') && yaml(name));
    let action = matches!(file_name(path), "action.yml" | "action.yaml")
        && (!path.contains('/') || path.starts_with(".github/actions/"));
    workflow || action
}

/// Whether a changed line of a workflow or action file is an action pin, `uses: <ref>`, the only
/// kind of line a `deps` pull request may change there.
pub(crate) fn is_pin_line(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix('-').map_or(line, str::trim_start);
    line.starts_with("uses:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pr_meta::resolve::wildcard;

    /// One `dorny/paths-filter` step of a workflow: whether its `predicate-quantifier` is
    /// `every`, and its filters by name with their patterns.
    #[derive(Debug)]
    struct FilterStep {
        every: bool,
        filters: Vec<(String, Vec<String>)>,
    }

    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Every `dorny/paths-filter` step in a workflow, read from its `with:` block: the
    /// quantifier, and under `filters: |` each `<name>:` with its `- '<pattern>'` lines.
    fn paths_filter_steps(workflow: &str) -> Vec<FilterStep> {
        let lines: Vec<&str> = workflow.lines().collect();
        let mut steps = Vec::new();
        for (at, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("uses: dorny/paths-filter@") {
                continue;
            }
            let indent = indent_of(line);
            let mut step = FilterStep { every: false, filters: Vec::new() };
            let mut in_filters = false;
            for line in lines.iter().skip(at + 1) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if indent_of(line) < indent {
                    break;
                }
                if trimmed == "predicate-quantifier: every" {
                    step.every = true;
                } else if trimmed.starts_with("filters:") {
                    in_filters = true;
                } else if let Some(pattern) = trimmed.strip_prefix("- ")
                    && let Some((_, patterns)) = step.filters.last_mut()
                {
                    patterns.push(pattern.trim_matches('\'').to_owned());
                } else if in_filters && let Some(name) = trimmed.strip_suffix(':') {
                    step.filters.push((name.to_owned(), Vec::new()));
                }
            }
            steps.push(step);
        }
        steps
    }

    /// A glob as `dorny/paths-filter` reads it (picomatch with `dot: true`): `**` is any number
    /// of whole segments, and `*` and `?` match within one segment.
    fn glob(pattern: &str, path: &str) -> bool {
        fn segments(pattern: &[&str], path: &[&str]) -> bool {
            match pattern.split_first() {
                None => path.is_empty(),
                Some((&"**", rest)) => {
                    segments(rest, path)
                        || path.split_first().is_some_and(|(_, tail)| segments(pattern, tail))
                }
                Some((first, rest)) => path
                    .split_first()
                    .is_some_and(|(name, tail)| wildcard(first, name) && segments(rest, tail)),
            }
        }
        let pattern: Vec<&str> = pattern.split('/').collect();
        let path: Vec<&str> = path.split('/').collect();
        segments(&pattern, &path)
    }

    /// Whether any of `paths` matches the named filter: one of its patterns, or, under the
    /// `every` quantifier, all of them, where `!pattern` matches a path the pattern does not.
    fn filter_hits(steps: &[FilterStep], name: &str, paths: &[&str]) -> bool {
        let (every, patterns) = steps
            .iter()
            .find_map(|step| {
                let found = step.filters.iter().find(|(filter, _)| filter == name);
                found.map(|(_, patterns)| (step.every, patterns))
            })
            .unwrap_or_else(|| panic!("ci.yml has no `{name}` paths filter"));
        let predicate = |path: &str, pattern: &String| match pattern.strip_prefix('!') {
            Some(negated) => !glob(negated, path),
            None => glob(pattern, path),
        };
        paths.iter().any(|path| {
            if every {
                patterns.iter().all(|pattern| predicate(path, pattern))
            } else {
                patterns.iter().any(|pattern| predicate(path, pattern))
            }
        })
    }

    #[test]
    fn ci_filters_match_the_classifier() {
        let ci = include_str!("../../../.github/workflows/ci.yml").replace("\r\n", "\n");
        let steps = paths_filter_steps(&ci);
        let names: Vec<&str> =
            steps.iter().flat_map(|s| s.filters.iter().map(|(name, _)| name.as_str())).collect();
        assert_eq!(names, vec!["every_class", "not_docs", "not_scenarios"], "{steps:#?}");
        assert!(
            steps.iter().all(|s| s.filters.iter().all(|(_, patterns)| !patterns.is_empty())),
            "every filter has patterns: {steps:#?}"
        );
        // The plan job's `Decide the lanes` step: docs-only when no file is outside the docs
        // class and none selects every class, and likewise for scenarios.
        let ci_classes = |paths: &[&str]| {
            let every_class = filter_hits(&steps, "every_class", paths);
            Classes {
                docs_only: !every_class && !filter_hits(&steps, "not_docs", paths),
                scenarios_only: !every_class && !filter_hits(&steps, "not_scenarios", paths),
            }
        };
        let singles = [
            "Cargo.toml",
            "crates/eileen-core/Cargo.toml",
            "fuzz/Cargo.lock",
            "rust-toolchain.toml",
            "fuzz/rust-toolchain.toml",
            ".cargo/config.toml",
            ".config/nextest.toml",
            ".github/PULL_REQUEST_TEMPLATE.md",
            ".github/x.md",
            "docs/BUILD.md",
            "docs/x.csv",
            "README.md",
            "xtask/README.md",
            "notes.txt",
            "src/a.MD",
            "a.md/b.txt",
            "xtask/src/lib.rs",
            "Cargo.toml.orig",
            "scenarios/core/a/scenario.toml",
            "scenarios/README.md",
            "scenario.toml",
            "xscenarios/a.toml",
        ];
        for path in singles {
            assert_eq!(
                selects_every_class(path),
                filter_hits(&steps, "every_class", &[path]),
                "every_class for {path}"
            );
            assert_eq!(classify([path]), ci_classes(&[path]), "the classes of {path}");
        }
        let sets: [&[&str]; 4] = [
            &["docs/BUILD.md", "README.md"],
            &["docs/BUILD.md", "xtask/src/lib.rs"],
            &["scenarios/core/a/scenario.toml", "scenarios/README.md"],
            &["README.md", "scenarios/README.md"],
        ];
        for paths in sets {
            assert_eq!(classify(paths.iter().copied()), ci_classes(paths), "{paths:?}");
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
        assert!(!is_pin_line(""), "a blank line is not a pin");
        assert!(!is_pin_line("      - run: cargo test"), "a run step");
        assert!(!is_pin_line("        with:"), "a with key");
    }

    #[test]
    fn pins_live_only_in_workflows_and_action_files() {
        assert!(may_hold_pins(".github/workflows/ci.yml"), "a workflow");
        assert!(may_hold_pins(".github/workflows/release.yaml"), "a .yaml workflow");
        assert!(may_hold_pins(".github/actions/setup/action.yml"), "a local action");
        assert!(may_hold_pins("action.yml"), "the root action");
        assert!(!may_hold_pins(".github/workflows/notes/x.yml"), "workflows are not nested");
        assert!(!may_hold_pins(".github/dependabot.yml"), "not a workflow");
        assert!(!may_hold_pins("xtask/src/lib.rs"), "Rust");
        assert!(!may_hold_pins("docs/action.yml"), "an action file outside .github/actions/");
    }
}

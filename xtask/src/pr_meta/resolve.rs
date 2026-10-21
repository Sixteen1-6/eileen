//! Resolving the ids an Evidence section lists, without compiling (BUILD.md check 8).
//!
//! A test id is `<package>::<module>::...::<fn>`. The package is any workspace member, found by
//! reading the root manifest's `members` and each member's package name at the head commit. A
//! unit test's modules are its module path from the crate root; for an integration test the
//! first segment is the test target (the file stem under `tests/`, or the folder holding
//! `main.rs`), then the module path inside it. The id resolves when `git grep` finds `fn <name>`
//! under the package's directory in a file whose module path, plus the inline modules around the
//! function, matches. A scenario id `<area>/<id>` resolves when
//! `scenarios/<area>/<id>/scenario.toml` exists at the head.

use crate::pr_meta::scan;
use crate::repo::{Repo, RepoError};

/// A workspace member: its package name and its directory from the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) dir: String,
}

/// A parsed test id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestId {
    pub(crate) package: String,
    /// The segments between the package and the function name.
    pub(crate) path: Vec<String>,
    pub(crate) name: String,
}

/// Where an id led.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// The id exists; for a test, this is the file that defines it.
    Found(String),
    /// The id does not exist, and why.
    Missing(String),
}

/// Parses `<package>::<module>::...::<fn>`.
pub(crate) fn parse_test_id(id: &str) -> Result<TestId, String> {
    let segments: Vec<&str> = id.split("::").collect();
    let (Some((&package, rest)), Some(&name)) = (segments.split_first(), segments.last()) else {
        return Err(format!("`{id}` is not a test path"));
    };
    if rest.is_empty() {
        return Err(format!(
            "`{id}` names no test function; write `<package>::<module>::...::<fn>`"
        ));
    }
    let package_ok = !package.is_empty()
        && package.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !package_ok {
        return Err(format!("`{package}` in `{id}` is not a package name"));
    }
    if let Some(bad) = rest.iter().find(|segment| !is_identifier(segment)) {
        return Err(format!("`{bad}` in `{id}` is not a Rust identifier"));
    }
    let path = rest.iter().take(rest.len() - 1).map(|segment| (*segment).to_owned()).collect();
    Ok(TestId { package: package.to_owned(), path, name: name.to_owned() })
}

fn is_identifier(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The workspace members at the head commit, from the root manifest's `members` (with `*` and
/// `?` globs) and `exclude`, plus the root package if the root manifest has one.
pub(crate) fn members(repo: &dyn Repo) -> Result<Vec<Member>, RepoError> {
    let root = read_manifest(repo, "Cargo.toml")?
        .ok_or_else(|| RepoError("the head commit has no root Cargo.toml".to_owned()))?;
    let workspace = root.get("workspace").and_then(toml::Value::as_table);
    let strings = |key: &str| -> Vec<String> {
        workspace
            .and_then(|table| table.get(key))
            .and_then(toml::Value::as_array)
            .map(|items| items.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default()
    };
    let exclude = strings("exclude");
    let mut found = Vec::new();
    if let Some(name) = package_name(&root) {
        found.push(Member { name, dir: String::new() });
    }
    for pattern in strings("members") {
        for dir in expand(repo, pattern.trim_end_matches('/'))? {
            let excluded =
                exclude.iter().any(|ex| dir == *ex || dir.starts_with(&format!("{ex}/")));
            if excluded {
                continue;
            }
            if let Some(manifest) = read_manifest(repo, &format!("{dir}/Cargo.toml"))?
                && let Some(name) = package_name(&manifest)
            {
                found.push(Member { name, dir });
            }
        }
    }
    Ok(found)
}

fn read_manifest(repo: &dyn Repo, path: &str) -> Result<Option<toml::Table>, RepoError> {
    let Some(text) = repo.read_head(path)? else {
        return Ok(None);
    };
    text.parse::<toml::Table>()
        .map(Some)
        .map_err(|error| RepoError(format!("{path} at the head commit does not parse: {error}")))
}

fn package_name(manifest: &toml::Table) -> Option<String> {
    let package = manifest.get("package")?.as_table()?;
    package.get("name")?.as_str().map(str::to_owned)
}

/// Expands a member pattern segment by segment against the directories at the head commit.
fn expand(repo: &dyn Repo, pattern: &str) -> Result<Vec<String>, RepoError> {
    let mut dirs = vec![String::new()];
    for segment in pattern.split('/') {
        let mut next = Vec::new();
        for dir in &dirs {
            if segment.contains(['*', '?']) {
                for child in repo.list_dirs_head(dir)? {
                    if wildcard(segment, &child) {
                        next.push(join(dir, &child));
                    }
                }
            } else {
                next.push(join(dir, segment));
            }
        }
        dirs = next;
    }
    Ok(dirs)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_owned() } else { format!("{dir}/{name}") }
}

/// Glob match of one path segment: `*` is any run of characters, `?` is one.
fn wildcard(pattern: &str, name: &str) -> bool {
    fn matches(pattern: &[char], name: &[char]) -> bool {
        match (pattern.split_first(), name.split_first()) {
            (None, None) => true,
            (Some(('*', rest)), _) => {
                matches(rest, name)
                    || name.split_first().is_some_and(|(_, tail)| matches(pattern, tail))
            }
            (Some(('?', rest)), Some((_, tail))) => matches(rest, tail),
            (Some((expected, rest)), Some((found, tail))) => {
                expected == found && matches(rest, tail)
            }
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches(&pattern, &name)
}

/// The module path of a source file relative to its package directory: for `src/` files the
/// path from the crate root, for `tests/` files the test target followed by the path inside it.
/// `None` for files no test id can name (build scripts, benches, examples, binaries in
/// `src/bin/`).
pub(crate) fn module_path_of(relative: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = relative.split('/').collect();
    match parts.as_slice() {
        ["src", "bin", ..] => None,
        ["src", "lib.rs" | "main.rs"] => Some(Vec::new()),
        ["src", rest @ ..] => module_segments(rest),
        ["tests", file] => file.strip_suffix(".rs").map(|stem| vec![stem.to_owned()]),
        ["tests", target, "main.rs"] => Some(vec![(*target).to_owned()]),
        ["tests", target, rest @ ..] => {
            let mut path = vec![(*target).to_owned()];
            path.extend(module_segments(rest)?);
            Some(path)
        }
        _ => None,
    }
}

/// `a/b.rs` is `a::b` and `a/b/mod.rs` is `a::b`.
fn module_segments(parts: &[&str]) -> Option<Vec<String>> {
    let (file, dirs) = parts.split_last()?;
    let mut path: Vec<String> = dirs.iter().map(|dir| (*dir).to_owned()).collect();
    if *file != "mod.rs" {
        path.push(file.strip_suffix(".rs")?.to_owned());
    }
    Some(path)
}

/// Resolves a test id against the head commit.
pub(crate) fn resolve_test(
    repo: &dyn Repo,
    members: &[Member],
    id: &TestId,
) -> Result<Resolution, RepoError> {
    let normal = |name: &str| name.replace('-', "_");
    let Some(member) = members.iter().find(|member| normal(&member.name) == normal(&id.package))
    else {
        let names: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();
        return Ok(Resolution::Missing(format!(
            "no workspace member is named `{}` (members: {})",
            id.package,
            names.join(", ")
        )));
    };
    let mut elsewhere = Vec::new();
    for file in repo.grep_head(&format!("fn {}", id.name), &member.dir)? {
        let relative = if member.dir.is_empty() {
            file.as_str()
        } else {
            file.strip_prefix(&format!("{}/", member.dir)).unwrap_or(&file)
        };
        let Some(file_path) = module_path_of(relative) else {
            continue;
        };
        let Some(source) = repo.read_head(&file)? else {
            continue;
        };
        for site in scan::fn_sites(&source).into_iter().filter(|site| site.name == id.name) {
            let full: Vec<String> = file_path.iter().cloned().chain(site.modules).collect();
            if full == id.path {
                return Ok(Resolution::Found(file));
            }
            let mut shown = vec![member.name.clone()];
            shown.extend(full);
            shown.push(id.name.clone());
            elsewhere.push(format!("`{}` in {file}", shown.join("::")));
        }
    }
    let shown_dir = if member.dir.is_empty() { "the repository root" } else { member.dir.as_str() };
    Ok(Resolution::Missing(if elsewhere.is_empty() {
        format!("there is no `fn {}` under {shown_dir} at the head commit", id.name)
    } else {
        format!("`fn {}` exists only as {}", id.name, elsewhere.join(", "))
    }))
}

/// Resolves a scenario id `<area>/<id>` against the head commit.
pub(crate) fn resolve_scenario(repo: &dyn Repo, id: &str) -> Result<Resolution, RepoError> {
    let well_formed = id.split_once('/').is_some_and(|(area, name)| {
        [area, name].iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    });
    if !well_formed {
        return Ok(Resolution::Missing(format!("`{id}` is not `<area>/<id>`")));
    }
    let path = format!("scenarios/{id}/scenario.toml");
    Ok(match repo.read_head(&path)? {
        Some(_) => Resolution::Found(path),
        None => Resolution::Missing(format!("there is no {path} at the head commit")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::mem::MemRepo;

    fn workspace() -> MemRepo {
        MemRepo::default()
            .file(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/*\", \"xtask\"]\nexclude = [\"crates/old\"]\n",
            )
            .file("xtask/Cargo.toml", "[package]\nname = \"xtask\"\n")
            .file("xtask/src/lib.rs", "mod pr_meta;\n")
            .file(
                "xtask/src/pr_meta.rs",
                "fn check() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn title_passes() {}\n}\n",
            )
            .file("crates/eileen-core/Cargo.toml", "[package]\nname = \"eileen-core\"\n")
            .file("crates/eileen-core/src/lib.rs", "pub mod turn;\n")
            .file("crates/eileen-core/src/turn.rs", "#[cfg(test)]\nmod tests {\n    #[test]\n    fn barge_in_pauses() {}\n}\n")
            .file("crates/eileen-core/tests/it/main.rs", "mod job_model;\n")
            .file("crates/eileen-core/tests/it/job_model.rs", "#[test]\nfn focus_job_follows_the_last_mention() {}\n")
            .file("crates/old/Cargo.toml", "[package]\nname = \"old\"\n")
            .file("scenarios/core/hello/scenario.toml", "id = \"hello\"\n")
    }

    fn resolve(repo: &MemRepo, id: &str) -> Resolution {
        let members = members(repo).expect("the fixture manifest parses");
        let id = parse_test_id(id).expect("the fixture id parses");
        resolve_test(repo, &members, &id).expect("the in-memory repository cannot fail")
    }

    #[test]
    fn members_expand_globs_and_honour_exclude() {
        let names: Vec<String> =
            members(&workspace()).expect("parses").into_iter().map(|m| m.name).collect();
        assert_eq!(names, vec!["eileen-core".to_owned(), "xtask".to_owned()], "old is excluded");
    }

    #[test]
    fn xtask_unit_test_resolves() {
        assert_eq!(
            resolve(&workspace(), "xtask::pr_meta::tests::title_passes"),
            Resolution::Found("xtask/src/pr_meta.rs".to_owned()),
            "W0's own tests live in xtask and resolve like any member's"
        );
    }

    #[test]
    fn integration_test_resolves_by_its_test_target() {
        assert_eq!(
            resolve(&workspace(), "eileen-core::it::job_model::focus_job_follows_the_last_mention"),
            Resolution::Found("crates/eileen-core/tests/it/job_model.rs".to_owned()),
            "the first segment after the package is the test target, `it`"
        );
        assert!(
            matches!(
                resolve(&workspace(), "eileen_core::turn::tests::barge_in_pauses"),
                Resolution::Found(_)
            ),
            "underscores in the package name resolve too"
        );
    }

    #[test]
    fn wrong_module_path_or_missing_fn_is_missing() {
        let Resolution::Missing(why) = resolve(&workspace(), "xtask::tests::title_passes") else {
            panic!("a wrong module path must not resolve");
        };
        assert!(why.contains("xtask::pr_meta::tests::title_passes"), "names where it is: {why}");
        let missing = resolve(&workspace(), "xtask::pr_meta::tests::no_such_test");
        assert!(matches!(missing, Resolution::Missing(_)), "a missing fn does not resolve");
        let unknown = resolve(&workspace(), "eileen-hud::tests::x");
        assert!(
            matches!(unknown, Resolution::Missing(ref why) if why.contains("no workspace member")),
            "an unknown package is named: {unknown:?}"
        );
    }

    #[test]
    fn scenario_ids_resolve_by_folder() {
        let repo = workspace();
        assert!(matches!(resolve_scenario(&repo, "core/hello"), Ok(Resolution::Found(_))));
        assert!(matches!(resolve_scenario(&repo, "core/missing"), Ok(Resolution::Missing(_))));
        assert!(matches!(resolve_scenario(&repo, "../etc"), Ok(Resolution::Missing(_))));
    }

    #[test]
    fn test_ids_need_a_package_and_a_function() {
        assert!(parse_test_id("xtask").is_err(), "a package alone is not a test");
        assert!(parse_test_id("xtask::").is_err(), "an empty function name");
        assert!(parse_test_id("xtask::mod-name::f").is_err(), "a module is an identifier");
        assert_eq!(
            parse_test_id("repo-tools::checks::tests::em_dash_fails"),
            Ok(TestId {
                package: "repo-tools".to_owned(),
                path: vec!["checks".to_owned(), "tests".to_owned()],
                name: "em_dash_fails".to_owned(),
            }),
            "a hyphenated package name parses"
        );
    }

    #[test]
    fn module_paths_follow_cargo_layout() {
        let owned = |segments: &[&str]| Some(segments.iter().map(|s| (*s).to_owned()).collect());
        assert_eq!(module_path_of("src/lib.rs"), owned(&[]), "the crate root");
        assert_eq!(module_path_of("src/a/b.rs"), owned(&["a", "b"]), "a nested file module");
        assert_eq!(module_path_of("src/a/mod.rs"), owned(&["a"]), "a mod.rs module");
        assert_eq!(module_path_of("tests/smoke.rs"), owned(&["smoke"]), "a one-file test target");
        assert_eq!(module_path_of("tests/it/main.rs"), owned(&["it"]), "a folder test target");
        assert_eq!(module_path_of("tests/it/x/y.rs"), owned(&["it", "x", "y"]), "inside a target");
        assert_eq!(module_path_of("build.rs"), None, "a build script holds no tests");
        assert_eq!(module_path_of("src/bin/tool.rs"), None, "binaries are not named by test ids");
    }

    #[test]
    fn wildcard_matches_one_segment() {
        assert!(wildcard("*", "eileen-core"), "star matches everything");
        assert!(wildcard("eileen-*-win", "eileen-audio-win"), "star in the middle");
        assert!(!wildcard("eileen-*-win", "eileen-audio"), "the suffix must match");
        assert!(wildcard("x?ask", "xtask"), "question mark is one character");
    }
}

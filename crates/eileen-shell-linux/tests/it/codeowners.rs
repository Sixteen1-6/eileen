//! The Linux side's owners, as `.github/CODEOWNERS` records them: every Linux crate in the
//! workspace and `docs/LINUX.md` have a line naming both owners, after the catch-all line, so the
//! Linux lines win (GitHub uses the last matching line). The check lives in the first Linux crate
//! until the repository checks of WP0 cover CODEOWNERS.

use std::path::{Path, PathBuf};

/// The owners of the Linux side, in the order the file lists them.
const LINUX_OWNERS: [&str; 2] = ["@Sixteen1-6", "@ayhamyousef"];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Each rule line of CODEOWNERS as (pattern, owners), comments and blank lines left out.
fn rules() -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(repo().join(".github").join("CODEOWNERS")).unwrap();
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut words = line.split_whitespace();
            let pattern = words.next().unwrap().to_owned();
            (pattern, words.map(str::to_owned).collect())
        })
        .collect()
}

#[test]
fn every_linux_crate_and_the_plan_name_both_owners() {
    let rules = rules();
    let mut paths: Vec<String> = std::fs::read_dir(repo().join("crates"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with("-linux"))
        .map(|name| format!("/crates/{name}/"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 2, "the workspace has its Linux crates: {paths:?}");
    paths.push("/docs/LINUX.md".to_owned());
    for path in &paths {
        let Some((_, owners)) = rules.iter().rev().find(|(pattern, _)| pattern == path) else {
            panic!("CODEOWNERS has no line for {path}");
        };
        assert_eq!(*owners, LINUX_OWNERS, "the owners of {path}");
    }
}

#[test]
fn the_catch_all_comes_first_so_the_linux_lines_win() {
    let rules = rules();
    let (pattern, owners) = rules.first().unwrap();
    assert_eq!(pattern, "*", "the catch-all is the first rule");
    assert_eq!(*owners, ["@Sixteen1-6"], "every other file keeps one owner");
}

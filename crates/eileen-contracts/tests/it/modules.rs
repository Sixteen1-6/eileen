//! `modules.toml` lists exactly the crate's contract modules, and each provisional module says
//! so in its own doc (BUILD.md section 3.3).

use std::collections::BTreeMap;
use std::path::PathBuf;

use eileen_contracts::schema::module_names;
use serde::Deserialize;

/// The pinned modules: slice S1's path (BUILD.md section 6.2).
const PINNED: &[&str] = &["ids", "jobs", "core_io", "bus", "speech", "harness", "ports", "record"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModulesToml {
    modules: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file: String,
    provisional: bool,
    pinned_by: Option<String>,
}

fn modules_toml() -> ModulesToml {
    toml::from_str(include_str!("../../modules.toml")).unwrap()
}

#[test]
fn modules_toml_matches_the_crate() {
    let listed = modules_toml().modules;
    let mut names: Vec<_> = module_names().into_iter().map(str::to_owned).collect();
    names.sort();
    assert_eq!(listed.keys().cloned().collect::<Vec<_>>(), names, "the same modules both ways");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (name, entry) in &listed {
        let source = std::fs::read_to_string(root.join(&entry.file)).unwrap();
        let doc = source
            .lines()
            .take_while(|line| line.starts_with("//!"))
            .collect::<Vec<_>>()
            .join("\n");
        let marker = doc.contains("Provisional: ");
        assert_eq!(marker, entry.provisional, "{name}: the module doc's Provisional line");
        match (&entry.pinned_by, entry.provisional) {
            (Some(owner), true) => {
                let line =
                    format!("Provisional: {owner} pins this module in its first pull request");
                assert!(doc.contains(&line), "{name}: the doc names {owner}: {line:?}");
            }
            (None, false) => {}
            (pinned_by, provisional) => {
                panic!("{name}: provisional = {provisional} with pinned_by = {pinned_by:?}")
            }
        }
    }
}

#[test]
fn exactly_the_slice_modules_are_pinned() {
    let pinned: Vec<_> = modules_toml()
        .modules
        .into_iter()
        .filter(|(_, entry)| !entry.provisional)
        .map(|(name, _)| name)
        .collect();
    let mut expected: Vec<_> = PINNED.iter().map(|name| (*name).to_owned()).collect();
    expected.sort();
    assert_eq!(pinned, expected, "the modules on slice S1's path");
}

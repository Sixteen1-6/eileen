//! The scripts under `tests/harness-scripts/`: every one parses as `StubScriptLine`, passes
//! through the `Redactor` unchanged, holds no fixture annotation, and is named with its source
//! in the folder's README.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use eileen_contracts::record::StubScriptLine;
use eileen_stub_harness::load;
use eileen_trace::Redactor;
use serde_json::Value;

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/harness-scripts")
}

/// Every `.ndjson` file in the folder, by name, in name order.
fn scripts() -> Vec<(String, String)> {
    let mut scripts: Vec<(String, String)> = fs::read_dir(folder())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ndjson"))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&path).unwrap())
        })
        .collect();
    scripts.sort();
    scripts
}

#[test]
fn every_script_parses_as_stub_script_lines() {
    let scripts = scripts();
    assert!(scripts.len() >= 5, "the first five scripts are there: {}", scripts.len());
    for (name, text) in &scripts {
        let script = load(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(!script.lines().is_empty(), "{name} plays something");
    }
}

#[test]
fn every_script_is_left_unchanged_by_the_redactor() {
    let redactor = Redactor::new();
    for (name, text) in scripts() {
        for (index, line) in text.lines().enumerate() {
            assert!(
                matches!(redactor.redact(line), Cow::Borrowed(_)),
                "{name} line {} holds something the Redactor replaces",
                index + 1
            );
        }
    }
}

#[test]
fn the_readme_names_every_script() {
    let readme = fs::read_to_string(folder().join("README.md")).unwrap();
    for (name, _) in scripts() {
        assert!(readme.contains(&format!("`{name}`")), "README.md names {name} and its source");
    }
}

/// The first key, at any depth, that starts with `$`.
fn annotation(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, inner)| {
            key.starts_with('$').then_some(key.as_str()).or_else(|| annotation(inner))
        }),
        Value::Array(items) => items.iter().find_map(annotation),
        _ => None,
    }
}

/// A research fixture marks what it left out with keys such as `$omitted`; the CLI never
/// writes one, and the stub writes every `event` as it is, so none may reach a script.
#[test]
fn no_event_holds_a_fixture_annotation() {
    for (name, text) in scripts() {
        let script = load(&text).unwrap();
        for (line, step) in script.lines() {
            if let StubScriptLine::Event { json } = step {
                assert_eq!(
                    annotation(json),
                    None,
                    "{name} line {line} holds a fixture's annotation"
                );
            }
        }
    }
}

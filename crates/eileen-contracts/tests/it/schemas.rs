//! One snapshot per contract module: its own definitions (references to other modules' types
//! stay `$ref`s, so a field added in one module changes only that module's file) and, for a
//! toolset, the full input schema of each tool whose argument struct it holds, as the
//! dispatcher serves it. Keys are sorted, so a feature-unified `preserve_order` cannot change a
//! snapshot.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eileen_contracts::jobs::Toolset;
use eileen_contracts::schema::{catalogue, module_names};
use eileen_contracts::tools::{input_schemas, toolset_members};
use serde_json::{Value, json};

/// The snapshot file name of a module.
fn snapshot_name(module: &str) -> String {
    format!("contracts-{}", module.replace('/', "-"))
}

/// The document pinned for one module.
fn module_document(module: &str) -> Value {
    let catalogue = catalogue();
    let definitions = catalogue.modules.get(module).cloned().unwrap_or_default();
    let mut document = json!({ "definitions": definitions });
    let tools: BTreeMap<_, _> = input_schemas()
        .into_iter()
        .filter(|(_, input)| input.module == module)
        .map(|(name, input)| (name, input.schema))
        .collect();
    if !tools.is_empty() {
        document["tool_inputs"] = json!(tools);
    }
    if module == "tools" {
        let toolsets: BTreeMap<_, _> = [
            Toolset::Brain,
            Toolset::Act,
            Toolset::Worker,
            Toolset::Desktop,
            Toolset::Apps,
            Toolset::Learn,
            Toolset::Session,
            Toolset::Dev,
        ]
        .into_iter()
        .map(|toolset| (serde_json::to_value(toolset).unwrap(), toolset_members(toolset)))
        .map(|(key, members)| (key.as_str().unwrap().to_owned(), members))
        .collect();
        document["toolsets"] = json!(toolsets);
    }
    document
}

fn assert_module_snapshot(module: &str) {
    let mut settings = insta::Settings::clone_current();
    settings.set_sort_maps(true);
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("snapshots");
    settings.set_omit_expression(true);
    settings.bind(|| insta::assert_json_snapshot!(snapshot_name(module), module_document(module)));
}

/// The modules the snapshot tests cover, in order.
const SNAPSHOT_MODULES: &[&str] = &[
    "ids",
    "jobs",
    "core_io",
    "bus",
    "desktop",
    "hud",
    "speech",
    "harness",
    "tools",
    "tools/brain",
    "tools/worker",
    "tools/desktop",
    "tools/apps",
    "tools/learn",
    "tools/session",
    "tools/dev",
    "intent",
    "config",
    "connectors",
    "learn",
    "scenario",
    "record",
    "fleet",
    "ports",
];

#[test]
fn ids_matches_its_snapshot() {
    assert_module_snapshot("ids");
}

#[test]
fn jobs_matches_its_snapshot() {
    assert_module_snapshot("jobs");
}

#[test]
fn core_io_matches_its_snapshot() {
    assert_module_snapshot("core_io");
}

#[test]
fn bus_matches_its_snapshot() {
    assert_module_snapshot("bus");
}

#[test]
fn desktop_matches_its_snapshot() {
    assert_module_snapshot("desktop");
}

#[test]
fn hud_matches_its_snapshot() {
    assert_module_snapshot("hud");
}

#[test]
fn speech_matches_its_snapshot() {
    assert_module_snapshot("speech");
}

#[test]
fn harness_matches_its_snapshot() {
    assert_module_snapshot("harness");
}

#[test]
fn tools_matches_its_snapshot() {
    assert_module_snapshot("tools");
}

#[test]
fn tools_brain_matches_its_snapshot() {
    assert_module_snapshot("tools/brain");
}

#[test]
fn tools_worker_matches_its_snapshot() {
    assert_module_snapshot("tools/worker");
}

#[test]
fn tools_desktop_matches_its_snapshot() {
    assert_module_snapshot("tools/desktop");
}

#[test]
fn tools_apps_matches_its_snapshot() {
    assert_module_snapshot("tools/apps");
}

#[test]
fn tools_learn_matches_its_snapshot() {
    assert_module_snapshot("tools/learn");
}

#[test]
fn tools_session_matches_its_snapshot() {
    assert_module_snapshot("tools/session");
}

#[test]
fn tools_dev_matches_its_snapshot() {
    assert_module_snapshot("tools/dev");
}

#[test]
fn intent_matches_its_snapshot() {
    assert_module_snapshot("intent");
}

#[test]
fn config_matches_its_snapshot() {
    assert_module_snapshot("config");
}

#[test]
fn connectors_matches_its_snapshot() {
    assert_module_snapshot("connectors");
}

#[test]
fn learn_matches_its_snapshot() {
    assert_module_snapshot("learn");
}

#[test]
fn scenario_matches_its_snapshot() {
    assert_module_snapshot("scenario");
}

#[test]
fn record_matches_its_snapshot() {
    assert_module_snapshot("record");
}

#[test]
fn fleet_matches_its_snapshot() {
    assert_module_snapshot("fleet");
}

#[test]
fn ports_matches_its_snapshot() {
    assert_module_snapshot("ports");
}

#[test]
fn every_module_has_a_snapshot_test() {
    assert_eq!(SNAPSHOT_MODULES, module_names().as_slice(), "one snapshot test per module");
}

#[test]
fn every_definition_has_one_owner() {
    let catalogue = catalogue();
    assert!(
        catalogue.unowned.is_empty(),
        "definitions no module registers: {:?}",
        catalogue.unowned
    );
    assert!(catalogue.duplicates.is_empty(), "schema names used twice: {:?}", catalogue.duplicates);
    assert!(
        catalogue.inlined.is_empty(),
        "registered types with no definition: {:?}",
        catalogue.inlined
    );
    for (module, definitions) in &catalogue.modules {
        assert!(!definitions.is_empty(), "module {module} registers no type");
    }
}

#[test]
fn every_tool_of_every_toolset_has_an_input_schema() {
    let inputs = input_schemas();
    for toolset in [
        Toolset::Brain,
        Toolset::Act,
        Toolset::Worker,
        Toolset::Desktop,
        Toolset::Apps,
        Toolset::Learn,
        Toolset::Session,
        Toolset::Dev,
    ] {
        for tool in toolset_members(toolset) {
            let input = inputs.get(tool);
            assert!(input.is_some(), "{toolset:?} lists {tool}, which has no argument struct");
            let schema = &input.unwrap().schema;
            assert_eq!(schema["type"], "object", "{tool}'s input is a JSON object");
        }
    }
    let listed: Vec<_> = [
        Toolset::Brain,
        Toolset::Worker,
        Toolset::Desktop,
        Toolset::Apps,
        Toolset::Learn,
        Toolset::Session,
        Toolset::Dev,
    ]
    .into_iter()
    .flat_map(|toolset| toolset_members(toolset).iter().copied())
    .collect();
    for tool in inputs.keys() {
        assert!(listed.contains(tool), "{tool} has a schema but no toolset lists it");
    }
}

#[test]
fn path_and_match_fields_carry_their_extensions() {
    let inputs = input_schemas();
    let open_path = &inputs["apps_open_path"].schema;
    assert_eq!(open_path["properties"]["path"]["x-eileen-path"], true, "apps_open_path.path");
    let open_url = &inputs["apps_open_url"].schema;
    assert_eq!(
        open_url["properties"]["wait_for_media_title"]["x-eileen-match"], true,
        "apps_open_url.wait_for_media_title"
    );
}

#[test]
fn no_snapshot_file_is_unreferenced() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/it/snapshots");
    let expected: Vec<String> =
        SNAPSHOT_MODULES.iter().map(|module| format!("{}.snap", snapshot_name(module))).collect();
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(expected.contains(&name), "{name} in {} belongs to no test", folder.display());
        found.push(name);
    }
    for name in &expected {
        assert!(found.contains(name), "{name} is missing: a module has no snapshot test");
    }
}

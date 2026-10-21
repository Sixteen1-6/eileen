//! The text rule (BUILD.md section 3.3.1, check 26): every free-text string reachable from
//! `Input`, `ToolResult` and `DesktopState` is `Untrusted` or `Personal` (their schemas carry
//! `x-eileen-text`), or is listed in `text-allowlist.toml` with a reason. A string with an
//! `enum`, `const`, `pattern` or `format` is not free text. A schema that accepts any value
//! (`true`, or one with no `type`, `$ref`, `const`, `enum` or combinator, as schemars writes a
//! `serde_json::Value`) can hold free text, and so can the keys of a map.

use std::collections::{BTreeMap, BTreeSet};

use eileen_contracts::core_io::{Input, ToolResult};
use eileen_contracts::desktop::DesktopState;
use eileen_contracts::schema::settings;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
struct Allowlist {
    allow: BTreeMap<String, String>,
}

/// The reasons for every allowed plain string, by path.
fn allowlist() -> BTreeMap<String, String> {
    let text = include_str!("../../text-allowlist.toml");
    toml::from_str::<Allowlist>(text).unwrap().allow
}

/// True for a schema that accepts a free-text string: a string with no `enum`, `const`,
/// `pattern` or `format`, or a schema that accepts any value.
fn is_free_text(schema: &Map<String, Value>) -> bool {
    let accepts_string = match schema.get("type") {
        Some(Value::String(kind)) => kind == "string",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "string"),
        Some(_) => false,
        None => return accepts_any_value(schema),
    };
    accepts_string
        && ["enum", "const", "pattern", "format"].iter().all(|k| !schema.contains_key(*k))
}

/// True for an object schema that constrains nothing: no type, reference, constant, enum or
/// combinator.
fn accepts_any_value(schema: &Map<String, Value>) -> bool {
    ["type", "$ref", "const", "enum", "oneOf", "anyOf", "allOf", "not"]
        .iter()
        .all(|k| !schema.contains_key(*k))
}

/// True for an object schema whose keys are free text: a map (`additionalProperties` that
/// allows something) with no `propertyNames` rule.
fn has_free_keys(schema: &Map<String, Value>) -> bool {
    let open =
        schema.get("additionalProperties").is_some_and(|values| values != &Value::Bool(false));
    let named = schema
        .get("propertyNames")
        .and_then(Value::as_object)
        .is_some_and(|names| ["pattern", "enum", "const"].iter().any(|k| names.contains_key(*k)));
    open && !named
}

/// The definition a `$ref` names.
fn ref_name(reference: &str) -> Option<&str> {
    reference.strip_prefix("#/$defs/")
}

/// Collects the free-text paths under `schema` and the definitions it refers to.
fn walk(schema: &Value, path: &str, found: &mut BTreeSet<String>, refs: &mut Vec<String>) {
    let map = match schema {
        Value::Bool(true) => {
            found.insert(path.to_owned());
            return;
        }
        Value::Object(map) => map,
        Value::Null
        | Value::Bool(false)
        | Value::Number(_)
        | Value::String(_)
        | Value::Array(_) => {
            return;
        }
    };
    if map.contains_key("x-eileen-text") {
        return;
    }
    if let Some(Value::String(reference)) = map.get("$ref") {
        refs.extend(ref_name(reference).map(str::to_owned));
        return;
    }
    if is_free_text(map) {
        found.insert(path.to_owned());
    }
    if has_free_keys(map) {
        found.insert(format!("{path}{{key}}"));
    }
    if let Some(Value::Object(properties)) = map.get("properties") {
        for (name, property) in properties {
            walk(property, &format!("{path}.{name}"), found, refs);
        }
    }
    if let Some(items) = map.get("items") {
        walk(items, &format!("{path}[]"), found, refs);
    }
    if let Some(Value::Array(items)) = map.get("prefixItems") {
        for (index, item) in items.iter().enumerate() {
            walk(item, &format!("{path}[{index}]"), found, refs);
        }
    }
    if let Some(values) = map.get("additionalProperties") {
        walk(values, &format!("{path}{{}}"), found, refs);
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(alternatives)) = map.get(key) {
            for alternative in alternatives {
                walk(alternative, path, found, refs);
            }
        }
    }
}

/// Every free-text path reachable from the three roots.
fn free_text_paths() -> BTreeSet<String> {
    let mut generator = settings().into_generator();
    let roots = [
        generator.subschema_for::<Input>(),
        generator.subschema_for::<ToolResult>(),
        generator.subschema_for::<DesktopState>(),
    ];
    let definitions = generator.take_definitions(true);
    let mut found = BTreeSet::new();
    let mut queue = Vec::new();
    for root in &roots {
        walk(root.as_value(), "", &mut found, &mut queue);
    }
    let mut seen = BTreeSet::new();
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let definition = definitions.get(&name).unwrap();
        walk(definition, &name, &mut found, &mut queue);
    }
    found
}

#[test]
fn free_text_is_wrapped_or_allowlisted() {
    let found = free_text_paths();
    let allowed = allowlist();
    let unwrapped: Vec<_> = found.iter().filter(|path| !allowed.contains_key(*path)).collect();
    assert!(
        unwrapped.is_empty(),
        "free-text strings reachable from Input, ToolResult or DesktopState that are neither \
         Untrusted nor Personal; wrap them, or list them in text-allowlist.toml with a reason: \
         {unwrapped:#?}"
    );
    let stale: Vec<_> = allowed.keys().filter(|path| !found.contains(*path)).collect();
    assert!(stale.is_empty(), "text-allowlist.toml lists paths that no longer exist: {stale:#?}");
    let unexplained: Vec<_> = allowed
        .iter()
        .filter(|(_, reason)| reason.trim().len() < 10)
        .map(|(path, _)| path)
        .collect();
    assert!(unexplained.is_empty(), "allowlist entries need a reason: {unexplained:#?}");
}

#[test]
fn the_walk_sees_a_plain_string_and_skips_a_wrapped_one() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "plain": { "type": "string" },
            "maybe": { "type": ["string", "null"] },
            "wrapped": { "type": "string", "x-eileen-text": "untrusted" },
            "id": { "type": "string", "pattern": "^job_" },
            "kind": { "type": "string", "enum": ["a", "b"] },
            "list": { "type": "array", "items": { "type": "string" } },
            "pair": { "type": "array", "prefixItems": [{ "type": "string" }, { "type": "integer" }] },
            "other": { "$ref": "#/$defs/Other" },
            "any": true,
            "any2": { "description": "x" },
            "any_wrapped": { "description": "x", "x-eileen-text": "personal" },
            "count": { "type": "integer" },
            "choice": { "oneOf": [{ "const": "a" }, { "type": "integer" }] },
            "names": { "type": "object", "additionalProperties": { "type": "integer" } },
            "closed": { "type": "object", "additionalProperties": false }
        }
    });
    let (mut found, mut refs) = (BTreeSet::new(), Vec::new());
    walk(&schema, "T", &mut found, &mut refs);
    let expected: BTreeSet<String> =
        ["T.plain", "T.maybe", "T.list[]", "T.pair[0]", "T.any", "T.any2", "T.names{key}"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    assert_eq!(found, expected, "free text");
    assert_eq!(refs, ["Other"], "references followed");
}

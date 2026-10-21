//! A small checker for the JSON Schema 2020-12 keywords the contract schemas use, so a test can
//! hold a value against a generated schema without a validator crate (section 4.1 pins none).
//! It reports a keyword it does not know instead of skipping it; `pattern` and `format` are the
//! two it knowingly leaves unchecked.

use serde_json::{Map, Value};

/// Keywords that only describe, plus the two this checker does not evaluate.
const NOT_CHECKED: &[&str] = &[
    "$schema",
    "$defs",
    "title",
    "description",
    "default",
    "examples",
    "x-eileen-text",
    "x-eileen-path",
    "x-eileen-match",
    "pattern",
    "format",
];

/// Every way `value` breaks `root`, empty when it matches.
pub(super) fn errors(root: &Value, value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    check(root, root, value, "$", &mut out);
    out
}

/// True when `value` matches `schema` (within `root`).
fn matches(root: &Value, schema: &Value, value: &Value) -> bool {
    let mut out = Vec::new();
    check(root, schema, value, "$", &mut out);
    out.is_empty()
}

fn check(root: &Value, schema: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let map = match schema {
        Value::Bool(true) => return,
        Value::Object(map) => map,
        other => {
            out.push(format!("{path}: the schema {other} allows nothing"));
            return;
        }
    };
    for (keyword, rule) in map {
        match keyword.as_str() {
            "$ref" => check(root, definition(root, rule), value, path, out),
            "type" if !type_matches(rule, value) => out.push(format!("{path}: not {rule}")),
            "const" if rule != value => out.push(format!("{path}: not {rule}")),
            "enum" if !rule.as_array().is_some_and(|options| options.contains(value)) => {
                out.push(format!("{path}: not one of {rule}"));
            }
            "properties" => properties(root, rule, value, path, out),
            "required" => required(rule, value, path, out),
            "additionalProperties" => additional(root, map, rule, value, path, out),
            "items" => items(root, map, rule, value, path, out),
            "prefixItems" => prefix_items(root, rule, value, path, out),
            "minItems" | "maxItems" => item_count(keyword, rule, value, path, out),
            "minimum" | "maximum" => bound(keyword, rule, value, path, out),
            "oneOf" | "anyOf" | "allOf" => alternatives(root, keyword, rule, value, path, out),
            "type" | "const" | "enum" => {}
            other if NOT_CHECKED.contains(&other) => {}
            other => out.push(format!("{path}: the checker does not know `{other}`")),
        }
    }
}

/// The definition a `#/$defs/<name>` reference names.
fn definition<'a>(root: &'a Value, reference: &Value) -> &'a Value {
    let name = reference.as_str().and_then(|text| text.strip_prefix("#/$defs/")).unwrap();
    &root["$defs"][name]
}

fn type_matches(rule: &Value, value: &Value) -> bool {
    let one = |kind: &Value| match kind.as_str().unwrap() {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        other => panic!("unknown type {other}"),
    };
    match rule {
        Value::Array(kinds) => kinds.iter().any(one),
        kind => one(kind),
    }
}

fn properties(root: &Value, rule: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let (Some(object), Some(properties)) = (value.as_object(), rule.as_object()) else { return };
    for (name, schema) in properties {
        if let Some(field) = object.get(name) {
            check(root, schema, field, &format!("{path}.{name}"), out);
        }
    }
}

fn required(rule: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let Some(object) = value.as_object() else { return };
    for name in rule.as_array().unwrap().iter().filter_map(Value::as_str) {
        if !object.contains_key(name) {
            out.push(format!("{path}: `{name}` is missing"));
        }
    }
}

fn additional(
    root: &Value,
    map: &Map<String, Value>,
    rule: &Value,
    value: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    let Some(object) = value.as_object() else { return };
    let known = map.get("properties").and_then(Value::as_object);
    for (name, field) in object {
        if !known.is_some_and(|known| known.contains_key(name)) {
            check(root, rule, field, &format!("{path}.{name}"), out);
        }
    }
}

fn items(
    root: &Value,
    map: &Map<String, Value>,
    rule: &Value,
    value: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    let Some(array) = value.as_array() else { return };
    let skip = map.get("prefixItems").and_then(Value::as_array).map_or(0, Vec::len);
    for (index, item) in array.iter().enumerate().skip(skip) {
        check(root, rule, item, &format!("{path}[{index}]"), out);
    }
}

fn prefix_items(root: &Value, rule: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let (Some(array), Some(schemas)) = (value.as_array(), rule.as_array()) else { return };
    for (index, (item, schema)) in array.iter().zip(schemas).enumerate() {
        check(root, schema, item, &format!("{path}[{index}]"), out);
    }
}

fn item_count(keyword: &str, rule: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let (Some(array), Some(limit)) = (value.as_array(), rule.as_u64()) else { return };
    let count = u64::try_from(array.len()).unwrap();
    let fits = if keyword == "minItems" { count >= limit } else { count <= limit };
    if !fits {
        out.push(format!("{path}: {count} items breaks {keyword} {limit}"));
    }
}

fn bound(keyword: &str, rule: &Value, value: &Value, path: &str, out: &mut Vec<String>) {
    let (Some(number), Some(limit)) = (value.as_f64(), rule.as_f64()) else { return };
    let fits = if keyword == "minimum" { number >= limit } else { number <= limit };
    if !fits {
        out.push(format!("{path}: {number} breaks {keyword} {limit}"));
    }
}

fn alternatives(
    root: &Value,
    keyword: &str,
    rule: &Value,
    value: &Value,
    path: &str,
    out: &mut Vec<String>,
) {
    let schemas = rule.as_array().unwrap();
    let matched = schemas.iter().filter(|schema| matches(root, schema, value)).count();
    let fits = match keyword {
        "oneOf" => matched == 1,
        "anyOf" => matched >= 1,
        _ => matched == schemas.len(),
    };
    if !fits {
        out.push(format!("{path}: {matched} of {} {keyword} branches match", schemas.len()));
    }
}

#[test]
fn the_checker_reads_the_keywords_it_needs() {
    let root = serde_json::json!({
        "type": "object",
        "properties": {
            "n": { "type": "integer", "minimum": 0, "maximum": 9 },
            "tag": { "oneOf": [{ "const": "a" }, { "$ref": "#/$defs/B" }] },
            "list": { "type": "array", "items": { "type": "string" }, "maxItems": 2 }
        },
        "required": ["n"],
        "additionalProperties": false,
        "$defs": { "B": { "type": "object", "properties": {}, "additionalProperties": false } }
    });
    let good = serde_json::json!({ "n": 3, "tag": {}, "list": ["x"] });
    assert_eq!(errors(&root, &good), Vec::<String>::new(), "a matching value");
    for bad in [
        serde_json::json!({ "n": 10 }),
        serde_json::json!({ "n": 1, "extra": true }),
        serde_json::json!({ "tag": "a" }),
        serde_json::json!({ "n": 1, "tag": { "x": 1 } }),
        serde_json::json!({ "n": 1, "list": ["x", "y", "z"] }),
        serde_json::json!({ "n": 1.5 }),
    ] {
        assert!(!errors(&root, &bad).is_empty(), "{bad} should not match");
    }
    let unknown = serde_json::json!({ "uniqueItems": true });
    assert!(!errors(&unknown, &serde_json::json!([])).is_empty(), "an unknown keyword is reported");
}

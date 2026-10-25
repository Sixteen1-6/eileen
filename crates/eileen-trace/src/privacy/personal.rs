//! The placeholder rule for `Personal` positions: a walk of a JSON value alongside the
//! contracts' generated schema, which marks every `Personal` value with the `x-eileen-text`
//! extension. Where the schema offers alternatives (an enum's variants, an `Option`), the walk
//! follows each alternative the value could match by its type, required keys, constant tags
//! and closed property sets, which is how serde tells the variants apart.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::{Map, Value};

use super::PLACEHOLDERS;

/// How deep a walk goes before it stops (the contract types nest far less).
const MAX_DEPTH: usize = 128;

/// Every contract definition, by schema name.
fn definitions() -> &'static BTreeMap<String, Value> {
    static DEFINITIONS: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    DEFINITIONS.get_or_init(|| {
        eileen_contracts::schema::catalogue().modules.into_values().flatten().collect()
    })
}

/// True when `text` is built only from the placeholders: once they are removed, no letter or
/// digit is left (separators such as `\`, `/` and spaces may stay).
pub(super) fn built_from_placeholders(text: &str) -> bool {
    let rest = PLACEHOLDERS.iter().fold(text.to_owned(), |rest, p| rest.replace(p, ""));
    !rest.chars().any(char::is_alphanumeric)
}

/// The JSON pointers, inside `value` read as the contract type named `root` (`RecordLine`,
/// `Scenario`), of every value in a `Personal` position that is not built only from the
/// placeholders.
pub(super) fn offending(value: &Value, root: &str) -> Vec<String> {
    let definitions = definitions();
    let mut found = Vec::new();
    if let Some(schema) = definitions.get(root) {
        let mut walk = Walk { definitions, found: &mut found };
        walk.value(value, schema, "", 0);
    }
    // Two alternatives that both fit can reach one position twice.
    found.sort();
    found.dedup();
    found
}

/// `pointer` extended by one key, escaped as JSON Pointer says.
fn child(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// The definition a `$ref` names.
fn resolve<'a>(
    definitions: &'a BTreeMap<String, Value>,
    schema: &Map<String, Value>,
) -> Option<&'a Value> {
    let reference = schema.get("$ref")?.as_str()?;
    definitions.get(reference.strip_prefix("#/$defs/")?)
}

struct Walk<'a> {
    definitions: &'a BTreeMap<String, Value>,
    found: &'a mut Vec<String>,
}

impl Walk<'_> {
    /// Walks `value` under `schema`, collecting offending `Personal` values.
    fn value(&mut self, value: &Value, schema: &Value, pointer: &str, depth: usize) {
        let Some(schema) = schema.as_object() else { return };
        if depth > MAX_DEPTH {
            return;
        }
        if schema.get("x-eileen-text").and_then(Value::as_str) == Some("personal") {
            self.personal(value, pointer);
            return;
        }
        if let Some(target) = resolve(self.definitions, schema) {
            self.value(value, target, pointer, depth + 1);
        }
        for alternative in alternatives(schema, "allOf") {
            self.value(value, alternative, pointer, depth + 1);
        }
        for key in ["oneOf", "anyOf"] {
            for alternative in alternatives(schema, key) {
                if could_match(self.definitions, value, alternative, depth + 1) {
                    self.value(value, alternative, pointer, depth + 1);
                }
            }
        }
        match value {
            Value::Object(fields) => self.fields(fields, schema, pointer, depth),
            Value::Array(items) => self.items(items, schema, pointer, depth),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    fn fields(
        &mut self,
        fields: &Map<String, Value>,
        schema: &Map<String, Value>,
        pointer: &str,
        depth: usize,
    ) {
        let properties = schema.get("properties").and_then(Value::as_object);
        let additional = schema.get("additionalProperties").filter(|a| a.is_object());
        for (key, field) in fields {
            if let Some(property) = properties.and_then(|p| p.get(key)).or(additional) {
                self.value(field, property, &child(pointer, key), depth + 1);
            }
        }
    }

    fn items(&mut self, items: &[Value], schema: &Map<String, Value>, pointer: &str, depth: usize) {
        let prefix =
            schema.get("prefixItems").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let rest = schema.get("items").filter(|i| i.is_object());
        for (index, item) in items.iter().enumerate() {
            let at = child(pointer, &index.to_string());
            if let Some(schema) = prefix.get(index).or(rest) {
                self.value(item, schema, &at, depth + 1);
            }
        }
    }

    /// A value in a `Personal` position: null passes, text must be built from placeholders,
    /// any other scalar fails, and containers are checked leaf by leaf.
    fn personal(&mut self, value: &Value, pointer: &str) {
        match value {
            Value::Null => {}
            Value::String(text) => {
                if !built_from_placeholders(text) {
                    self.found.push(pointer.to_owned());
                }
            }
            Value::Bool(_) | Value::Number(_) => self.found.push(pointer.to_owned()),
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.personal(item, &child(pointer, &index.to_string()));
                }
            }
            Value::Object(fields) => {
                for (key, field) in fields {
                    self.personal(field, &child(pointer, key));
                }
            }
        }
    }
}

/// The schemas listed under `key` (`allOf`, `oneOf`, `anyOf`).
fn alternatives<'a>(schema: &'a Map<String, Value>, key: &str) -> &'a [Value] {
    schema.get(key).and_then(Value::as_array).map_or(&[][..], Vec::as_slice)
}

/// Whether `value` could be an instance of `schema`, judged by the keywords that tell serde's
/// enum variants and options apart: `type`, `const`, `enum`, `required`, the constant of a
/// tag property, and `additionalProperties: false`.
fn could_match(
    definitions: &BTreeMap<String, Value>,
    value: &Value,
    schema: &Value,
    depth: usize,
) -> bool {
    let schema = match schema {
        Value::Bool(accepts) => return *accepts,
        Value::Object(schema) => schema,
        Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_) => return false,
    };
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some(target) = resolve(definitions, schema) {
        return could_match(definitions, value, target, depth + 1);
    }
    if schema.get("const").is_some_and(|c| c != value) {
        return false;
    }
    if schema.get("enum").and_then(Value::as_array).is_some_and(|options| !options.contains(value))
    {
        return false;
    }
    if !type_matches(schema.get("type"), value) {
        return false;
    }
    if let Value::Object(fields) = value
        && !fields_match(definitions, fields, schema)
    {
        return false;
    }
    let any = |key| {
        let options = alternatives(schema, key);
        options.is_empty() || options.iter().any(|o| could_match(definitions, value, o, depth + 1))
    };
    any("oneOf")
        && any("anyOf")
        && alternatives(schema, "allOf")
            .iter()
            .all(|o| could_match(definitions, value, o, depth + 1))
}

/// The object checks of [`could_match`]: required keys present, tag constants equal, and no
/// unknown key where the schema closes its properties.
fn fields_match(
    definitions: &BTreeMap<String, Value>,
    fields: &Map<String, Value>,
    schema: &Map<String, Value>,
) -> bool {
    let required = schema.get("required").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if !required.iter().filter_map(Value::as_str).all(|key| fields.contains_key(key)) {
        return false;
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    if schema.get("additionalProperties") == Some(&Value::Bool(false))
        && !fields.keys().all(|key| properties.is_some_and(|p| p.contains_key(key)))
    {
        return false;
    }
    properties.is_none_or(|properties| {
        properties.iter().all(|(key, property)| {
            let constant = property
                .as_object()
                .and_then(|p| p.get("const").or_else(|| resolve(definitions, p)?.get("const")));
            match (fields.get(key), constant) {
                (Some(field), Some(constant)) => field == constant,
                _ => true,
            }
        })
    })
}

/// Whether `value` has one of the JSON types `kind` names (a name or a list); no `type`
/// accepts anything.
fn type_matches(kind: Option<&Value>, value: &Value) -> bool {
    let is_type = |name: &str| match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_u64() || value.is_i64(),
        _ => false,
    };
    match kind {
        None => true,
        Some(Value::String(name)) => is_type(name),
        Some(Value::Array(types)) => types.iter().filter_map(Value::as_str).any(is_type),
        Some(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{built_from_placeholders, definitions, offending};

    #[test]
    fn placeholder_text_is_recognised() {
        for text in ["{NAME}", "{HOME}\\{DOCS}\\{TITLE}", "{WORK} / {TITLE}", ""] {
            assert!(built_from_placeholders(text), "{text}");
        }
        for text in ["Jordan Vale", "{DOCS}\\tax.pdf", "{NAME}2", "{TITLE}.docx"] {
            assert!(!built_from_placeholders(text), "{text}");
        }
    }

    #[test]
    fn the_roots_the_rule_reads_are_contract_definitions() {
        for root in ["RecordLine", "Scenario", "HudInput", "UserInput"] {
            assert!(definitions().contains_key(root), "{root} is in the catalogue");
        }
    }

    #[test]
    fn only_the_variant_the_value_takes_is_walked() {
        // `typed` and `cli` both carry a personal `text`; a `toggle_mute` string carries none.
        let cli = json!({ "cli": { "text": "open my diary" } });
        assert_eq!(offending(&cli, "UserInput"), ["/cli/text"], "the cli variant");
        let placeholder = json!({ "typed": { "text": "{TITLE}" } });
        assert!(offending(&placeholder, "UserInput").is_empty(), "a placeholder passes");
    }
}

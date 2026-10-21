//! The `pii.` policy: user content travels only in fields whose name starts with `pii.`
//! (`pii.transcript`, `pii.title`, `pii.path`), and each sink decides what to do with them.
//!
//! | Sink | `pii.` fields |
//! |---|---|
//! | Flight recorder (memory) | kept; secrets scanned at dump time |
//! | JSON log files | length and keyed hash, unless `logging.include_personal` |
//! | Console | kept (a terminal the owner is watching) |

use serde_json::{Map, Value, json};

/// The prefix that marks a field as user content.
pub const PREFIX: &str = "pii.";

/// True for a field that carries user content.
#[must_use]
pub fn is_pii(field: &str) -> bool {
    field.starts_with(PREFIX)
}

/// What a sink does with `pii.` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Keep the value.
    Keep,
    /// Replace the value with `{"len": <chars>, "h": "<8 hex>"}`, a keyed hash, so two lines
    /// about the same window title can be matched without the title.
    Reduce {
        /// The key (per install, from the runtime).
        key: [u8; 32],
    },
}

/// The reduced form of one value.
#[must_use]
pub fn reduce(value: &Value, key: &[u8; 32]) -> Value {
    let text = if let Value::String(s) = value { s.clone() } else { value.to_string() };
    let hash = blake3::keyed_hash(key, text.as_bytes());
    let [a, b, c, d, ..] = *hash.as_bytes();
    json!({ "len": text.chars().count(), "h": format!("{a:02x}{b:02x}{c:02x}{d:02x}") })
}

/// Applies `policy` to every `pii.` field of `fields`, in place.
pub fn apply(fields: &mut Map<String, Value>, policy: Policy) {
    let Policy::Reduce { key } = policy else {
        return;
    };
    for (name, value) in fields.iter_mut() {
        if is_pii(name) {
            *value = reduce(value, &key);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{Policy, apply, is_pii, reduce};

    #[test]
    fn reduce_keeps_the_length_and_a_keyed_hash() {
        let key = [7; 32];
        let reduced = reduce(&json!("Quarterly report.docx"), &key);
        assert_eq!(reduced["len"], 21, "length in characters");
        assert_eq!(reduced["h"].as_str().map(str::len), Some(8), "8 hex digits");
        assert_eq!(reduce(&json!("Quarterly report.docx"), &key), reduced, "stable");
        assert_ne!(reduce(&json!("Quarterly report.docx"), &[8; 32]), reduced, "keyed");
    }

    #[test]
    fn apply_touches_only_pii_fields() {
        let mut fields: Map<String, Value> = Map::new();
        fields.insert("pii.title".to_owned(), json!("My diary"));
        fields.insert("tool".to_owned(), json!("desktop_tree"));
        apply(&mut fields, Policy::Reduce { key: [0; 32] });
        assert!(fields["pii.title"].get("h").is_some(), "reduced");
        assert_eq!(fields["tool"], "desktop_tree", "left alone");
        assert!(is_pii("pii.transcript") && !is_pii("piixel"), "prefix");
    }
}

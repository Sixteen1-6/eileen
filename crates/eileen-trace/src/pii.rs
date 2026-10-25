//! The `pii.` policy: user content travels only in fields whose name starts with `pii.`
//! (`pii.transcript`, `pii.title`, `pii.path`), and each sink decides what to do with them.
//!
//! | Sink | `pii.` fields |
//! |---|---|
//! | Flight recorder (memory) | kept; secrets scanned at dump time |
//! | JSON log files | length and keyed hash, unless `logging.include_personal` |
//! | Console | kept (a terminal the owner is watching); secrets redacted |
//!
//! The hash's key is per install: the runtime derives it at setup and passes it to every
//! process it starts. With no key the files keep the length alone, so a process started
//! without one never hashes user text under a key anyone could know.

use std::fmt;

use serde_json::{Map, Value, json};

/// The prefix that marks a field as user content.
pub const PREFIX: &str = "pii.";

/// The per-install key of the files' keyed hash. Its `Debug` prints `PiiKey(..)`, so a
/// configuration, a policy or a panic message formatted with `{:?}` never shows it: the key's
/// secrecy is what stops anyone matching a reduced `{len, h}` against a dictionary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PiiKey([u8; 32]);

impl PiiKey {
    /// A key from its 32 bytes (the runtime derives them at setup).
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for PiiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PiiKey(..)")
    }
}

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
    /// about the same window title can be matched without the title; with no key, with
    /// `{"len": <chars>}` alone.
    Reduce {
        /// The key (per install, from the runtime); `None` keeps the length alone.
        key: Option<PiiKey>,
    },
}

/// The reduced form of one value: its length, and its keyed hash when there is a key.
#[must_use]
pub fn reduce(value: &Value, key: Option<&PiiKey>) -> Value {
    let text = if let Value::String(s) = value { s.clone() } else { value.to_string() };
    let len = text.chars().count();
    match key {
        Some(key) => {
            let hash = blake3::keyed_hash(key.bytes(), text.as_bytes());
            let [a, b, c, d, ..] = *hash.as_bytes();
            json!({ "len": len, "h": format!("{a:02x}{b:02x}{c:02x}{d:02x}") })
        }
        None => json!({ "len": len }),
    }
}

/// Applies `policy` to every `pii.` field of `fields`, in place.
pub fn apply(fields: &mut Map<String, Value>, policy: Policy) {
    let Policy::Reduce { key } = policy else {
        return;
    };
    for (name, value) in fields.iter_mut() {
        if is_pii(name) {
            *value = reduce(value, key.as_ref());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{PiiKey, Policy, apply, is_pii, reduce};

    #[test]
    fn reduce_keeps_the_length_and_a_keyed_hash() {
        let key = PiiKey::new([7; 32]);
        let reduced = reduce(&json!("Quarterly report.docx"), Some(&key));
        assert_eq!(reduced["len"], 21, "length in characters");
        assert_eq!(reduced["h"].as_str().map(str::len), Some(8), "8 hex digits");
        assert_eq!(reduce(&json!("Quarterly report.docx"), Some(&key)), reduced, "stable");
        let other = PiiKey::new([8; 32]);
        assert_ne!(reduce(&json!("Quarterly report.docx"), Some(&other)), reduced, "keyed");
    }

    #[test]
    fn the_key_never_shows_in_debug() {
        let policy = Policy::Reduce { key: Some(PiiKey::new([0xAB; 32])) };
        let shown = format!("{policy:?}");
        assert!(shown.contains("PiiKey(..)"), "{shown}");
        assert!(!shown.contains("171") && !shown.contains("ab"), "no byte of the key: {shown}");
    }

    #[test]
    fn without_a_key_only_the_length_is_kept() {
        assert_eq!(reduce(&json!("My diary"), None), json!({ "len": 8 }), "no hash");
    }

    #[test]
    fn apply_touches_only_pii_fields() {
        let mut fields: Map<String, Value> = Map::new();
        fields.insert("pii.title".to_owned(), json!("My diary"));
        fields.insert("tool".to_owned(), json!("desktop_tree"));
        apply(&mut fields, Policy::Reduce { key: Some(PiiKey::new([3; 32])) });
        assert!(fields["pii.title"].get("h").is_some(), "reduced");
        assert_eq!(fields["tool"], "desktop_tree", "left alone");
        assert!(is_pii("pii.transcript") && !is_pii("piixel"), "prefix");
    }
}

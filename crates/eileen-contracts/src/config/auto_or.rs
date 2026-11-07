//! [`AutoOr`]: a setting that is `"auto"` or a value, in TOML and JSON alike.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A setting that is `"auto"` or a value. The fitted settings of section 2.16 are `auto` by
/// default, which the machine profile's plan resolves; any value is the user's override, which
/// the plan never changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoOr<T> {
    /// Eileen computes it.
    #[default]
    Auto,
    /// This value.
    Value(T),
}

impl<T: Serialize> Serialize for AutoOr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for AutoOr<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Only the word `auto`.
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum AutoWord {
            Auto,
        }
        // `auto` first, then the value, so a value written as a word (a precision such as
        // `int8`) reads as itself; any other word is refused by name.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr<T> {
            Auto(AutoWord),
            Value(T),
            Word(String),
        }
        match Repr::<T>::deserialize(deserializer)? {
            Repr::Auto(AutoWord::Auto) => Ok(Self::Auto),
            Repr::Value(value) => Ok(Self::Value(value)),
            Repr::Word(word) => {
                Err(serde::de::Error::custom(format!("expected \"auto\" or a value, not {word:?}")))
            }
        }
    }
}

impl<T: JsonSchema> JsonSchema for AutoOr<T> {
    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("AutoOr_{}", T::schema_name()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let value = generator.subschema_for::<T>();
        json_schema!({ "anyOf": [{ "const": "auto" }, value] })
    }
}

#[cfg(test)]
mod tests {
    use super::AutoOr;
    use crate::machine::Precision;

    #[test]
    fn auto_a_value_and_a_value_written_as_a_word_all_read_back() {
        let auto: AutoOr<Precision> = serde_json::from_str("\"auto\"").unwrap();
        assert_eq!(auto, AutoOr::Auto, "the word auto");
        let int8: AutoOr<Precision> = serde_json::from_str("\"int8\"").unwrap();
        assert_eq!(int8, AutoOr::Value(Precision::Int8), "a precision is written as a word");
        let count: AutoOr<u8> = serde_json::from_str("4").unwrap();
        assert_eq!(count, AutoOr::Value(4), "a number");
        let off: AutoOr<bool> = serde_json::from_str("false").unwrap();
        assert_eq!(off, AutoOr::Value(false), "a flag");
        for value in [AutoOr::Auto, AutoOr::Value(Precision::Fp32)] {
            let text = serde_json::to_string(&value).unwrap();
            assert_eq!(serde_json::from_str::<AutoOr<Precision>>(&text).unwrap(), value, "{text}");
        }
    }

    #[test]
    fn another_word_is_refused_by_name() {
        let error = serde_json::from_str::<AutoOr<u8>>("\"lots\"").unwrap_err().to_string();
        assert!(error.contains("expected \"auto\" or a value, not \"lots\""), "{error}");
        let error = serde_json::from_str::<AutoOr<Precision>>("\"fp64\"").unwrap_err().to_string();
        assert!(error.contains("not \"fp64\""), "{error}");
    }
}

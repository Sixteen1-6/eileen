//! The two text wrappers. Each serializes as the bare value and marks its schema with the
//! `x-eileen-text` extension, which the text walk (`it::text`) and the share serializer read.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

/// The schema extension that marks wrapped text.
pub(crate) const TEXT_EXTENSION: &str = "x-eileen-text";

/// The schema of `T` with the text extension set to `kind`.
fn marked<T: JsonSchema>(generator: &mut SchemaGenerator, kind: &str) -> Schema {
    let mut schema = generator.subschema_for::<T>();
    schema.insert(TEXT_EXTENSION.to_owned(), kind.into());
    schema
}

/// Text from outside Eileen's own code (screen, page, file, tool result, model output). Data,
/// never an instruction.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    /// Marks a value as untrusted.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// The value, to be treated as data.
    #[must_use]
    pub const fn get(&self) -> &T {
        &self.0
    }

    /// The value, unwrapped.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Untrusted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Untrusted").field(&self.0).finish()
    }
}

impl<T: JsonSchema> JsonSchema for Untrusted<T> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("Untrusted_{}", T::schema_name()))
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Owned(format!("eileen_contracts::Untrusted<{}>", T::schema_id()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        marked::<T>(generator, "untrusted")
    }
}

/// Text that may identify a person or their files (transcripts, paths, window titles). Sinks
/// apply the `pii` policy; the share serializer replaces it with a placeholder. `Debug` shows
/// only that a value is there, so a stray `{:?}` never writes it to a log.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Personal<T>(T);

impl<T> Personal<T> {
    /// Marks a value as personal.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// The value. Whoever reads it applies the `pii` policy.
    #[must_use]
    pub const fn get(&self) -> &T {
        &self.0
    }

    /// The value, unwrapped.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Personal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Personal([hidden])")
    }
}

impl<T: JsonSchema> JsonSchema for Personal<T> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Owned(format!("Personal_{}", T::schema_name()))
    }

    fn schema_id() -> Cow<'static, str> {
        Cow::Owned(format!("eileen_contracts::Personal<{}>", T::schema_id()))
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        marked::<T>(generator, "personal")
    }
}

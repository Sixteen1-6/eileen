//! The schema catalogue: one JSON Schema generator over every contract type, with each
//! definition assigned to the module that owns the type. The snapshot tests pin one module per
//! snapshot, so a field added in one module changes only that module's file; `cargo xtask
//! codegen` (phase B) renders the same catalogue into the generated contract schemas.

use std::collections::BTreeMap;

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::Value;

/// The function that registers one module's types.
type RegisterFn = fn(&mut Registry);

/// Every contract module, in the order BUILD.md section 3.3 lists them, with the function that
/// registers its types. The names are the keys of `modules.toml`.
const MODULES: &[(&str, RegisterFn)] = &[
    ("ids", crate::ids::register),
    ("jobs", crate::jobs::register),
    ("core_io", crate::core_io::register),
    ("bus", crate::bus::register),
    ("desktop", crate::desktop::register),
    ("hud", crate::hud::register),
    ("speech", crate::speech::register),
    ("harness", crate::harness::register),
    ("tools", crate::tools::register),
    ("tools/brain", crate::tools::brain::register),
    ("tools/worker", crate::tools::worker::register),
    ("tools/desktop", crate::tools::desktop::register),
    ("tools/apps", crate::tools::apps::register),
    ("tools/learn", crate::tools::learn::register),
    ("tools/session", crate::tools::session::register),
    ("tools/dev", crate::tools::dev::register),
    ("intent", crate::intent::register),
    ("config", crate::config::register),
    ("connectors", crate::connectors::register),
    ("learn", crate::learn::register),
    ("scenario", crate::scenario::register),
    ("record", crate::record::register),
    ("fleet", crate::fleet::register),
    ("ports", crate::ports::register),
];

/// The names of every contract module, as `modules.toml` keys them.
#[must_use]
pub fn module_names() -> Vec<&'static str> {
    MODULES.iter().map(|(name, _)| *name).collect()
}

/// Collects the types of one module at a time into a shared generator.
#[derive(Debug)]
pub(crate) struct Registry {
    generator: SchemaGenerator,
    module: &'static str,
    owners: BTreeMap<String, &'static str>,
    duplicates: Vec<String>,
}

impl Registry {
    fn new() -> Self {
        Self {
            generator: settings().into_generator(),
            module: "",
            owners: BTreeMap::new(),
            duplicates: Vec::new(),
        }
    }

    /// Registers `T` as a type of the current module and generates its schema.
    pub(crate) fn add<T: JsonSchema>(&mut self) {
        let name = T::schema_name().into_owned();
        match self.owners.get(&name) {
            Some(owner) if *owner != self.module => {
                self.duplicates.push(format!("{name} (in {owner} and {})", self.module));
            }
            Some(_) => {}
            None => {
                self.owners.insert(name, self.module);
            }
        }
        drop(self.generator.subschema_for::<T>());
    }
}

/// The schema settings every contract schema uses: JSON Schema 2020-12, describing what the
/// types accept when deserializing.
#[must_use]
pub fn settings() -> SchemaSettings {
    SchemaSettings::draft2020_12()
}

/// Every definition, by module, and what does not fit.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    /// Module name to its definitions, each a schema keyed by its schema name.
    pub modules: BTreeMap<&'static str, BTreeMap<String, Value>>,
    /// Definitions no module registered: a type that should be registered, or a name collision
    /// that schemars resolved with a numeric suffix.
    pub unowned: Vec<String>,
    /// Types registered by more than one module under the same schema name.
    pub duplicates: Vec<String>,
    /// Types registered but inlined, so they have no definition to pin.
    pub inlined: Vec<String>,
}

/// Builds the catalogue of every contract type.
#[must_use]
pub fn catalogue() -> Catalogue {
    let mut registry = Registry::new();
    for (name, register) in MODULES {
        registry.module = name;
        register(&mut registry);
    }
    let definitions = registry.generator.take_definitions(true);
    let mut catalogue = Catalogue {
        modules: MODULES.iter().map(|(name, _)| (*name, BTreeMap::new())).collect(),
        duplicates: registry.duplicates,
        ..Catalogue::default()
    };
    for (name, schema) in definitions {
        match registry.owners.get(&name).and_then(|owner| catalogue.modules.get_mut(owner)) {
            Some(module) => {
                module.insert(name, schema);
            }
            None => catalogue.unowned.push(name),
        }
    }
    for (name, owner) in &registry.owners {
        let defined = catalogue.modules.get(owner).is_some_and(|module| module.contains_key(name));
        if !defined {
            catalogue.inlined.push(name.clone());
        }
    }
    catalogue
}

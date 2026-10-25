//! Eileen's configuration (BUILD.md sections 2.11 and 3.3.11): the settings registry, loading
//! the TOML files with every error naming its line, validation and safety bounds, edits that
//! keep the user's comments, and `explain`. The pure parts back the core's settings flow through
//! the `SettingsValidator` port; the file IO serves the runtime's driver. A stub until WP7 builds
//! it.
//!
//! # Owns
//!
//! - [`registry`], one [`SettingSpec`] per configuration leaf key; [`load`], [`validate`],
//!   [`apply`] and [`explain`].
//! - [`Validator`], the implementation of `eileen_contracts::ports::SettingsValidator`.
//! - `eileen config` and `eileen apps add-mcp` ([`cli`]) and the `config_toml` fuzz target's
//!   entry (`fuzz_entry`).
//!
//! # Does not own
//!
//! - The configuration's types (`eileen-contracts::config`), the propose, check, confirm, apply
//!   and undo flow (`eileen-core::settings`), or secrets (the `SecretStore` trait, which
//!   `eileen-cli` hands it).
//!
//! # Invariants
//!
//! - The registry covers every configuration leaf key exactly once, with matching docs (check
//!   27, a test here from WP7).
//! - `Safety` keys change only with a `UserInput` approval.
//! - Links no native library and nothing that can hang (check 24).
//! - The stub loads, validates and applies nothing: each says it is not built yet (unit tests
//!   in `tests`).
//!
//! # Boundaries
//!
//! - Called by `eileen-core` through the port, `eileen-runtime` (the files) and `eileen-cli`.
//! - May depend on `eileen-platform`, `eileen-contracts`, `eileen-stdx` and `eileen-trace`.
//! - Wire types: the configuration files, in the contract types' TOML form.
//!
//! # Testing
//!
//! - Fast command: `cargo xtask test -p eileen-config` (until `wp0/xtask` merges,
//!   `cargo nextest run -p eileen-config`).
//! - Layers: the registry's coverage test, table tests per key, round trips that keep comments,
//!   and the `config_toml` fuzz target.
//! - A bug here starts as an error that names no line, or an edit that lost a comment.
//!
//! # Traps
//!
//! - Edits go through `toml_edit` so the user's comments and order survive.
//!
//! # Logs and tracing
//!
//! - None yet; WP7 declares its events in `eileen_trace::names`.
//!
//! # Do not change without a decision record
//!
//! - Which keys are `Safety` (section 2.11) and their bounds.

#![forbid(unsafe_code)]

pub mod cli;
#[doc(hidden)]
pub mod fuzz_entry;

use std::path::Path;

use eileen_contracts::config::{
    ApplyMode, Config, Proposal, Risk, SettingChange, SettingSpecView, SettingsSource,
    ValidationError,
};
use eileen_contracts::ports::SettingsValidator;

/// One setting in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    /// The dotted key, for example `assistant.name`.
    pub key: &'static str,
    /// Its documentation.
    pub doc: &'static str,
    /// Whether it can be changed by voice.
    pub voice: bool,
    /// Whether it can be changed on the HUD.
    pub hud: bool,
    /// Its risk.
    pub risk: Risk,
    /// When a change takes effect.
    pub apply: ApplyMode,
}

/// A problem in a configuration file that did not stop it loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The file's line.
    pub line: u32,
    /// The key, when there is one.
    pub key: Option<String>,
    /// What is wrong, in one sentence.
    pub message: String,
}

/// Where each value came from: the defaults, a profile, the user's files. WP7 adds its fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layers;

/// One setting's effective value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explained {
    /// The dotted key.
    pub key: String,
    /// The value, as TOML.
    pub value: String,
    /// Where it came from.
    pub source: SettingsSource,
}

/// What went wrong with the configuration. WP7's errors each name a line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Not built yet.
    #[error("{what} is not built yet (WP7 builds it)")]
    Unavailable {
        /// What was asked for.
        what: &'static str,
    },
}

/// Every setting. The stub's registry is empty.
#[must_use]
#[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's registry")]
pub fn registry() -> &'static [SettingSpec] {
    &[]
}

/// Loads the configuration from `dir`, with the problems that did not stop it.
#[expect(clippy::missing_const_for_fn, reason = "a stub until WP7 reads the files")]
pub fn load(_dir: &Path) -> Result<(Config, Vec<Diagnostic>), ConfigError> {
    Err(ConfigError::Unavailable { what: "loading the configuration" })
}

/// Checks `changes` against `current`: validation, the diff and the safety bounds.
#[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's validation")]
pub fn validate(_current: &Config, _changes: &[SettingChange]) -> Result<Proposal, ConfigError> {
    Err(ConfigError::Unavailable { what: "validating settings" })
}

/// Applies `changes` to the TOML text, keeping its comments.
#[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's edits")]
pub fn apply(_current_toml: &str, _changes: &[SettingChange]) -> Result<String, ConfigError> {
    Err(ConfigError::Unavailable { what: "editing the configuration" })
}

/// Every setting's effective value and where it came from. The stub explains nothing.
#[must_use]
#[expect(clippy::missing_const_for_fn, reason = "a stub until WP7's explain")]
pub fn explain(_config: &Config, _layers: &Layers) -> Vec<Explained> {
    Vec::new()
}

/// The settings validator the core's settings flow calls. The stub refuses every change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Validator;

impl SettingsValidator for Validator {
    fn validate(
        &self,
        _current: &Config,
        _changes: &[SettingChange],
    ) -> Result<Proposal, ValidationError> {
        Err(ValidationError {
            key: String::new(),
            line: None,
            message: "the settings validator is not built yet".to_owned(),
        })
    }

    fn specs(&self) -> &[SettingSpecView] {
        &[]
    }
}

#[cfg(test)]
mod tests;

//! The checks' data in `xtask/tidy/`: the hashed denylist, the files exempt from check 6, the
//! long modules allowed with a reason, the folders whose guidance may differ, the sanctioned
//! `#[expect(clippy::disallowed_methods)]` sites and the credential boundary's patterns. Each
//! file is read through the [`Tree`], so a fixture supplies its own.

use std::collections::BTreeSet;

use super::{Tree, TreeError};

const DENYLIST: &str = "xtask/tidy/denylist.toml";
const EXEMPT: &str = "xtask/tidy/exempt.toml";
const ALLOW: &str = "xtask/tidy/allow.toml";
const GUIDANCE: &str = "xtask/tidy/guidance.toml";
const DISALLOWED: &str = "xtask/tidy/disallowed-methods.toml";
const CREDENTIALS: &str = "xtask/tidy/credential-boundary.toml";

/// A path, with the reason it is listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    /// A file, or a folder (everything under it).
    pub(crate) path: String,
    /// Why it is listed.
    pub(crate) reason: String,
}

/// One sanctioned `#[expect(clippy::disallowed_methods)]` site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Site {
    /// The file, or a folder for a function whose file is not fixed yet.
    pub(crate) path: String,
    /// The function the expectation sits on or in.
    pub(crate) function: String,
    /// Whether it may also call `Command::new`.
    pub(crate) spawns: bool,
}

/// Everything in `xtask/tidy/` the checks read.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tidy {
    /// The denylist's salt.
    pub(crate) salt: String,
    /// The denylist's salted hashes, as lowercase hex.
    pub(crate) denied: BTreeSet<String>,
    /// Files and folders check 6 does not scan.
    pub(crate) style_exempt: Vec<Listed>,
    /// Modules allowed over 800 lines.
    pub(crate) long_modules: Vec<Listed>,
    /// Folders that may hold a nested `AGENTS.md`.
    pub(crate) nested: Vec<Listed>,
    /// Folders whose guidance files are copies for other projects, which checks 2 and 4 skip.
    pub(crate) guidance_ignored: Vec<Listed>,
    /// Folders exempt from check 14's spawn and expectation rules.
    pub(crate) spawn_exempt: Vec<Listed>,
    /// The sanctioned expectation sites.
    pub(crate) sites: Vec<Site>,
    /// Vendor credential paths no source may name.
    pub(crate) credential_paths: Vec<String>,
    /// Commands no source may name.
    pub(crate) credential_commands: Vec<String>,
    /// The extensions of the files check 14's credential scan reads.
    pub(crate) source_extensions: Vec<String>,
    /// Files and folders that credential scan skips.
    pub(crate) credential_exempt: Vec<Listed>,
}

impl Tidy {
    /// Reads every tidy file; a missing file is an empty list (a missing denylist has no
    /// entries), a malformed one stops the run.
    pub(crate) fn load(tree: &dyn Tree) -> Result<Self, TreeError> {
        let denylist = table(tree, DENYLIST)?;
        let exempt = table(tree, EXEMPT)?;
        let allow = table(tree, ALLOW)?;
        let guidance = table(tree, GUIDANCE)?;
        let disallowed = table(tree, DISALLOWED)?;
        let credentials = table(tree, CREDENTIALS)?;
        let sites = disallowed
            .get("site")
            .and_then(toml::Value::as_array)
            .map(|items| items.iter().map(site).collect::<Result<Vec<_>, _>>())
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            salt: denylist.get("salt").and_then(toml::Value::as_str).unwrap_or_default().to_owned(),
            denied: strings(&denylist, "entries").into_iter().map(|e| e.to_lowercase()).collect(),
            style_exempt: listed(&exempt, "exempt", EXEMPT)?,
            long_modules: listed(&allow, "module", ALLOW)?,
            nested: listed_dirs(&guidance, "nested")?,
            guidance_ignored: listed_dirs(&guidance, "ignore")?,
            spawn_exempt: listed(&disallowed, "exempt", DISALLOWED)?,
            sites,
            credential_paths: strings(&credentials, "paths"),
            credential_commands: strings(&credentials, "commands"),
            source_extensions: strings(&credentials, "extensions"),
            credential_exempt: listed(&credentials, "exempt", CREDENTIALS)?,
        })
    }
}

/// Whether `path` is one of `listed` or under one of them.
pub(crate) fn is_listed(listed: &[Listed], path: &str) -> bool {
    listed.iter().any(|entry| super::under(path, &entry.path))
}

fn table(tree: &dyn Tree, path: &str) -> Result<toml::Table, TreeError> {
    match tree.read(path)? {
        None => Ok(toml::Table::new()),
        Some(text) => {
            text.parse::<toml::Table>().map_err(|error| TreeError(format!("{path}: {error}")))
        }
    }
}

fn strings(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(toml::Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The `[[key]]` entries of a tidy file, each a `path` and a `reason`.
fn listed(table: &toml::Table, key: &str, file: &str) -> Result<Vec<Listed>, TreeError> {
    entries(table, key, "path", file)
}

/// The `[[key]]` entries of the guidance file, each a `dir` and a `reason`.
fn listed_dirs(table: &toml::Table, key: &str) -> Result<Vec<Listed>, TreeError> {
    entries(table, key, "dir", GUIDANCE)
}

fn entries(
    table: &toml::Table,
    key: &str,
    path_key: &str,
    file: &str,
) -> Result<Vec<Listed>, TreeError> {
    let Some(items) = table.get(key).and_then(toml::Value::as_array) else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|item| {
            let field = |name: &str| item.get(name).and_then(toml::Value::as_str);
            match (field(path_key), field("reason")) {
                (Some(path), Some(reason)) if !reason.trim().is_empty() => {
                    Ok(Listed { path: path.to_owned(), reason: reason.to_owned() })
                }
                _ => Err(TreeError(format!(
                    "{file}: every [[{key}]] needs a `{path_key}` and a non-empty `reason`"
                ))),
            }
        })
        .collect()
}

fn site(item: &toml::Value) -> Result<Site, TreeError> {
    let field = |name: &str| item.get(name).and_then(toml::Value::as_str);
    match (field("path"), field("function"), field("reason")) {
        (Some(path), Some(function), Some(reason)) if !reason.trim().is_empty() => Ok(Site {
            path: path.to_owned(),
            function: function.to_owned(),
            spawns: item.get("spawns").and_then(toml::Value::as_bool).unwrap_or(false),
        }),
        _ => Err(TreeError(format!(
            "{DISALLOWED}: every [[site]] needs a `path`, a `function` and a non-empty `reason`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::mem::MemTree;
    use super::Tidy;

    #[test]
    fn the_clean_fixtures_tidy_files_load() {
        let tidy = Tidy::load(&MemTree::clean()).unwrap();
        assert_eq!(tidy.salt.len(), 32, "the salt");
        assert!(tidy.denied.is_empty(), "no entries");
        assert_eq!(tidy.sites.len(), 1, "one site");
        assert!(tidy.sites.iter().all(|s| s.spawns), "it may spawn");
        assert_eq!(tidy.nested.len(), 1, "one nested folder");
    }

    #[test]
    fn an_entry_without_a_reason_stops_the_run() {
        let mut tree = MemTree::clean();
        tree.set("xtask/tidy/allow.toml", "[[module]]\npath = \"a.rs\"\nreason = \" \"\n");
        let error = Tidy::load(&tree).unwrap_err();
        assert!(error.0.contains("reason"), "it names the missing reason: {error}");
    }
}

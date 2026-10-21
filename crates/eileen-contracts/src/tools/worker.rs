//! The worker toolset's arguments and results (`ask_user` is the brain toolset's).
//!
//! Provisional: WP3 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ToolInput, input};
use crate::ids::Untrusted;
use crate::learn::RouteKind;
use crate::schema::Registry;

const MODULE: &str = "tools/worker";

/// `report`: a worker's result for its job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// What it did, in a few sentences.
    pub summary: String,
    /// Files it made.
    #[serde(default)]
    #[schemars(extend("x-eileen-path" = true))]
    pub artifacts: Vec<String>,
    /// How it ended.
    pub outcome: ReportOutcome,
}

/// How a worker's job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReportOutcome {
    /// Done.
    Done,
    /// Blocked on something only the user can do.
    Blocked,
    /// Failed.
    Failed,
}

/// `routes_lookup`: Eileen's apps sidecar queries the official MCP registry (one fixed host,
/// through the supervisor, 10 s, 256 KB cap, cached a day). Tier 0; observes `Web`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RoutesLookup {
    /// The app, a key of Eileen's app index.
    pub app: String,
}

/// What `routes_lookup` returns, all of it untrusted registry data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RoutesLookupOutput {
    /// The servers the registry lists for the app.
    pub servers: Vec<RegistryServer>,
}

/// One registry entry. `namespace` is the registry-verified part of the server name before the
/// `/`; `from_app_vendor` is computed by Eileen from `app-vendors.toml`; the registry's `_meta`
/// "official" block is never read as an endorsement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RegistryServer {
    /// The registry's id for it.
    pub registry_id: String,
    /// Its name.
    pub name: Untrusted<String>,
    /// The verified namespace.
    pub namespace: String,
    /// The namespace is a domain listed for this app in `app-vendors.toml`.
    pub from_app_vendor: bool,
    /// Its registry status.
    pub status: Untrusted<String>,
    /// Its description.
    pub description: Untrusted<String>,
    /// Its source repository.
    pub source_url: Untrusted<String>,
    /// Its packages.
    pub packages: Vec<RegistryPackage>,
}

/// One package of a registry entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RegistryPackage {
    /// The package registry (`npm`, `pypi`, ...).
    pub registry_type: Untrusted<String>,
    /// The package's identifier.
    pub identifier: Untrusted<String>,
    /// The version.
    pub version: Untrusted<String>,
}

/// `routes_propose`: records a route proposal in `learn.sqlite` and shows it on the HUD;
/// installs nothing. `AppMcp` needs `registry_id`, which must match an active `routes_lookup`
/// entry cached for this job and app, and every shown field is copied from that entry; `AppApi`
/// (with `link`, stored as `source_url`) and `DeepLink` (with `uri_scheme`) are stored as
/// unverified. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RoutesPropose {
    /// The app.
    pub app: String,
    /// The kind of route.
    pub kind: RouteKind,
    /// `AppMcp`: the registry entry.
    pub registry_id: Option<String>,
    /// `DeepLink`: the URI scheme.
    pub uri_scheme: Option<String>,
    /// `AppApi`: the documentation link.
    pub link: Option<String>,
    /// The worker's note on why.
    pub why: String,
}

/// `approve`: only if WP3's contract test picks the permission-prompt-tool route; accepted only
/// for a pending ask with this `tool_use_id`. The reply's shape is recorded from the real CLI by
/// WP3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Approve {
    /// The tool asking.
    pub tool_name: String,
    /// Its input.
    pub input: Value,
    /// The vendor's id for the use.
    pub tool_use_id: String,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<Report>(out, MODULE, "report");
    input::<RoutesLookup>(out, MODULE, "routes_lookup");
    input::<RoutesPropose>(out, MODULE, "routes_propose");
    input::<Approve>(out, MODULE, "approve");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<Report>();
    r.add::<ReportOutcome>();
    r.add::<RoutesLookup>();
    r.add::<RoutesLookupOutput>();
    r.add::<RegistryServer>();
    r.add::<RegistryPackage>();
    r.add::<RoutesPropose>();
    r.add::<Approve>();
}

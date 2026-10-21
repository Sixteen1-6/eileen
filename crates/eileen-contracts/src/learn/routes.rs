//! Route proposals and the route and source statistics.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core_io::Rung;
use crate::ids::{JobId, Personal, RouteProposalId, Untrusted, WallTime};
use crate::schema::Registry;

/// A worker's proposal of a better route to an app. `app` is a key of Eileen's app index,
/// validated by the dispatcher; `registry_id`, `McpPackage.version` and `Publisher.namespace`
/// have validated formats (a registry id, an exact semantic version, a reverse-DNS or
/// `io.github.<user>` namespace). For `AppMcp` every other field but `why` comes from the
/// registry entry the proposal matched; `why` is the worker's note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteProposal {
    /// Its id.
    pub id: RouteProposalId,
    /// The app.
    pub app: String,
    /// The kind of route.
    pub kind: RouteKind,
    /// `AppMcp` matched to a registry entry; `AppApi` and `DeepLink` are always false.
    pub verified: bool,
    /// The registry entry.
    pub registry_id: Option<String>,
    /// The server's name.
    pub name: Untrusted<String>,
    /// The registry-verified namespace of the server name, never a free-text field.
    pub publisher: Option<Publisher>,
    /// The namespace is a domain listed for this app in `app-vendors.toml`; there is no
    /// "official" flag (the registry's `_meta` block is bookkeeping).
    pub from_app_vendor: bool,
    /// `AppMcp`: the registry entry's repository; `AppApi`: the worker's link.
    pub source_url: Untrusted<String>,
    /// The package to install.
    pub package: Option<McpPackage>,
    /// `DeepLink`: the URI scheme.
    pub uri_scheme: Option<Untrusted<String>>,
    /// The worker's note.
    pub why: Untrusted<String>,
    /// The job that proposed it.
    pub job: JobId,
    /// Where it is.
    pub status: ProposalStatus,
}

/// The kinds of route a worker can propose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    /// The app's own MCP server, from the registry.
    AppMcp,
    /// The app's scripting interface, by its documentation.
    AppApi,
    /// A URI scheme.
    DeepLink,
}

/// Where a route proposal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Waiting for the user.
    Open,
    /// Accepted.
    Accepted,
    /// Dismissed.
    Dismissed,
    /// Refused by Eileen's own checks.
    Refused,
}

/// A registry-verified publisher. `GitHub { user }` for `io.github.<user>` (verified by the
/// registry through that account); `Domain` for a reverse-DNS namespace (verified through DNS
/// or HTTP); `display` is what the HUD shows and Eileen speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Publisher {
    /// The namespace.
    pub namespace: String,
    /// How it was verified.
    pub kind: PublisherKind,
    /// What is shown and spoken.
    pub display: String,
}

/// How a publisher's namespace was verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublisherKind {
    /// Through a GitHub account.
    #[serde(rename = "github")]
    GitHub,
    /// Through DNS or HTTP.
    Domain,
}

/// A package to install: an exact version, installed once into
/// `%LOCALAPPDATA%\eileen\mcp\<app>\`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct McpPackage {
    /// The package registry.
    pub registry_type: PackageType,
    /// The package's identifier.
    pub identifier: Untrusted<String>,
    /// The exact version.
    pub version: String,
}

/// The package registries Eileen installs from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    /// npm.
    Npm,
    /// `PyPI`.
    Pypi,
    /// Shipped with the app.
    AppBundled,
}

/// How one route of an app has done at one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RouteStat {
    /// The app.
    pub app: String,
    /// The app's major version.
    pub app_major: String,
    /// The task, for example `navigation`.
    pub task: String,
    /// The route.
    pub rung: Rung,
    /// Successes.
    pub successes: u32,
    /// Failures.
    pub failures: u32,
    /// The median time, ms.
    pub median_ms: u32,
    /// Times a better rung was skipped for it.
    pub rung_skipped: u32,
}

/// Which sources gave accepted deep answers per topic (section 2.10 item 11). `host` is a
/// registrable domain, nothing more; `topic` is the deep answer's topic key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceStat {
    /// The topic.
    pub topic: Personal<String>,
    /// The source's registrable domain.
    pub host: String,
    /// Times it was cited.
    pub cited: u32,
    /// Times an answer citing it was accepted.
    pub accepted: u32,
    /// Times the user opened it.
    pub opened: u32,
    /// When it was last used.
    pub last: WallTime,
}

pub(super) fn register(r: &mut Registry) {
    r.add::<RouteProposal>();
    r.add::<RouteKind>();
    r.add::<ProposalStatus>();
    r.add::<Publisher>();
    r.add::<PublisherKind>();
    r.add::<McpPackage>();
    r.add::<PackageType>();
    r.add::<RouteStat>();
    r.add::<SourceStat>();
}

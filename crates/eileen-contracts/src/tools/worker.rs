//! The worker toolset's arguments and results (`ask_user` is the brain toolset's).
//!
//! `approve` is the permission-prompt tool of BUILD.md section 2.5, the route WP3's contract
//! test picked: after the tier guard answers `ask`, the CLI calls it with [`Approve`] and keeps
//! it out of the model's tool list. The dispatcher answers with an [`ApproveReply`] in one text
//! block, in one of the two shapes the CLI accepted in that test
//! (`docs/research/fixtures/wp3-approval/mcp-approve.json`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ToolInput, input};
use crate::core_io::Content;
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

/// `approve`: the permission-prompt tool (section 2.5, route 2). The CLI calls it after the tier
/// guard answers `ask`, with the guard's `tool_use_id`. Accepted only for a pending ask with
/// that id and that exact input; held up to `approvals.hook_hold_s` while the user decides;
/// answered with an `ApproveReply`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Approve {
    /// The tool asking.
    pub tool_name: String,
    /// Its input, as the CLI sends it.
    pub input: Value,
    /// The vendor's id for the use, the one the guard saw.
    pub tool_use_id: String,
}

impl Approve {
    /// Whether this call answers the pending ask the guard issued for `tool_use_id`, whose input
    /// it stored as `stored`: the same id, and an input equal to the stored one as JSON (so key
    /// order does not count and any changed value does). The dispatcher refuses any other call
    /// and logs it as a security event.
    #[must_use]
    pub fn matches_ask(&self, tool_use_id: &str, stored: &Untrusted<Value>) -> bool {
        self.tool_use_id == tool_use_id && self.input == *stored.get()
    }
}

/// What `approve` answers: one text block holding this as JSON, in one of the two shapes the CLI
/// accepts. `allow` carries the input the guard stored with the ask, never one built from the
/// call, because the CLI runs `updatedInput` without calling the guard again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum ApproveReply {
    /// Run the call; only with an `ApprovalId` that came from `UserInput`.
    Allow {
        /// The input the guard stored with the ask.
        #[serde(rename = "updatedInput")]
        updated_input: Untrusted<Value>,
    },
    /// Do not run it.
    Deny {
        /// One sentence for the model, written by Eileen's code.
        message: String,
    },
}

/// The text of a reply `serde_json` could not write: a denial, so a failure never allows.
const UNWRITTEN_REPLY: &str =
    r#"{"behavior":"deny","message":"Eileen could not write its answer, so this call is denied."}"#;

impl ApproveReply {
    /// Allows the pending ask with the input the guard stored when it answered `ask`: the
    /// `tool_input` of that `PreToolUse` request, an `Untrusted<Value>` as the request carries
    /// it. [`Approve::input`] is a plain `Value`, so the call's own input cannot be passed here
    /// by accident; the CLI would run it unchecked (the approval contract test's finding 7).
    ///
    /// ```
    /// use eileen_contracts::ids::Untrusted;
    /// use eileen_contracts::tools::worker::{Approve, ApproveReply};
    /// use serde_json::json;
    ///
    /// let stored = Untrusted::new(json!({ "command": "git push" }));
    /// let call = Approve {
    ///     tool_name: "Bash".to_owned(),
    ///     input: json!({ "command": "git push" }),
    ///     tool_use_id: "toolu_01".to_owned(),
    /// };
    /// assert!(call.matches_ask("toolu_01", &stored));
    /// let reply = ApproveReply::allow(stored);
    /// assert_eq!(reply.text(), r#"{"behavior":"allow","updatedInput":{"command":"git push"}}"#);
    /// ```
    ///
    /// The call's input does not type-check:
    ///
    /// ```compile_fail
    /// use eileen_contracts::tools::worker::{Approve, ApproveReply};
    /// use serde_json::json;
    ///
    /// let call = Approve {
    ///     tool_name: "Bash".to_owned(),
    ///     input: json!({ "command": "git push" }),
    ///     tool_use_id: "toolu_01".to_owned(),
    /// };
    /// let reply = ApproveReply::allow(call.input);
    /// ```
    #[must_use]
    pub const fn allow(stored: Untrusted<Value>) -> Self {
        Self::Allow { updated_input: stored }
    }

    /// Denies the call, with one sentence the model reads.
    #[must_use]
    pub fn deny(message: impl Into<String>) -> Self {
        Self::Deny { message: message.into() }
    }

    /// The JSON the text block holds, compact: `{"behavior":"allow","updatedInput":...}` or
    /// `{"behavior":"deny","message":"..."}`. `serde_json` cannot fail on these types (every map
    /// key is a string); if it ever did, the text would deny.
    #[must_use]
    pub fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| UNWRITTEN_REPLY.to_owned())
    }

    /// The one block the dispatcher answers `approve` with, which the shim writes as MCP's
    /// `{"type":"text","text":...}`. Its result is never an error, a denial included: the CLI
    /// reads an `approve` call that errors as a failed tool, not as a denial.
    #[must_use]
    pub fn content(&self) -> Content {
        Content::Text(Untrusted::new(self.text()))
    }
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
    r.add::<ApproveReply>();
}

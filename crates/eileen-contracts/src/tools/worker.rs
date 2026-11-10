//! The worker toolset's arguments and results (`ask_user` is the brain toolset's).
//!
//! `approve` is the permission-prompt tool of BUILD.md section 2.5, the route WP3's contract
//! test picked: after the tier guard answers `ask`, the CLI calls it with [`Approve`] and keeps
//! it out of the model's tool list. The dispatcher answers with an [`ApproveReply`] in one text
//! block, in one of the two shapes the CLI accepted in that test
//! (`docs/research/fixtures/wp3-approval/mcp-approve.json`). An allow is built only from a
//! [`PendingAsk`], which only the guard's `PreToolUse` request makes, so it carries the input the
//! guard saw and never the call's.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ToolInput, input};
use crate::core_io::Content;
use crate::harness::HookRequest;
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
/// that tool, that id and that exact input; held up to `approvals.hook_hold_s` while the user
/// decides; answered with an `ApproveReply`.
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
    /// Whether this call answers `ask`: the same tool, the same `tool_use_id`, and an input equal
    /// to the stored one as JSON (so key order does not count and any changed value does). The
    /// dispatcher refuses any other call and logs it as a security event.
    #[must_use]
    pub fn matches_ask(&self, ask: &PendingAsk) -> bool {
        self.tool_use_id == ask.tool_use_id
            && self.tool_name == ask.tool_name
            && self.input == *ask.input.0.get()
    }
}

/// An ask the tier guard issued, which the dispatcher holds until the user decides: the tool,
/// the vendor's id for the use and the input of the guard's `PreToolUse` request.
///
/// An allow can be built only from one. Its fields are private and its one constructor,
/// [`PendingAsk::from_guard_ask`], reads a `PreToolUse` request, so neither the `approve` call's
/// input, nor the shim's arguments, nor a `PermissionRequest`'s input can become one. It is not
/// `Clone` and not deserializable, and [`ApproveReply::allow`] consumes it, so one ask allows
/// once. The one way around it is to write a `HookRequest` out by hand from the call, a
/// deliberate step that review sees.
///
/// ```
/// use eileen_contracts::harness::HookRequest;
/// use eileen_contracts::ids::Untrusted;
/// use eileen_contracts::tools::worker::{Approve, ApproveReply, PendingAsk};
/// use serde_json::json;
///
/// // The guard's request, which it answered `ask`.
/// let request = HookRequest::PreToolUse {
///     request: 1,
///     tool_name: "Bash".to_owned(),
///     tool_input: Untrusted::new(json!({ "command": "git push" })),
///     tool_use_id: "toolu_01".to_owned(),
///     paths: Vec::new(),
///     git: Vec::new(),
///     urls: Vec::new(),
/// };
/// let ask = PendingAsk::from_guard_ask(&request).unwrap();
/// // The CLI's `approve` call for it.
/// let call = Approve {
///     tool_name: "Bash".to_owned(),
///     input: json!({ "command": "git push" }),
///     tool_use_id: "toolu_01".to_owned(),
/// };
/// assert!(call.matches_ask(&ask));
/// let reply = ApproveReply::allow(ask);
/// assert_eq!(reply.text(), r#"{"behavior":"allow","updatedInput":{"command":"git push"}}"#);
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct PendingAsk {
    tool_name: String,
    tool_use_id: String,
    input: AskedInput,
}

impl PendingAsk {
    /// The ask for `request`, the hook request the guard answered `ask`: `Some` for a
    /// `PreToolUse` request, `None` for any other kind.
    #[must_use]
    pub fn from_guard_ask(request: &HookRequest) -> Option<Self> {
        let HookRequest::PreToolUse { tool_name, tool_input, tool_use_id, .. } = request else {
            return None;
        };
        Some(Self {
            tool_name: tool_name.clone(),
            tool_use_id: tool_use_id.clone(),
            input: AskedInput(tool_input.clone()),
        })
    }

    /// The tool asking.
    #[must_use]
    pub const fn tool_name(&self) -> &str {
        self.tool_name.as_str()
    }

    /// The vendor's id for the use, the key the dispatcher holds the ask under.
    #[must_use]
    pub const fn tool_use_id(&self) -> &str {
        self.tool_use_id.as_str()
    }

    /// The input the guard stored, for the question Eileen builds from it.
    #[must_use]
    pub const fn input(&self) -> &Untrusted<Value> {
        &self.input.0
    }
}

/// The input an allow carries: the `tool_input` of the guard's `PreToolUse` request, moved out
/// of a [`PendingAsk`]. Its field is private, so no other value becomes one. It serializes as
/// the bare input.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AskedInput(Untrusted<Value>);

impl AskedInput {
    /// The input, to be treated as data.
    #[must_use]
    pub const fn get(&self) -> &Untrusted<Value> {
        &self.0
    }
}

/// The schema of the untrusted input it holds, inline, so the reply's schema shows the input
/// itself.
impl JsonSchema for AskedInput {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("AskedInput")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        Untrusted::<Value>::json_schema(generator)
    }
}

/// What `approve` answers: one text block holding this as JSON, in one of the two shapes the CLI
/// accepts. `allow` carries the input the guard stored with the ask, never one built from the
/// call, because the CLI runs `updatedInput` without calling the guard again.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum ApproveReply {
    /// Run the call; only with an `ApprovalId` that came from `UserInput`.
    Allow {
        /// The input the guard stored with the ask.
        #[serde(rename = "updatedInput")]
        updated_input: AskedInput,
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
    /// Allows `ask` with the input the guard stored when it answered `ask`, and consumes it.
    /// Section 2.5's echo rule needs exactly that input: the CLI runs `updatedInput` without
    /// calling the guard again (finding 7 of the approval contract test). The dispatcher calls
    /// it only for an ask the `approve` call matched ([`Approve::matches_ask`]) and only with an
    /// `ApprovalId` that came from `UserInput`.
    ///
    /// Each example below builds the same call as the one on [`PendingAsk`] and fails only on
    /// its last line. The call's input cannot be passed here, wrapped as untrusted or not,
    /// because `allow` takes a [`PendingAsk`]:
    ///
    /// ```compile_fail,E0308
    /// # use eileen_contracts::ids::Untrusted;
    /// # use eileen_contracts::tools::worker::{Approve, ApproveReply};
    /// # use serde_json::json;
    /// # let call = Approve {
    /// #     tool_name: "Bash".to_owned(),
    /// #     input: json!({ "command": "git push" }),
    /// #     tool_use_id: "toolu_01".to_owned(),
    /// # };
    /// let reply = ApproveReply::allow(Untrusted::new(call.input));
    /// ```
    ///
    /// Nor can the variant be written out with it, because its field is an [`AskedInput`]:
    ///
    /// ```compile_fail,E0308
    /// # use eileen_contracts::ids::Untrusted;
    /// # use eileen_contracts::tools::worker::{Approve, ApproveReply};
    /// # use serde_json::json;
    /// # let call = Approve {
    /// #     tool_name: "Bash".to_owned(),
    /// #     input: json!({ "command": "git push" }),
    /// #     tool_use_id: "toolu_01".to_owned(),
    /// # };
    /// let reply = ApproveReply::Allow { updated_input: Untrusted::new(call.input) };
    /// ```
    ///
    /// Nor can an [`AskedInput`] be made from it, because its field is private:
    ///
    /// ```compile_fail,E0603
    /// # use eileen_contracts::ids::Untrusted;
    /// # use eileen_contracts::tools::worker::{Approve, ApproveReply, AskedInput};
    /// # use serde_json::json;
    /// # let call = Approve {
    /// #     tool_name: "Bash".to_owned(),
    /// #     input: json!({ "command": "git push" }),
    /// #     tool_use_id: "toolu_01".to_owned(),
    /// # };
    /// let reply = ApproveReply::Allow { updated_input: AskedInput(Untrusted::new(call.input)) };
    /// ```
    ///
    /// Nor can a reply be parsed from it. Eileen writes replies and the CLI reads them, so
    /// `ApproveReply` is not deserializable:
    ///
    /// ```compile_fail,E0277
    /// # use eileen_contracts::tools::worker::{Approve, ApproveReply};
    /// # use serde_json::json;
    /// # let call = Approve {
    /// #     tool_name: "Bash".to_owned(),
    /// #     input: json!({ "command": "git push" }),
    /// #     tool_use_id: "toolu_01".to_owned(),
    /// # };
    /// let reply: Result<ApproveReply, _> =
    ///     serde_json::from_value(json!({ "behavior": "allow", "updatedInput": call.input }));
    /// ```
    #[must_use]
    pub fn allow(ask: PendingAsk) -> Self {
        Self::Allow { updated_input: ask.input }
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

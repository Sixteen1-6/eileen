//! Worker and brain drivers (BUILD.md section 3.3.8): the sessions, commands, events and hooks
//! that pass between the core and `eileen-harness`.
//!
//! The driver traits (`WorkerDriver`, `BrainBackend`), their enum dispatch and their errors are
//! `eileen-harness`'s own; only the data they carry is a contract.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core_io::{GitFact, PathFact, UrlFact};
use crate::ids::{ApprovalId, JobId, Personal, TraceCtx, TurnId, Untrusted, WorkerSessionId};
use crate::jobs::{Cost, JobKind, PlanUsage, PoolClass, Route, Toolset, TurnAuthority};
use crate::learn::{SkillRef, SnapshotManifest};
use crate::schema::Registry;

/// The brain's process slots. The runtime holds up to three: `Primary` (`routing.brain`),
/// `Spare` (the same route) and `Fallback` (`routing.fallback_brain`, started only when it
/// differs from the primary).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BrainSlot {
    /// The brain that answers.
    Primary,
    /// A warm copy, promoted when the primary stalls or crashes.
    Spare,
    /// The fallback route, started on first need and stopped after 10 idle minutes.
    Fallback,
}

/// From the brain driver. `instance` numbers every brain process the runtime starts
/// (monotonic, never reused), so the core can tell a late delta from a stalled process apart
/// from the fallback's answer to the same turn: it accepts `TextDelta` and `TurnDone` for a
/// turn only from the instance it last sent that turn to, and drops (and counts) the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrainEvent {
    /// A brain process is ready.
    Ready {
        /// Its slot.
        which: BrainSlot,
        /// Its instance.
        instance: u32,
        /// Its route.
        route: Route,
        /// The resolved full model id.
        model: String,
    },
    /// Streamed text of a turn.
    TextDelta {
        /// The instance that sent it.
        instance: u32,
        /// The turn.
        turn: TurnId,
        /// The text.
        text: Untrusted<String>,
    },
    /// A turn finished.
    TurnDone {
        /// The instance that answered.
        instance: u32,
        /// The turn.
        turn: TurnId,
        /// What it cost.
        cost: Cost,
        /// The session's context size after the turn.
        context_tokens: u64,
        /// Whether the vendor reported an error.
        is_error: bool,
    },
    /// The vendor's rate-limit event.
    RateLimit(PlanUsage),
    /// A brain process exited.
    Exited {
        /// Its slot.
        which: BrainSlot,
        /// Its instance.
        instance: u32,
        /// Its exit code.
        code: Option<i32>,
    },
}

/// To the brain driver; every command names a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrainCmd {
    /// Send a turn.
    Turn {
        /// The slot.
        slot: BrainSlot,
        /// The turn.
        turn: TurnId,
        /// The composed message.
        message: String,
        /// Whether the turn was addressed.
        authority: TurnAuthority,
    },
    /// Interrupt the turn in progress.
    Interrupt {
        /// The slot.
        slot: BrainSlot,
    },
    /// Start a fresh session carrying a summary of the old one.
    Recycle {
        /// The slot.
        slot: BrainSlot,
        /// The summary.
        summary: String,
    },
    /// The named slot's process (the spare) becomes the primary; the old primary is killed and
    /// a new spare starts.
    Promote {
        /// The slot being promoted.
        slot: BrainSlot,
    },
    /// Start a slot's process (the fallback, on first need).
    Start {
        /// The slot.
        slot: BrainSlot,
    },
    /// Stop a slot's process (the fallback, after 10 idle minutes).
    Stop {
        /// The slot.
        slot: BrainSlot,
    },
}

/// From a worker session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerEvent {
    /// The session started; from its `system/init` event.
    Init {
        /// The vendor's session id.
        session_id: String,
        /// The process id.
        pid: u32,
        /// The resolved model id.
        model: String,
        /// The tools the session has.
        tools: Vec<String>,
        /// The MCP servers and their status.
        mcp_servers: Vec<String>,
        /// The plugins loaded.
        plugins: Vec<String>,
        /// The capabilities the CLI reports.
        capabilities: Vec<String>,
    },
    /// A narration line (text lines starting with `> `).
    Narration {
        /// The line.
        line: Untrusted<String>,
    },
    /// Streamed text.
    Text {
        /// The text.
        delta: Untrusted<String>,
    },
    /// A tool use.
    ToolUse {
        /// The vendor's id for it.
        tool_use_id: String,
        /// The tool's name.
        name: String,
        /// Its input, redacted, at most 2 KB.
        input: Untrusted<Value>,
    },
    /// A tool result.
    ToolResult {
        /// The vendor's id for the use.
        tool_use_id: String,
        /// Whether it succeeded.
        ok: bool,
        /// For every call section 2.12's observed-read table maps to a source (built-ins, shell
        /// reads, Claude in Chrome, third-party MCP tools, approved egress commands): the result
        /// text, redacted, at most 16 KB, for provenance only.
        observed: Option<Untrusted<String>>,
    },
    /// A `Skill` tool use.
    SkillInvoked {
        /// The skill's name.
        name: String,
        /// Its arguments.
        args: Untrusted<String>,
    },
    /// The session's Job Object hit `memory.worker_job_limit_mb`.
    MemoryLimit {
        /// The limit, MB.
        limit_mb: u32,
    },
    /// A turn finished.
    TurnDone {
        /// What it cost.
        cost: Cost,
        /// Whether the vendor reported an error.
        is_error: bool,
        /// The final message.
        summary: Untrusted<String>,
    },
    /// The vendor's rate-limit event.
    RateLimit(PlanUsage),
    /// Sign-in failed; parsed from the result event, not the exit code.
    AuthFailed {
        /// The vendor's message.
        message: Untrusted<String>,
    },
    /// A line that did not parse.
    Malformed {
        /// Its length.
        line_len: u32,
    },
    /// The session's process exited.
    Exited {
        /// Its exit code.
        code: Option<i32>,
    },
}

/// To a worker session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkerCmd {
    /// Spawn a session, pooled or for one job.
    Start(Box<SessionSpec>),
    /// Give a started session its job: the first user turn.
    Bind(JobBinding),
    /// One more user turn.
    Prompt {
        /// The text.
        text: String,
    },
    /// Interrupt the turn in progress.
    Interrupt,
    /// Kill the session's Job Object.
    Cancel,
    /// End the session and start a fresh one in its place.
    Recycle,
    /// Remove the job's worktree.
    RemoveWorktree,
    /// Restore the job's files to a snapshot.
    RestoreSnapshot {
        /// The snapshot.
        snapshot_ref: String,
    },
}

/// A hook request from a worker session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookRequest {
    /// Before a tool runs; the core answers with a [`HookDecision`].
    PreToolUse {
        /// The request, for the answer.
        request: u64,
        /// The tool.
        tool_name: String,
        /// Its input.
        tool_input: Untrusted<Value>,
        /// The vendor's id for the use.
        tool_use_id: String,
        /// Added by the runtime: each path argument canonicalised and classified.
        paths: Vec<PathFact>,
        /// Added by the runtime: one per git command in a shell command line.
        git: Vec<GitFact>,
        /// Added by the runtime: one per URL in the call (`WebFetch`, shell arguments).
        urls: Vec<UrlFact>,
    },
    /// The CLI asks permission; the core answers with a [`PermissionDecision`].
    PermissionRequest {
        /// The request, for the answer.
        request: u64,
        /// The tool.
        tool_name: String,
        /// Its input.
        tool_input: Untrusted<Value>,
        /// The vendor's id for the use.
        tool_use_id: String,
    },
    /// A notification from the CLI.
    Notification {
        /// Its kind.
        kind: String,
        /// Its message.
        message: Untrusted<String>,
    },
    /// The CLI denied a tool.
    PermissionDenied {
        /// The tool.
        tool_name: String,
    },
    /// A tool failed.
    PostToolUseFailure {
        /// The tool.
        tool_name: String,
        /// The error.
        error: Untrusted<String>,
    },
    /// A subagent started.
    SubagentStart,
    /// A subagent stopped.
    SubagentStop,
    /// The session stopped.
    Stop {
        /// Its last message.
        last_message: Option<Untrusted<String>>,
    },
    /// The session failed to stop cleanly.
    StopFailure {
        /// The error.
        error: Untrusted<String>,
    },
}

/// The core's answer to a gating hook request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookReply {
    /// The answer to a `PreToolUse`.
    PreToolUse {
        /// The request.
        request: u64,
        /// The decision.
        decision: HookDecision,
    },
    /// The answer to a `PermissionRequest`.
    PermissionRequest {
        /// The request.
        request: u64,
        /// The decision.
        decision: PermissionDecision,
    },
}

/// The tier guard's answer before a tool runs. Never `Allow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    /// No decision: the CLI's own rules apply.
    None,
    /// Ask the user.
    Ask {
        /// Why.
        reason: String,
    },
    /// Refuse.
    Deny {
        /// Why.
        reason: String,
    },
}

/// The answer to a permission request. `Allow` only from a `UserInput` approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Allowed by the user.
    Allow {
        /// The approval that allowed it.
        approval: ApprovalId,
    },
    /// Refused.
    Deny {
        /// Why.
        reason: String,
    },
}

/// Everything a driver needs to spawn a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionSpec {
    /// The session.
    pub session: WorkerSessionId,
    /// Its pool, for a warm pooled session.
    pub pool: Option<PoolClass>,
    /// The job kind it serves.
    pub kind: JobKind,
    /// Its route.
    pub route: Route,
    /// The model name or full id.
    pub model: String,
    /// The reasoning effort, where the CLI takes one.
    pub effort: Option<Effort>,
    /// The working folder.
    pub cwd: String,
    /// A worktree to create for it.
    pub worktree: Option<WorktreeSpec>,
    /// A snapshot to restore before it starts.
    pub snapshot_ref: Option<String>,
    /// Text appended to the system prompt.
    pub append_system_prompt: String,
    /// Eileen's toolsets it gets.
    pub toolsets: Vec<Toolset>,
    /// The `--allowedTools` list.
    pub allowed_tools: Vec<String>,
    /// Approved third-party MCP servers.
    pub extra_mcp_servers: Vec<ApprovedMcpServer>,
    /// `--tools`: the kind's built-in tools; nothing else exists in the session.
    pub builtin_tools: Vec<String>,
    /// `--disallowedTools`: the subagent tools from the harness's flag constant.
    pub disallowed_tools: Vec<String>,
    /// Plugin folders, the session's learned-skill snapshot first.
    pub plugin_dirs: Vec<String>,
    /// Re-hashed before spawn and checked against `system/init`.
    pub snapshot: Option<SnapshotManifest>,
    /// A session to resume.
    pub resume_session: Option<String>,
    /// Whether Claude in Chrome is on for it.
    pub chrome: bool,
    /// How its environment is built (section 2.12).
    pub env: EnvPolicy,
    /// Held for build-heavy coding jobs; carries the memory-scaled cargo job count.
    pub build_slot: Option<BuildSlotTicket>,
    /// `JOB_OBJECT_LIMIT_JOB_MEMORY` when `memory.worker_job_limit_mb` is set.
    pub memory_limit_mb: Option<u32>,
    /// Correlation.
    pub trace: TraceCtx,
    /// Where its hooks post.
    pub hook_endpoint: HookEndpoint,
    /// Pipe name and per-session token for the generated MCP config.
    pub shim: ShimEndpoint,
}

/// A started session's job: the first user turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct JobBinding {
    /// The job.
    pub job: JobId,
    /// Its kind.
    pub kind: JobKind,
    /// The task.
    pub task: String,
    /// A learned skill to follow.
    pub skill: Option<SkillRef>,
    /// The skill's text, at most 4 KB, when the snapshot lacks it.
    pub inline_skill: Option<String>,
    /// The facts block, at most about 400 tokens, scoped to the repository and app.
    pub facts: Vec<String>,
    /// Correlation.
    pub trace: TraceCtx,
}

/// A CLI's reasoning effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// `--effort low`.
    Low,
    /// `--effort medium`.
    Medium,
    /// `--effort high`.
    High,
}

/// A git worktree for a coding job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorktreeSpec {
    /// The repository.
    pub repo: Personal<String>,
    /// The worktree's folder.
    pub path: Personal<String>,
    /// The new branch.
    pub branch: String,
    /// The commit or branch it starts from.
    pub base: String,
}

/// A third-party MCP server the user approved for an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ApprovedMcpServer {
    /// The app.
    pub app: String,
    /// Its name in the session's MCP config.
    pub name: String,
    /// The command, inside `%LOCALAPPDATA%\eileen\mcp\<app>\`.
    pub command: Personal<String>,
    /// Its arguments.
    pub args: Vec<String>,
}

/// How a worker's environment is built (section 2.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnvPolicy {
    /// The user's environment minus the worker denylist.
    WorkerDenylist,
    /// Only the variables on the constructed allowlist.
    ConstructedAllowlist,
}

/// A build slot a build-heavy coding job holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct BuildSlotTicket {
    /// The slot, 0 or 1.
    pub slot: u8,
    /// The memory-scaled `CARGO_BUILD_JOBS`.
    pub cargo_jobs: u8,
}

/// Where a session's hooks post: the runtime's loopback endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HookEndpoint {
    /// The loopback port.
    pub port: u16,
    /// The session's path on it.
    pub path: String,
    /// The session's token.
    pub token: SessionToken,
}

/// Where a session's shim connects: the dispatcher pipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ShimEndpoint {
    /// The pipe's name.
    pub pipe: String,
    /// The session's token.
    pub token: SessionToken,
}

/// A per-session token. Minted by the runtime from OS randomness (the core reads none), checked
/// by the dispatcher and the hook endpoint (WP2), which own the verification. `Debug` does not
/// show it, recordings replace it, and the text is reachable only through [`Self::expose`].
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct SessionToken(String);

impl SessionToken {
    /// Wraps a token the runtime minted.
    #[must_use]
    pub const fn new(token: String) -> Self {
        Self(token)
    }

    /// The token's text, for the few places that must send or check it: the generated MCP
    /// config, the hook URL and the checkers. Never log it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Compares every byte whatever the first difference, so `==` on a token (the derived
/// comparisons of the types that hold one) does not end early at a mismatch. The checkers
/// still own verification and may use a vetted constant-time comparison of their own.
impl PartialEq for SessionToken {
    fn eq(&self, other: &Self) -> bool {
        let (ours, theirs) = (self.0.as_bytes(), other.0.as_bytes());
        let differences = ours.iter().zip(theirs).fold(0_u8, |acc, (a, b)| acc | (a ^ b));
        ours.len() == theirs.len() && differences == 0
    }
}

impl Eq for SessionToken {}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken([hidden])")
    }
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<BrainSlot>();
    r.add::<BrainEvent>();
    r.add::<BrainCmd>();
    r.add::<WorkerEvent>();
    r.add::<WorkerCmd>();
    r.add::<HookRequest>();
    r.add::<HookReply>();
    r.add::<HookDecision>();
    r.add::<PermissionDecision>();
    r.add::<SessionSpec>();
    r.add::<JobBinding>();
    r.add::<Effort>();
    r.add::<WorktreeSpec>();
    r.add::<ApprovedMcpServer>();
    r.add::<EnvPolicy>();
    r.add::<BuildSlotTicket>();
    r.add::<HookEndpoint>();
    r.add::<ShimEndpoint>();
    r.add::<SessionToken>();
}

#[cfg(test)]
mod tests {
    use super::SessionToken;

    #[test]
    fn a_token_compares_by_its_whole_text_and_never_shows_it() {
        let token = SessionToken::new("c0ffee-01".to_owned());
        assert_eq!(token, SessionToken::new("c0ffee-01".to_owned()), "the same text");
        assert_ne!(token, SessionToken::new("c0ffee-02".to_owned()), "one byte differs");
        assert_ne!(token, SessionToken::new("c0ffee-0".to_owned()), "a prefix");
        assert_ne!(token, SessionToken::new("c0ffee-011".to_owned()), "a longer token");
        assert_eq!(token.expose(), "c0ffee-01", "the deliberate accessor");
        assert_eq!(format!("{token:?}"), "SessionToken([hidden])", "Debug hides it");
        assert_eq!(serde_json::to_value(&token).unwrap(), "c0ffee-01", "the wire form");
    }
}

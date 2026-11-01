//! The typed stream-json parser (BUILD.md sections 2.5, 3.3.8 and 6.5). [`parse`] turns one line
//! that `claude -p --output-format stream-json` writes into one [`Line`], and two pure mappings,
//! [`worker_events`] and [`brain_events`], turn a line into the contracts' events.
//!
//! The shapes are the ones WP3's approval contract test recorded from CLI 2.1.304
//! (`docs/research/fixtures/wp3-approval/`). The rate-limit event and partial messages are not in
//! those fixtures; their shapes come from the Agent SDK's message parser and the
//! subscription-harnesses report, until the first scripts recorded on the owner's PC confirm them.
//!
//! - A line longer than [`MAX_LINE_BYTES`] is [`Line::Malformed`] before any parsing, and so is
//!   one that is not a JSON object with a string `type`, or a known type without the fields it
//!   needs. A `type`, or a `system` subtype, this parser does not know is [`Line::Unknown`].
//! - Model text, tool results and the vendor's messages are `Untrusted`. A tool's input is
//!   redacted with the [`Redactor`] before anything else sees it and capped at
//!   [`MAX_TOOL_INPUT_BYTES`]; a `Skill` call becomes [`Block::SkillInvoked`].
//! - Assistant text is split into text and narration: each line that starts with
//!   [`NARRATION_PREFIX`] is one narration line.
//! - Denials come from the stream, because the CLI's denial hooks never fired (section 2.5):
//!   [`Line::denials`] reads the errored `tool_result`, `result.permission_denials` and
//!   `system/permission_denied`, and counts an `approve` call that errored, timed out or lost its
//!   server as a denial, though the CLI reports it as a tool error.
//! - A failed sign-in is read from the `result` event, never from the exit code.
//! - The CLI writes `system/init` again at the start of every user turn, so the mappings give an
//!   `Init` or a `Ready` for each one; the driver passes on the first and checks the rest.
//! - The stdio control protocol's lines (`control_request`, `control_response`,
//!   `control_cancel_request`) are [`Line::Unknown`] here; their parser is the `control_message`
//!   half of this work, in a pull request of its own.

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::{Personal, TurnId, Untrusted, WallTime};
use eileen_contracts::jobs::{Cost, PlanUsage, Route};
use eileen_trace::Redactor;
use serde_json::Value;

/// The longest line parsed. A longer one is [`Line::Malformed`] unread. It is large because a
/// tool result can carry a screenshot as a base64 image block.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// The most a tool's input keeps once redacted (section 3.3.3): its compact JSON text, or the
/// first bytes of that text as a JSON string when the whole is longer.
pub const MAX_TOOL_INPUT_BYTES: usize = 2048;

/// The most an unknown `type` or `subtype` keeps of its name.
pub const MAX_KIND_BYTES: usize = 64;

/// What starts a narration line in assistant text (section 2.5, "Narration").
pub const NARRATION_PREFIX: &str = "> ";

/// The built-in tool whose call loads a skill.
pub const SKILL_TOOL: &str = "Skill";

/// Texts of a failed sign-in in an errored `result`, as the subscription-harnesses report saw them
/// change by CLI version ("Not logged in" on 2.1.175, "Failed to authenticate" on 2.1.294), with
/// the API's own error type. An errored result with HTTP status 401 is one too.
const AUTH_FAILURE_TEXTS: [&str; 4] =
    ["Not logged in", "Failed to authenticate", "Please run /login", "authentication_error"];

/// One stream-json line, typed.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// `system/init`: what the session has, for feature detection.
    Init(Box<Init>),
    /// An `assistant` message's blocks, in order.
    Assistant(Vec<Block>),
    /// The tool results a `user` line carries.
    ToolResults(Vec<ToolResult>),
    /// Text from a partial message (`stream_event`, a `content_block_delta` of `text_delta`).
    TextDelta(Untrusted<String>),
    /// Any other partial message: message and block starts and stops, tool input and thinking.
    Partial,
    /// A `rate_limit_event`, for one window of the plan.
    RateLimit(PlanUsage),
    /// `system/hook_started`.
    HookStarted(Hook),
    /// `system/hook_response`.
    HookResponse(Hook, HookOutcome),
    /// `system/permission_denied`: an ask that found no route, or the mode's refusal.
    PermissionDenied(Denial),
    /// The `result` that ends a turn.
    Result(Box<TurnResult>),
    /// A `type`, or `system/<subtype>`, this parser does not know.
    Unknown {
        /// The `type`, or `system/` and the subtype, each cut to [`MAX_KIND_BYTES`].
        kind: String,
    },
    /// A line that did not parse.
    Malformed {
        /// Its length in bytes, saturating.
        line_len: u32,
        /// What was wrong, for the log.
        reason: &'static str,
    },
}

/// What `system/init` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Init {
    /// The vendor's session id.
    pub session_id: String,
    /// The resolved model id.
    pub model: String,
    /// The tools the model sees.
    pub tools: Vec<String>,
    /// The MCP servers and their status.
    pub mcp_servers: Vec<McpServer>,
    /// The plugins loaded, by name.
    pub plugins: Vec<String>,
    /// The protocol features the CLI offers, such as `interrupt_receipt_v1`.
    pub capabilities: Vec<String>,
    /// The permission mode, as the CLI names it (`manual` reports `default`).
    pub permission_mode: Option<String>,
    /// The auto-memory folder (`memory_paths.auto`); absent while auto memory is off.
    pub memory_path: Option<Personal<String>>,
    /// The CLI's version.
    pub cli_version: Option<String>,
}

impl Init {
    /// True when the MCP server `name` is listed as connected. A worker session without
    /// `eileen` connected is never bound to a job (section 2.5).
    #[must_use]
    pub fn connected(&self, name: &str) -> bool {
        self.mcp_servers.iter().any(|server| server.name == name && server.status == "connected")
    }

    /// True when the CLI offers `capability`.
    #[must_use]
    pub fn offers(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|offered| offered == capability)
    }
}

/// One MCP server in `system/init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    /// Its name in the MCP config.
    pub name: String,
    /// `connected`, `failed`, `pending` and the like.
    pub status: String,
}

/// One block of an assistant message.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Text that is not narration, its lines kept together.
    Text(Untrusted<String>),
    /// One narration line, without its prefix.
    Narration(Untrusted<String>),
    /// A tool call.
    ToolUse(ToolUse),
    /// A `Skill` call.
    SkillInvoked {
        /// The skill's name.
        name: String,
        /// Its arguments, redacted, at most [`MAX_TOOL_INPUT_BYTES`].
        args: Untrusted<String>,
    },
}

/// A tool call from the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolUse {
    /// The vendor's id for the call.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// Its input, redacted, at most [`MAX_TOOL_INPUT_BYTES`].
    pub input: Untrusted<Value>,
}

/// A tool result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// The vendor's id for the call.
    pub tool_use_id: String,
    /// Whether the CLI marked it an error.
    pub is_error: bool,
    /// Why, when the error text says a permission stopped the call.
    pub denial: Option<DenialKind>,
}

/// A tool call that did not run because something refused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    /// The vendor's id for the call, when the line names it.
    pub tool_use_id: Option<String>,
    /// The tool, when the line names it.
    pub tool_name: Option<String>,
    /// What refused it.
    pub kind: DenialKind,
}

/// What refused a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialKind {
    /// The `PreToolUse` guard denied it, or blocked it because the guard failed.
    Guard,
    /// The permission mode refused a call outside `--allowedTools`.
    Mode,
    /// The `approve` call errored, timed out or lost its server: a tool error to the CLI, which
    /// leaves it out of `permission_denials`, and a denial to Eileen.
    ApproveFailed,
    /// The `approve` reply was in neither shape the CLI accepts.
    ApproveInvalid,
    /// Listed as denied without a reason this parser tells apart, such as an `approve` deny.
    Listed,
}

/// A hook event's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    /// The CLI's id, the same in the start and the response.
    pub id: String,
    /// The hook event, such as `PreToolUse`.
    pub event: String,
    /// The hook's name, such as `PreToolUse:Bash`.
    pub name: String,
}

/// How a hook ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOutcome {
    /// It answered.
    Success,
    /// It failed: a dead port, an HTTP error, a body that is not JSON.
    Error,
    /// It timed out.
    Cancelled,
    /// An outcome this parser does not know.
    Other,
}

/// The `result` that ends a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnResult {
    /// Whether the vendor reported an error.
    pub is_error: bool,
    /// Time, turns and tokens. Input tokens count the cache writes too.
    pub cost: Cost,
    /// The context's size after the turn: the last request's input, cache and output tokens.
    pub context_tokens: u64,
    /// The final message, or the errors of a failed turn.
    pub text: Untrusted<String>,
    /// The calls listed in `permission_denials`.
    pub denials: Vec<Denial>,
    /// Whether the turn failed because the CLI is not signed in.
    pub auth_failed: bool,
}

impl Line {
    /// The denials this line reports (section 2.5): an errored `tool_result` whose text names a
    /// refusal or a failed `approve` call, a `system/permission_denied` event, or the calls a
    /// `result` lists. One refused call can appear in all three; the caller counts it once by
    /// its `tool_use_id`.
    #[must_use]
    pub fn denials(&self) -> Vec<Denial> {
        match self {
            Self::ToolResults(results) => results
                .iter()
                .filter_map(|result| {
                    result.denial.map(|kind| Denial {
                        tool_use_id: Some(result.tool_use_id.clone()),
                        tool_name: None,
                        kind,
                    })
                })
                .collect(),
            Self::PermissionDenied(denial) => vec![denial.clone()],
            Self::Result(result) => result.denials.clone(),
            Self::Init(_)
            | Self::Assistant(_)
            | Self::TextDelta(_)
            | Self::Partial
            | Self::RateLimit(_)
            | Self::HookStarted(_)
            | Self::HookResponse(..)
            | Self::Unknown { .. }
            | Self::Malformed { .. } => Vec::new(),
        }
    }
}

/// Parses one line (without or with its line ending).
#[must_use]
pub fn parse(line: &[u8], redactor: &Redactor) -> Line {
    let line_len = u32::try_from(line.len()).unwrap_or(u32::MAX);
    if line.len() > MAX_LINE_BYTES {
        return Line::Malformed { line_len, reason: "the line is longer than MAX_LINE_BYTES" };
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Line::Malformed { line_len, reason: "the line is not JSON" };
    };
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return Line::Malformed { line_len, reason: "the line has no string type" };
    };
    let parsed = match kind {
        "system" => system(&value),
        "assistant" => assistant(&value, redactor),
        "user" => user(&value),
        "stream_event" => Ok(partial(&value)),
        "rate_limit_event" => rate_limit(&value),
        "result" => result(&value),
        other => Ok(Line::Unknown { kind: cut(other, MAX_KIND_BYTES).to_owned() }),
    };
    parsed.unwrap_or_else(|reason| Line::Malformed { line_len, reason })
}

/// The worker events a line gives (the Claude column of section 3.3.8's mapping table). Partial
/// messages give none, because the whole message follows them; hook events, unknown lines and
/// denials have no worker event of their own (read denials first with
/// [`Line::denials`]). `ToolResult.observed` waits for WP1's table of observed reads.
#[must_use]
pub fn worker_events(line: Line, pid: u32) -> Vec<WorkerEvent> {
    match line {
        Line::Init(init) => {
            let Init { session_id, model, tools, mcp_servers, plugins, capabilities, .. } = *init;
            let mcp_servers = mcp_servers
                .into_iter()
                .map(|server| format!("{}:{}", server.name, server.status))
                .collect();
            vec![WorkerEvent::Init {
                session_id,
                pid,
                model,
                tools,
                mcp_servers,
                plugins,
                capabilities,
            }]
        }
        Line::Assistant(blocks) => blocks.into_iter().map(worker_event).collect(),
        Line::ToolResults(results) => results
            .into_iter()
            .map(|result| WorkerEvent::ToolResult {
                tool_use_id: result.tool_use_id,
                ok: !result.is_error,
                observed: None,
            })
            .collect(),
        Line::RateLimit(usage) => vec![WorkerEvent::RateLimit(usage)],
        Line::Result(result) => {
            let TurnResult { is_error, cost, text, auth_failed, .. } = *result;
            let mut events = Vec::with_capacity(2);
            if auth_failed {
                events.push(WorkerEvent::AuthFailed { message: text.clone() });
            }
            events.push(WorkerEvent::TurnDone { cost, is_error, summary: text });
            events
        }
        Line::Malformed { line_len, .. } => vec![WorkerEvent::Malformed { line_len }],
        Line::TextDelta(_)
        | Line::Partial
        | Line::HookStarted(_)
        | Line::HookResponse(..)
        | Line::PermissionDenied(_)
        | Line::Unknown { .. } => Vec::new(),
    }
}

/// One assistant block as a worker event.
fn worker_event(block: Block) -> WorkerEvent {
    match block {
        Block::Text(delta) => WorkerEvent::Text { delta },
        Block::Narration(line) => WorkerEvent::Narration { line },
        Block::ToolUse(ToolUse { id, name, input }) => {
            WorkerEvent::ToolUse { tool_use_id: id, name, input }
        }
        Block::SkillInvoked { name, args } => WorkerEvent::SkillInvoked { name, args },
    }
}

/// Which brain process a line came from, and the turn it last sent that process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrainAt {
    /// The process's slot.
    pub which: BrainSlot,
    /// The process's instance number.
    pub instance: u32,
    /// The turn in progress, if any.
    pub turn: Option<TurnId>,
}

/// The brain events a line gives. The brain speaks from partial messages, so text comes only
/// from [`Line::TextDelta`]; a delta or a result with no turn in progress gives nothing.
#[must_use]
pub fn brain_events(line: Line, at: BrainAt) -> Vec<BrainEvent> {
    let BrainAt { which, instance, turn } = at;
    match line {
        Line::Init(init) => {
            let Init { model, .. } = *init;
            vec![BrainEvent::Ready { which, instance, route: Route::Claude, model }]
        }
        Line::TextDelta(text) => {
            turn.map(|turn| BrainEvent::TextDelta { instance, turn, text }).into_iter().collect()
        }
        Line::Result(result) => turn
            .map(|turn| BrainEvent::TurnDone {
                instance,
                turn,
                cost: result.cost,
                context_tokens: result.context_tokens,
                is_error: result.is_error,
            })
            .into_iter()
            .collect(),
        Line::RateLimit(usage) => vec![BrainEvent::RateLimit(usage)],
        Line::Assistant(_)
        | Line::ToolResults(_)
        | Line::Partial
        | Line::HookStarted(_)
        | Line::HookResponse(..)
        | Line::PermissionDenied(_)
        | Line::Unknown { .. }
        | Line::Malformed { .. } => Vec::new(),
    }
}

/// A `system` line.
fn system(value: &Value) -> Result<Line, &'static str> {
    let subtype =
        value.get("subtype").and_then(Value::as_str).ok_or("a system line without a subtype")?;
    match subtype {
        "init" => init(value).map(|init| Line::Init(Box::new(init))),
        "hook_started" => Ok(Line::HookStarted(hook(value)?)),
        "hook_response" => Ok(Line::HookResponse(hook(value)?, outcome(value))),
        "permission_denied" => {
            let kind = match str_at(value, "decision_reason_type") {
                "hook" => DenialKind::Guard,
                "mode" => DenialKind::Mode,
                _ => DenialKind::Listed,
            };
            Ok(Line::PermissionDenied(denial(value, kind)))
        }
        other => Ok(Line::Unknown { kind: format!("system/{}", cut(other, MAX_KIND_BYTES)) }),
    }
}

/// `system/init`.
fn init(value: &Value) -> Result<Init, &'static str> {
    let mcp_servers =
        value.get("mcp_servers").and_then(Value::as_array).map_or_else(Vec::new, |servers| {
            servers
                .iter()
                .filter_map(|server| {
                    Some(McpServer {
                        name: text(server, "name")?,
                        status: text(server, "status").unwrap_or_default(),
                    })
                })
                .collect()
        });
    Ok(Init {
        session_id: text(value, "session_id").ok_or("an init without a session_id")?,
        model: text(value, "model").ok_or("an init without a model")?,
        tools: names(value.get("tools")),
        mcp_servers,
        plugins: names(value.get("plugins")),
        capabilities: names(value.get("capabilities")),
        permission_mode: text(value, "permissionMode"),
        memory_path: value
            .pointer("/memory_paths/auto")
            .and_then(Value::as_str)
            .map(|path| Personal::new(path.to_owned())),
        cli_version: text(value, "claude_code_version"),
    })
}

/// An `assistant` line.
fn assistant(value: &Value, redactor: &Redactor) -> Result<Line, &'static str> {
    let content = value
        .pointer("/message/content")
        .and_then(Value::as_array)
        .ok_or("an assistant line without a content list")?;
    let mut blocks = Vec::new();
    for block in content {
        match str_at(block, "type") {
            "text" => {
                let text =
                    block.get("text").and_then(Value::as_str).ok_or("a text block without text")?;
                split_narration(text, &mut blocks);
            }
            "tool_use" => blocks.push(tool_use(block, redactor)?),
            // Thinking, redacted thinking and server-side tool blocks carry nothing for Eileen.
            _ => {}
        }
    }
    Ok(Line::Assistant(blocks))
}

/// Splits assistant text into text and narration blocks, in order. A narration line with
/// nothing after its prefix is dropped.
fn split_narration(text: &str, blocks: &mut Vec<Block>) {
    let mut plain = String::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(said) = line.strip_prefix(NARRATION_PREFIX) {
            let said = said.trim();
            if !said.is_empty() {
                flush_text(&mut plain, blocks);
                blocks.push(Block::Narration(Untrusted::new(said.to_owned())));
            }
        } else {
            if !plain.is_empty() {
                plain.push('\n');
            }
            plain.push_str(line);
        }
    }
    flush_text(&mut plain, blocks);
}

/// Moves the text gathered so far into a block, unless it is only white space.
fn flush_text(plain: &mut String, blocks: &mut Vec<Block>) {
    if !plain.trim().is_empty() {
        blocks.push(Block::Text(Untrusted::new(std::mem::take(plain))));
    }
    plain.clear();
}

/// A `tool_use` block.
fn tool_use(block: &Value, redactor: &Redactor) -> Result<Block, &'static str> {
    let id = text(block, "id").ok_or("a tool_use without an id")?;
    let name = text(block, "name").ok_or("a tool_use without a name")?;
    let input = block.get("input");
    if name == SKILL_TOOL
        && let Some(skill) = input.and_then(|input| input.get("skill")).and_then(Value::as_str)
    {
        let args = input.and_then(|input| input.get("args")).map_or_else(String::new, |args| {
            args.as_str().map_or_else(|| args.to_string(), str::to_owned)
        });
        let args = redactor.redact(&args);
        return Ok(Block::SkillInvoked {
            name: skill.to_owned(),
            args: Untrusted::new(cut(&args, MAX_TOOL_INPUT_BYTES).to_owned()),
        });
    }
    let input = input.map_or(Value::Null, |input| redacted_input(input, redactor));
    Ok(Block::ToolUse(ToolUse { id, name, input: Untrusted::new(input) }))
}

/// A tool's input, redacted and capped: the redacted value when its compact text fits in
/// [`MAX_TOOL_INPUT_BYTES`], else the first bytes of the redacted text as a JSON string. It is
/// redacted whole before it is cut, so a cut never leaves part of a secret behind.
fn redacted_input(input: &Value, redactor: &Redactor) -> Value {
    let text = input.to_string();
    let redacted = redactor.redact(&text);
    if redacted.len() <= MAX_TOOL_INPUT_BYTES
        && let Ok(value) = serde_json::from_str::<Value>(&redacted)
        && value.to_string().len() <= MAX_TOOL_INPUT_BYTES
    {
        return value;
    }
    Value::String(cut(&redacted, MAX_TOOL_INPUT_BYTES).to_owned())
}

/// A `user` line: its tool results.
fn user(value: &Value) -> Result<Line, &'static str> {
    let content = value.pointer("/message/content").ok_or("a user line without content")?;
    let mut results = Vec::new();
    for block in content.as_array().into_iter().flatten() {
        if str_at(block, "type") == "tool_result" {
            let tool_use_id =
                text(block, "tool_use_id").ok_or("a tool_result without a tool_use_id")?;
            let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let denial =
                if is_error { denial_in(&result_text(block.get("content"))) } else { None };
            results.push(ToolResult { tool_use_id, is_error, denial });
        }
    }
    Ok(Line::ToolResults(results))
}

/// A tool result's text: the string, or the text blocks of a list joined.
fn result_text(content: Option<&Value>) -> String {
    match content.and_then(Value::as_array) {
        Some(blocks) => {
            blocks.iter().filter_map(|block| block.get("text").and_then(Value::as_str)).collect()
        }
        None => content.and_then(Value::as_str).unwrap_or_default().to_owned(),
    }
}

/// What refused a call, read from its errored result's text as CLI 2.1.304 writes it (section
/// 2.2 of the approval contract report). `None` for an ordinary tool error, and for an `approve`
/// deny, whose text is Eileen's own sentence; the `result` lists that one.
fn denial_in(text: &str) -> Option<DenialKind> {
    let text = text.trim_start();
    if text.starts_with("PreToolUse:") && text.contains(" hook error: ") {
        Some(DenialKind::Guard)
    } else if text.starts_with("<tool_use_error>Error calling tool (") {
        Some(DenialKind::ApproveFailed)
    } else if text.starts_with("The permission prompt tool returned an invalid permission result") {
        Some(DenialKind::ApproveInvalid)
    } else if text.starts_with("Permission to use ") && text.contains(" has been denied") {
        Some(DenialKind::Mode)
    } else {
        None
    }
}

/// A partial message (`stream_event`).
fn partial(value: &Value) -> Line {
    let is =
        |pointer: &str, want: &str| value.pointer(pointer).and_then(Value::as_str) == Some(want);
    if is("/event/type", "content_block_delta")
        && is("/event/delta/type", "text_delta")
        && let Some(text) = value.pointer("/event/delta/text").and_then(Value::as_str)
    {
        return Line::TextDelta(Untrusted::new(text.to_owned()));
    }
    Line::Partial
}

/// A `rate_limit_event`: `rate_limit_info` with `status`, `rateLimitType`, `utilization` (a
/// fraction) and `resetsAt` (Unix seconds), as the Agent SDK's parser reads it.
fn rate_limit(value: &Value) -> Result<Line, &'static str> {
    let info = value.get("rate_limit_info").ok_or("a rate_limit_event without rate_limit_info")?;
    let pct = info
        .get("utilization")
        .and_then(Value::as_f64)
        .and_then(percent)
        .or_else(|| (str_at(info, "status") == "rejected").then_some(100));
    let resets_at = info
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(|seconds| seconds.checked_mul(1_000_000))
        .map(WallTime);
    let mut usage = PlanUsage { resets_at, ..PlanUsage::default() };
    match str_at(info, "rateLimitType") {
        "five_hour" => usage.five_hour_pct = pct,
        "seven_day" | "seven_day_opus" | "seven_day_sonnet" => usage.seven_day_pct = pct,
        _ => {}
    }
    Ok(Line::RateLimit(usage))
}

/// A fraction of the plan as a whole percent, rounded half up and held to 0 to 100.
fn percent(fraction: f64) -> Option<u8> {
    if !fraction.is_finite() {
        return None;
    }
    let pct = fraction * 100.0;
    Some((0..=100_u8).find(|&whole| f64::from(whole) + 0.5 > pct).unwrap_or(100))
}

/// A `result` line.
fn result(value: &Value) -> Result<Line, &'static str> {
    let is_error =
        value.get("is_error").and_then(Value::as_bool).ok_or("a result without is_error")?;
    let usage = value.get("usage");
    let cost = Cost {
        duration_ms: count(Some(value), "duration_ms"),
        api_ms: count(Some(value), "duration_api_ms"),
        model_turns: u32::try_from(count(Some(value), "num_turns")).unwrap_or(u32::MAX),
        tokens_in: count(usage, "input_tokens")
            .saturating_add(count(usage, "cache_creation_input_tokens")),
        tokens_out: count(usage, "output_tokens"),
        tokens_cache_read: count(usage, "cache_read_input_tokens"),
    };
    let text = match value.get("result").and_then(Value::as_str) {
        Some(text) => text.to_owned(),
        None => names(value.get("errors")).join("; "),
    };
    let unauthorised = value.get("api_error_status").and_then(Value::as_u64) == Some(401);
    let auth_failed =
        is_error && (unauthorised || AUTH_FAILURE_TEXTS.iter().any(|sign| text.contains(sign)));
    let denials =
        value.get("permission_denials").and_then(Value::as_array).map_or_else(Vec::new, |list| {
            list.iter().map(|entry| denial(entry, DenialKind::Listed)).collect()
        });
    Ok(Line::Result(Box::new(TurnResult {
        is_error,
        cost,
        context_tokens: context_tokens(usage),
        text: Untrusted::new(text),
        denials,
        auth_failed,
    })))
}

/// The context's size after a turn: the last request's tokens (`usage.iterations`), or the
/// turn's totals when the CLI lists no requests.
fn context_tokens(usage: Option<&Value>) -> u64 {
    let last = usage
        .and_then(|usage| usage.get("iterations"))
        .and_then(Value::as_array)
        .and_then(|all| all.last());
    let source = last.or(usage);
    ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens"]
        .iter()
        .map(|key| count(source, key))
        .fold(0, u64::saturating_add)
}

/// A hook event's identity.
fn hook(value: &Value) -> Result<Hook, &'static str> {
    Ok(Hook {
        id: text(value, "hook_id").ok_or("a hook event without a hook_id")?,
        event: text(value, "hook_event").unwrap_or_default(),
        name: text(value, "hook_name").unwrap_or_default(),
    })
}

/// How a hook ended.
fn outcome(value: &Value) -> HookOutcome {
    match str_at(value, "outcome") {
        "success" => HookOutcome::Success,
        "error" => HookOutcome::Error,
        "cancelled" => HookOutcome::Cancelled,
        _ => HookOutcome::Other,
    }
}

/// A denial from an object that names the call.
fn denial(value: &Value, kind: DenialKind) -> Denial {
    Denial { tool_use_id: text(value, "tool_use_id"), tool_name: text(value, "tool_name"), kind }
}

/// The string at `key`, owned.
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// The string at `key`, or the empty string.
fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The number at `key`, or 0.
fn count(value: Option<&Value>, key: &str) -> u64 {
    value.and_then(|value| value.get(key)).and_then(Value::as_u64).unwrap_or(0)
}

/// The names in a list of strings, or of objects with a `name`.
fn names(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array).map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|item| item.as_str().or_else(|| item.get("name")?.as_str()))
            .map(str::to_owned)
            .collect()
    })
}

/// The longest start of `text` that is at most `max` bytes and ends on a character boundary.
fn cut(text: &str, max: usize) -> &str {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;

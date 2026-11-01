//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `stream_json_line` and `control_message`, lines from `claude -p`. The seed corpora under
//! `fuzz/corpus/<target>/` replay through these functions as ordinary tests
//! (`it::fuzz_corpus`). Each returns `Ok` for every input that keeps the invariants it guards and
//! `Err` naming the broken one; it never panics, because the harness runs in the core.

use std::borrow::Cow;
use std::sync::Arc;

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::{TurnId, WallTime};
use eileen_trace::Redactor;
use serde_json::Value;

use crate::stream::control::{self, INTERRUPT_CAPABILITY, MAX_REQUEST_ID_BYTES, Message, Requests};
use crate::stream::{
    self, Block, BrainAt, Init, Line, MAX_LINE_BYTES, MAX_SKILL_NAME_BYTES, MAX_TOOL_INPUT_BYTES,
    NARRATION_PREFIX, PlanWindows,
};

/// The process id the worker mapping is given.
const PID: u32 = 4242;
/// The instance the brain mapping is given.
const INSTANCE: u32 = 7;
/// A literal registered with the redactor, as a session's hook token is: no tool input, skill
/// name, skill arguments, `can_use_tool` reason or `control_response` error may keep it. The
/// seeds `hand-assistant-secret`, `hand-reason-secret` and `hand-error-secret` hold it.
const SECRET: &str = "fuzz-secret-7c1e";
/// The marks of the token kinds a cut cannot make: a prefix and a run of token bytes, with no
/// rule on what ends it. A cut text that the redactor would mark again kept one of these.
const TOKEN_MARKS: [&str; 3] =
    ["[redacted:github_token]", "[redacted:api_key]", "[redacted:slack_token]"];

/// One stream-json line from `claude -p`: the parser's typed line, its denials and both
/// mappings must keep the line cap, the caps on tool inputs and skill calls, the redaction of a
/// registered secret and of every token the redactor finds, and the narration split.
pub fn stream_json_line(data: &[u8]) -> Result<(), &'static str> {
    let line_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let redactor = Arc::new(Redactor::new());
    let _secret = redactor.add_literal(SECRET).map_err(|_| "the redactor refused the secret")?;
    let line = stream::parse(data, &redactor);
    let malformed = matches!(line, Line::Malformed { line_len: len, .. } if len == line_len);
    if !malformed && matches!(line, Line::Malformed { .. }) {
        return Err("a malformed line names the wrong length");
    }
    if !malformed && (data.len() > MAX_LINE_BYTES || serde_json::from_slice::<Value>(data).is_err())
    {
        return Err("a line over the cap, or one that is not JSON, was not malformed");
    }
    if let Line::Assistant(blocks) = &line {
        blocks.iter().try_for_each(|block| block_holds(block, &redactor))?;
    }
    if let Line::ToolResults(results) = &line
        && line.denials(|_| true).len() > results.iter().filter(|result| result.is_error).count()
    {
        return Err("a tool result that is not an error was read as a denial");
    }
    if let Line::RateLimit(limit) = &line {
        let usage = PlanWindows::default().update(*limit, WallTime(0));
        let over = |pct: Option<u8>| pct.is_some_and(|pct| pct > 100);
        if over(usage.five_hour_pct) || over(usage.seven_day_pct) {
            return Err("plan usage is over 100 percent");
        }
    }
    for event in stream::worker_events(line.clone(), PID) {
        worker_event_holds(&event, malformed, line_len, &redactor)?;
    }
    let turn = TurnId::from_parts(1, 1);
    let at = BrainAt { which: BrainSlot::Primary, instance: INSTANCE, turn: Some(turn) };
    for event in stream::brain_events(line, at) {
        brain_event_holds(&event)?;
    }
    Ok(())
}

/// One control message from `claude -p`: a parsed request or response has its id within the
/// cap, a `can_use_tool` input keeps the tool input invariants, the error text and the reason
/// keep no registered secret, the stream parser reads the line the same, an interrupt with the
/// same id parses back as that interrupt, and with one interrupt open (`eileen_1`) only a response
/// to `eileen_1` settles it, which closes it, and anything else leaves it open. The seed
/// `hand-response-eileen-1` takes the settling branch.
pub fn control_message(data: &[u8]) -> Result<(), &'static str> {
    let redactor = Arc::new(Redactor::new());
    let _secret = redactor.add_literal(SECRET).map_err(|_| "the redactor refused the secret")?;
    let Ok(message) = control::parse(data, &redactor) else {
        if matches!(stream::parse(data, &redactor), Line::Control(_)) {
            return Err("the stream parser read a line the control parser refused");
        }
        return Ok(());
    };
    let request_id = match &message {
        Message::CanUseTool(ask) => {
            input_holds(ask.input.get(), &redactor)?;
            if ask.reason.as_ref().is_some_and(|reason| reason.get().contains(SECRET)) {
                return Err("a can_use_tool reason keeps a registered secret");
            }
            &ask.request_id
        }
        Message::Response { error: Some(error), .. } if error.get().contains(SECRET) => {
            return Err("a control_response error keeps a registered secret");
        }
        Message::Interrupt { request_id }
        | Message::Request { request_id, .. }
        | Message::Response { request_id, .. }
        | Message::Cancel { request_id } => request_id,
    };
    if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID_BYTES {
        return Err("a control message parsed with no request_id, or one over the cap");
    }
    let written = control::interrupt(request_id);
    let read_back = control::parse(written.as_bytes(), &redactor);
    if !matches!(&read_back, Ok(Message::Interrupt { request_id: back }) if back == request_id) {
        return Err("the interrupt line did not parse back as the same interrupt");
    }
    if !matches!(stream::parse(data, &redactor), Line::Control(same) if same == message) {
        return Err("the stream parser read the control line differently");
    }
    settles_only_its_request(message)
}

/// With one interrupt open, `message` settles it exactly when it is a response to its id.
fn settles_only_its_request(message: Message) -> Result<(), &'static str> {
    let mut requests = Requests::default();
    let offered = Init {
        session_id: String::new(),
        model: String::new(),
        tools: Vec::new(),
        mcp_servers: Vec::new(),
        plugins: Vec::new(),
        capabilities: vec![INTERRUPT_CAPABILITY.to_owned()],
        permission_mode: None,
        memory_path: None,
        cli_version: None,
    };
    requests.interrupt(&offered).map_err(|_| "an interrupt was refused on an offering session")?;
    let Message::Response { request_id, error } = message else {
        return Ok(());
    };
    let answers_it = request_id == "eileen_1";
    match requests.settle(&request_id, error) {
        Ok(reply) if !answers_it || reply.request_id != request_id => {
            Err("a response settled a request Eileen did not send")
        }
        Ok(_) if requests.open() != 0 => Err("a settled request stayed open"),
        Err(_) if answers_it => Err("the response to the open request did not settle it"),
        Err(_) if requests.open() != 1 => Err("a response that settled nothing closed a request"),
        Ok(_) | Err(_) => Ok(()),
    }
}

/// The invariants of one assistant block.
fn block_holds(block: &Block, redactor: &Redactor) -> Result<(), &'static str> {
    match block {
        Block::Text(text) => {
            let text = text.get();
            if text.trim().is_empty() {
                return Err("a text block holds only white space");
            }
            let narration = |line: &str| {
                let line = line.strip_suffix('\r').unwrap_or(line);
                line.strip_prefix(NARRATION_PREFIX).is_some_and(|said| !said.trim().is_empty())
            };
            if text.split('\n').any(narration) {
                return Err("a text block holds a narration line");
            }
        }
        Block::Narration(line) => {
            let line = line.get();
            if line.is_empty() || line.contains('\n') || line.trim() != line {
                return Err("a narration line is empty, spans lines or is not trimmed");
            }
        }
        Block::ToolUse(call) => input_holds(call.input.get(), redactor)?,
        Block::SkillInvoked { name, args } => skill_holds(name, args.get(), redactor)?,
    }
    Ok(())
}

/// The invariants of one worker event.
fn worker_event_holds(
    event: &WorkerEvent,
    malformed: bool,
    line_len: u32,
    redactor: &Redactor,
) -> Result<(), &'static str> {
    if let WorkerEvent::Malformed { line_len: len } = event
        && (!malformed || *len != line_len)
    {
        return Err(
            "a worker event says malformed for a line that parsed, or names another length",
        );
    }
    if let WorkerEvent::ToolUse { input, .. } = event {
        input_holds(input.get(), redactor)?;
    }
    if let WorkerEvent::SkillInvoked { name, args } = event {
        skill_holds(name, args.get(), redactor)?;
    }
    Ok(())
}

/// The invariants of one brain event.
fn brain_event_holds(event: &BrainEvent) -> Result<(), &'static str> {
    let instance = match event {
        BrainEvent::Ready { instance, .. }
        | BrainEvent::TextDelta { instance, .. }
        | BrainEvent::TurnDone { instance, .. }
        | BrainEvent::Exited { instance, .. } => Some(*instance),
        BrainEvent::RateLimit(usage) => {
            let over = |pct: Option<u8>| pct.is_some_and(|pct| pct > 100);
            if over(usage.five_hour_pct) || over(usage.seven_day_pct) {
                return Err("plan usage is over 100 percent");
            }
            None
        }
    };
    if instance.is_some_and(|instance| instance != INSTANCE) {
        return Err("a brain event names another instance");
    }
    Ok(())
}

/// A tool input's compact JSON text, a cut input's quotes and escapes included, is at most
/// [`MAX_TOOL_INPUT_BYTES`] and does not hold the registered secret. Each string, key and number
/// of an input that kept its shape comes back unchanged from the redactor, so the parser left no
/// match in it; a string, which a cut input is, keeps no token a cut cannot make.
fn input_holds(input: &Value, redactor: &Redactor) -> Result<(), &'static str> {
    let text = input.to_string();
    if text.len() > MAX_TOOL_INPUT_BYTES {
        return Err("a tool input is over the cap");
    }
    if text.contains(SECRET) {
        return Err("a tool input keeps a registered secret");
    }
    match input {
        Value::String(text) => keeps_no_token(text, redactor),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            if redacted_already(input, redactor) {
                Ok(())
            } else {
                Err("a tool input keeps a match the redactor finds")
            }
        }
    }
}

/// True when every string, object key and number in `value` comes back unchanged from the
/// redactor.
fn redacted_already(value: &Value, redactor: &Redactor) -> bool {
    let unchanged = |text: &str| matches!(redactor.redact(text), Cow::Borrowed(_));
    match value {
        Value::String(text) => unchanged(text),
        Value::Number(number) => unchanged(&number.to_string()),
        Value::Array(items) => items.iter().all(|item| redacted_already(item, redactor)),
        Value::Object(map) => {
            map.iter().all(|(key, item)| unchanged(key) && redacted_already(item, redactor))
        }
        Value::Null | Value::Bool(_) => true,
    }
}

/// `text` holds no token of the [`TOKEN_MARKS`] kinds that the redactor would still mark. The
/// kinds whose rule looks past the token's end (an AWS key's exact length, a card number's
/// digits) are left out, because a cut can make one of those from text that held none.
fn keeps_no_token(text: &str, redactor: &Redactor) -> Result<(), &'static str> {
    let again = redactor.redact(text);
    if TOKEN_MARKS.iter().any(|mark| again.matches(mark).count() > text.matches(mark).count()) {
        return Err("a cut tool input or a skill call keeps a token");
    }
    Ok(())
}

/// A skill call's name and arguments keep their caps, do not hold the registered secret and
/// keep no token.
fn skill_holds(name: &str, args: &str, redactor: &Redactor) -> Result<(), &'static str> {
    if name.len() > MAX_SKILL_NAME_BYTES {
        return Err("a skill's name is over the cap");
    }
    if args.len() > MAX_TOOL_INPUT_BYTES {
        return Err("a skill's arguments are over the cap");
    }
    if name.contains(SECRET) || args.contains(SECRET) {
        return Err("a skill call keeps a registered secret");
    }
    keeps_no_token(name, redactor)?;
    keeps_no_token(args, redactor)
}

//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `stream_json_line` and `control_message`, lines from `claude -p`. The seed corpus under
//! `fuzz/corpus/stream_json_line/` replays through its function as an ordinary test
//! (`it::fuzz_corpus`); `control_message` is a stub until its parser lands. Each returns `Ok`
//! for every input that keeps the invariants it guards and `Err` naming the broken one; it never
//! panics, because the harness runs in the core.

use std::sync::Arc;

use std::borrow::Cow;

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::{TurnId, WallTime};
use eileen_trace::Redactor;
use serde_json::Value;

use crate::stream::{
    self, Block, BrainAt, Line, MAX_LINE_BYTES, MAX_SKILL_NAME_BYTES, MAX_TOOL_INPUT_BYTES,
    NARRATION_PREFIX, PlanWindows,
};

/// The process id the worker mapping is given.
const PID: u32 = 4242;
/// The instance the brain mapping is given.
const INSTANCE: u32 = 7;
/// A literal registered with the redactor, as a session's hook token is: no tool input, skill
/// name or skill arguments may keep it. The seed `hand-assistant-secret` holds it.
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

/// One control message from `claude -p`. The stub accepts every input until the
/// `control_message` half of the parser lands in its own pull request.
#[expect(
    clippy::missing_const_for_fn,
    clippy::unnecessary_wraps,
    reason = "a stub until the control protocol's parser, whose checks can fail"
)]
pub fn control_message(_data: &[u8]) -> Result<(), &'static str> {
    Ok(())
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
        input if redacted_already(input, redactor) => Ok(()),
        _ => Err("a tool input keeps a match the redactor finds"),
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

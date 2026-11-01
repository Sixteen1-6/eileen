//! The entry points of the fuzz targets this crate owns (BUILD.md section 4.7):
//! `stream_json_line` and `control_message`, lines from `claude -p`. The seed corpus under
//! `fuzz/corpus/stream_json_line/` replays through its function as an ordinary test
//! (`it::fuzz_corpus`); `control_message` is a stub until its parser lands. Each returns `Ok`
//! for every input that keeps the invariants it guards and `Err` naming the broken one; it never
//! panics, because the harness runs in the core.

use eileen_contracts::harness::{BrainEvent, BrainSlot, WorkerEvent};
use eileen_contracts::ids::TurnId;
use eileen_trace::Redactor;
use serde_json::Value;

use crate::stream::{
    self, Block, BrainAt, Line, MAX_LINE_BYTES, MAX_TOOL_INPUT_BYTES, NARRATION_PREFIX,
};

/// The process id the worker mapping is given.
const PID: u32 = 4242;
/// The instance the brain mapping is given.
const INSTANCE: u32 = 7;

/// One stream-json line from `claude -p`: the parser's typed line, its denials and both
/// mappings must keep the line cap, the tool input cap and the narration split.
pub fn stream_json_line(data: &[u8]) -> Result<(), &'static str> {
    let line_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let line = stream::parse(data, &Redactor::new());
    let malformed = matches!(line, Line::Malformed { line_len: len, .. } if len == line_len);
    if !malformed && matches!(line, Line::Malformed { .. }) {
        return Err("a malformed line names the wrong length");
    }
    if !malformed && (data.len() > MAX_LINE_BYTES || serde_json::from_slice::<Value>(data).is_err())
    {
        return Err("a line over the cap, or one that is not JSON, was not malformed");
    }
    if let Line::Assistant(blocks) = &line {
        blocks.iter().try_for_each(block_holds)?;
    }
    if let Line::ToolResults(results) = &line
        && line.denials().len() > results.iter().filter(|result| result.is_error).count()
    {
        return Err("a tool result that is not an error was read as a denial");
    }
    for event in stream::worker_events(line.clone(), PID) {
        worker_event_holds(&event, malformed, line_len)?;
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
fn block_holds(block: &Block) -> Result<(), &'static str> {
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
        Block::ToolUse(call) => {
            if !input_fits(call.input.get()) {
                return Err("a tool input is over the cap");
            }
        }
        Block::SkillInvoked { args, .. } => {
            if args.get().len() > MAX_TOOL_INPUT_BYTES {
                return Err("a skill's arguments are over the cap");
            }
        }
    }
    Ok(())
}

/// The invariants of one worker event.
fn worker_event_holds(
    event: &WorkerEvent,
    malformed: bool,
    line_len: u32,
) -> Result<(), &'static str> {
    if let WorkerEvent::Malformed { line_len: len } = event
        && (!malformed || *len != line_len)
    {
        return Err(
            "a worker event says malformed for a line that parsed, or names another length",
        );
    }
    if let WorkerEvent::ToolUse { input, .. } = event
        && !input_fits(input.get())
    {
        return Err("a worker tool input is over the cap");
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

/// True when a redacted tool input keeps the cap: a JSON string of at most
/// [`MAX_TOOL_INPUT_BYTES`], or any other value whose compact text is no longer.
fn input_fits(input: &Value) -> bool {
    if let Value::String(text) = input {
        text.len() <= MAX_TOOL_INPUT_BYTES
    } else {
        input.to_string().len() <= MAX_TOOL_INPUT_BYTES
    }
}

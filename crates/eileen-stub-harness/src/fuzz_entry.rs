//! The entry point of the `stub_script` fuzz target, which this crate owns: section 3.3.16 asks
//! a fuzz target of every parser of its formats, and the stub parses two, a script
//! (`StubScriptLine` lines) and the stdin lines a driver writes to it. The seed corpus under
//! `fuzz/corpus/stub_script/` replays through this function as an ordinary test
//! (`it::fuzz_corpus`). It returns `Ok` for every input that keeps the invariants it guards and
//! `Err` naming the broken one; it never panics.

use std::io::Cursor;
use std::sync::mpsc;

use eileen_contracts::record::StubScriptLine;
use serde_json::Value;

use crate::play::{self, Input, MAX_REQUEST_ID_BYTES};
use crate::script::load;

/// The bytes as a script and as stdin. As a script (lossy UTF-8): `load` never panics; a
/// loaded line plays as one stdout line; an event's kept text is the value it loaded as; a
/// crash code fits an exit code; and every line serialises and loads again to the same line.
/// As stdin: the reader classifies every line without panicking, stops at the first broken
/// one, and a control request it passes on carries a `request_id` of 1 to 128 bytes.
pub fn stub_script(data: &[u8]) -> Result<(), &'static str> {
    script_holds(&String::from_utf8_lossy(data))?;
    stdin_holds(data)
}

fn script_holds(text: &str) -> Result<(), &'static str> {
    let Ok(script) = load(text) else { return Ok(()) };
    for (_, step, event) in script.steps() {
        match (step, event) {
            (StubScriptLine::Event { json }, Some(text)) => {
                if text.contains(['\n', '\r']) {
                    return Err("an event would write a line break");
                }
                if serde_json::from_str::<Value>(text).ok().as_ref() != Some(json) {
                    return Err("an event's kept text is not the value it loaded as");
                }
            }
            (StubScriptLine::Event { .. }, None) => return Err("an event's text was not kept"),
            (StubScriptLine::Malformed { bytes }, _) if bytes.contains(['\n', '\r']) => {
                return Err("malformed bytes would write a line break");
            }
            (StubScriptLine::Crash { code }, _) if u8::try_from(*code).is_err() => {
                return Err("a crash code outside 0 to 255 loaded");
            }
            _ => {}
        }
        round_trips(step)?;
    }
    Ok(())
}

/// A loaded line serialises, and that text loads again as one line equal to it.
fn round_trips(step: &StubScriptLine) -> Result<(), &'static str> {
    let text = serde_json::to_string(step).map_err(|_| "a loaded line does not serialise")?;
    let again = load(&text).map_err(|_| "a serialised line does not load again")?;
    let [(_, again)] = again.lines() else {
        return Err("a serialised line loads as more or fewer than one line");
    };
    let (Ok(before), Ok(after)) = (serde_json::to_value(step), serde_json::to_value(again)) else {
        return Err("a loaded line does not serialise to a value");
    };
    if same(&before, &after) { Ok(()) } else { Err("a line changed through a round trip") }
}

/// Equal JSON values, except that two floats may differ in their last bits: `serde_json` without
/// its `float_roundtrip` feature parses a float to the nearest value only on a best effort, so
/// printing and parsing one again can move it by a few units in the last place.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x == y
                || x.as_f64().zip(y.as_f64()).is_some_and(|(x, y)| {
                    (x - y).abs()
                        <= f64::EPSILON * 8.0 * x.abs().max(y.abs()).max(f64::MIN_POSITIVE)
                })
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(key, x)| y.get(key).is_some_and(|y| same(x, y)))
        }
        _ => a == b,
    }
}

fn stdin_holds(data: &[u8]) -> Result<(), &'static str> {
    let lines = data.split(|byte| *byte == b'\n').count();
    // Room for every line, so the reader never waits for a player.
    let (queue, input) = mpsc::sync_channel(lines);
    play::read_input(Cursor::new(data), &queue);
    drop(queue);
    let read: Vec<Input> = input.iter().collect();
    for (index, one) in read.iter().enumerate() {
        match one {
            Input::Control { request_id, .. }
                if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID_BYTES =>
            {
                return Err("a control request passed on with a request_id outside 1 to 128 bytes");
            }
            Input::Broken(_) if index.saturating_add(1) != read.len() => {
                return Err("the reader read on after a broken line");
            }
            Input::User | Input::Control { .. } | Input::Other | Input::Broken(_) => {}
        }
    }
    Ok(())
}

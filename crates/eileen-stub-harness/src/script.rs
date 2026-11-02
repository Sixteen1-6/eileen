//! Loading a script: one `StubScriptLine` per line, every line checked before anything plays.

use eileen_contracts::record::StubScriptLine;

/// A script, loaded and checked, with each line's number in its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    lines: Vec<(usize, StubScriptLine)>,
    /// Beside each line, an `Event`'s `json` as the script's own text, which the stub writes
    /// unchanged: re-serialising the parsed value would sort its keys and could change a
    /// float's last digit.
    events: Vec<Option<Box<str>>>,
}

impl Script {
    /// The lines to play, in order, each with its 1-based line number in the file.
    #[must_use]
    pub fn lines(&self) -> &[(usize, StubScriptLine)] {
        &self.lines
    }

    /// The lines with, for an `Event`, the text of its `json` as the script wrote it.
    pub(crate) fn steps(&self) -> impl Iterator<Item = (usize, &StubScriptLine, Option<&str>)> {
        self.lines
            .iter()
            .zip(&self.events)
            .map(|((line, step), event)| (*line, step, event.as_deref()))
    }
}

/// The text of the top-level `json` member of `line`, a JSON object that has already parsed as
/// a `StubScriptLine`, so its syntax is known to be good. A scan by hand, because `serde_json`
/// gives a value's own text only through its `raw_value` feature, and that feature changes how
/// every crate built beside this one parses an object holding its private marker key. The
/// `stub_script` fuzz target checks the text found parses to the value `load` read.
fn json_member(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut at = skip_space(bytes, 0);
    if bytes.get(at) != Some(&b'{') {
        return None;
    }
    at = at.checked_add(1)?;
    loop {
        let key_start = skip_space(bytes, at);
        let key_end = skip_string(bytes, key_start)?;
        // A key may be written with escapes; serde_json reads it as `StubScriptLine` did.
        let key: String = serde_json::from_str(line.get(key_start..key_end)?).ok()?;
        let colon = skip_space(bytes, key_end);
        if bytes.get(colon) != Some(&b':') {
            return None;
        }
        let start = skip_space(bytes, colon.checked_add(1)?);
        let end = skip_value(bytes, start)?;
        if key == "json" {
            return line.get(start..end);
        }
        let comma = skip_space(bytes, end);
        if bytes.get(comma) != Some(&b',') {
            return None;
        }
        at = comma.checked_add(1)?;
    }
}

/// The first index at or after `at` that is not JSON whitespace.
fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r')) {
        at = at.saturating_add(1);
    }
    at
}

/// Just past the closing quote of the string that starts at `at`.
fn skip_string(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'"') {
        return None;
    }
    let mut index = at.checked_add(1)?;
    loop {
        match bytes.get(index)? {
            b'\\' => index = index.checked_add(2)?,
            b'"' => return index.checked_add(1),
            _ => index = index.checked_add(1)?,
        }
    }
}

/// Just past the end of the value that starts at `at`.
fn skip_value(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at)? {
        b'"' => skip_string(bytes, at),
        b'{' | b'[' => {
            let mut depth = 0_usize;
            let mut index = at;
            loop {
                index = match bytes.get(index)? {
                    b'"' => skip_string(bytes, index)?,
                    b'{' | b'[' => {
                        depth = depth.checked_add(1)?;
                        index.checked_add(1)?
                    }
                    b'}' | b']' => {
                        depth = depth.checked_sub(1)?;
                        let next = index.checked_add(1)?;
                        if depth == 0 {
                            return Some(next);
                        }
                        next
                    }
                    _ => index.checked_add(1)?,
                };
            }
        }
        // A number, `true`, `false` or `null`: it runs to the next delimiter.
        _ => {
            let mut index = at;
            while bytes.get(index).is_some_and(|byte| {
                !matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            }) {
                index = index.saturating_add(1);
            }
            (index > at).then_some(index)
        }
    }
}

/// Why a script did not load.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// A line is not a `StubScriptLine`.
    #[error("line {line} is not a StubScriptLine: {source}")]
    Parse {
        /// The 1-based line number.
        line: usize,
        /// What serde said.
        source: serde_json::Error,
    },
    /// A `Crash` line's code does not fit an exit code that every OS carries.
    #[error("line {line} crashes with code {code}, which is outside 0 to 255")]
    CrashCode {
        /// The 1-based line number.
        line: usize,
        /// The code.
        code: i32,
    },
    /// A line would write a line break inside its one stdout line: `Malformed` bytes holding
    /// `\n` or `\r`, or an `Event` with a `\r` between its JSON tokens. A reader would see two
    /// lines, and the second could be stream-json the driver acts on.
    #[error("line {line} would write a line break inside one stdout line")]
    LineBreak {
        /// The 1-based line number.
        line: usize,
    },
    /// An `Event` line parsed, but the scan for its `json` text did not find it. Never seen;
    /// the `stub_script` fuzz target reports it as a broken invariant.
    #[error("line {line} is an event whose json text was not found")]
    EventText {
        /// The 1-based line number.
        line: usize,
    },
}

/// Whether `text` holds a byte that ends a line for some reader.
fn breaks_line(text: &str) -> bool {
    text.contains(['\n', '\r'])
}

/// Loads a script from its text: one JSON `StubScriptLine` per line, blank lines skipped. Every
/// line is checked first, so a script that does not load plays nothing.
pub fn load(text: &str) -> Result<Script, ScriptError> {
    let mut lines = Vec::new();
    let mut events = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        if raw.trim().is_empty() {
            continue;
        }
        let parsed: StubScriptLine =
            serde_json::from_str(raw).map_err(|source| ScriptError::Parse { line, source })?;
        let event = match &parsed {
            StubScriptLine::Crash { code } if u8::try_from(*code).is_err() => {
                return Err(ScriptError::CrashCode { line, code: *code });
            }
            StubScriptLine::Malformed { bytes } if breaks_line(bytes) => {
                return Err(ScriptError::LineBreak { line });
            }
            StubScriptLine::Event { .. } => {
                let json = json_member(raw).ok_or(ScriptError::EventText { line })?;
                if breaks_line(json) {
                    return Err(ScriptError::LineBreak { line });
                }
                Some(Box::<str>::from(json))
            }
            StubScriptLine::Crash { .. }
            | StubScriptLine::Malformed { .. }
            | StubScriptLine::Delay { .. }
            | StubScriptLine::Narration { .. }
            | StubScriptLine::AwaitUser
            | StubScriptLine::PermissionRequest { .. } => None,
        };
        lines.push((line, parsed));
        events.push(event);
    }
    Ok(Script { lines, events })
}

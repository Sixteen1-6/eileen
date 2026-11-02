//! Loading a script: one `StubScriptLine` per line, every line checked before anything plays.

use eileen_contracts::record::StubScriptLine;
use serde::Deserialize;
use serde_json::value::RawValue;

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

/// An `Event` line read again for its `json` text. Serde reads a `RawValue` only from the
/// JSON text itself, not from the buffered content an internally tagged enum such as
/// `StubScriptLine` reads first, so this is a plain struct whose other keys are ignored.
#[derive(Deserialize)]
struct EventText {
    json: Box<RawValue>,
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
        let parse = |source| ScriptError::Parse { line, source };
        let parsed: StubScriptLine = serde_json::from_str(raw).map_err(parse)?;
        let event = match &parsed {
            StubScriptLine::Crash { code } if u8::try_from(*code).is_err() => {
                return Err(ScriptError::CrashCode { line, code: *code });
            }
            StubScriptLine::Malformed { bytes } if breaks_line(bytes) => {
                return Err(ScriptError::LineBreak { line });
            }
            StubScriptLine::Event { .. } => {
                let EventText { json } = serde_json::from_str(raw).map_err(parse)?;
                if breaks_line(json.get()) {
                    return Err(ScriptError::LineBreak { line });
                }
                Some(Box::<str>::from(json.get()))
            }
            _ => None,
        };
        lines.push((line, parsed));
        events.push(event);
    }
    Ok(Script { lines, events })
}

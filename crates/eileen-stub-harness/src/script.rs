//! Loading a script: one `StubScriptLine` per line, every line checked before anything plays.

use eileen_contracts::record::StubScriptLine;

/// A script, loaded and checked, with each line's number in its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    lines: Vec<(usize, StubScriptLine)>,
}

impl Script {
    /// The lines to play, in order, each with its 1-based line number in the file.
    #[must_use]
    pub fn lines(&self) -> &[(usize, StubScriptLine)] {
        &self.lines
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
}

/// Loads a script from its text: one JSON `StubScriptLine` per line, blank lines skipped. Every
/// line is checked first, so a script that does not load plays nothing.
pub fn load(text: &str) -> Result<Script, ScriptError> {
    let mut lines = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        if raw.trim().is_empty() {
            continue;
        }
        let parsed: StubScriptLine =
            serde_json::from_str(raw).map_err(|source| ScriptError::Parse { line, source })?;
        if let StubScriptLine::Crash { code } = parsed
            && u8::try_from(code).is_err()
        {
            return Err(ScriptError::CrashCode { line, code });
        }
        lines.push((line, parsed));
    }
    Ok(Script { lines })
}

//! Entry points for the fuzz targets this crate owns (BUILD.md section 4.7): `uia_text` (UI
//! Automation strings through [`sanitize`]) and `desktop_state` (`DesktopState` snapshots).
//! The seed corpora under `fuzz/corpus/<target>/` replay through these functions as ordinary
//! tests (`it::fuzz_corpus`). Each returns `Ok` for every input that keeps the invariant it
//! guards and `Err` naming the broken one; it never panics, because every Eileen binary links
//! this crate. The test and the cargo-fuzz targets turn an `Err` into a failure.

use crate::desktop::{DesktopState, is_bidi_control, is_invisible_format, sanitize};

/// UI Automation text: the first byte picks the bound, the rest is the text, decoded lossily
/// the way the desktop server decodes a UTF-16 string it could not read whole.
pub fn uia_text(data: &[u8]) -> Result<(), &'static str> {
    let (max, text) = match data.split_first() {
        Some((first, rest)) => (usize::from(*first), String::from_utf8_lossy(rest)),
        None => (0, String::from_utf8_lossy(data)),
    };
    let out = sanitize(&text, max).into_inner();
    if out.chars().count() > max {
        return Err("sanitize kept more characters than its bound");
    }
    if !out.chars().all(|c| c == '\n' || c == '\t' || !c.is_control()) {
        return Err("sanitize kept a control character");
    }
    if out.contains(['\r', '\u{2028}', '\u{2029}']) {
        return Err("sanitize kept a line break other than \\n");
    }
    if out.chars().any(|c| is_bidi_control(c) || is_invisible_format(c)) {
        return Err("sanitize kept a bidirectional or invisible formatting character");
    }
    if sanitize(&out, max).into_inner() != out {
        return Err("sanitize is not idempotent");
    }
    Ok(())
}

/// A `DesktopState` snapshot from bytes: when it parses, it must write back and parse again to
/// an equal value.
pub fn desktop_state(data: &[u8]) -> Result<(), &'static str> {
    let Ok(state) = serde_json::from_slice::<DesktopState>(data) else { return Ok(()) };
    let value =
        serde_json::to_value(&state).map_err(|_| "a parsed DesktopState did not serialize")?;
    let again = serde_json::from_value::<DesktopState>(value)
        .map_err(|_| "a written DesktopState did not parse back")?;
    if again != state {
        return Err("a written DesktopState parsed back to a different value");
    }
    let text = serde_json::to_vec(&state).map_err(|_| "a parsed DesktopState did not write")?;
    serde_json::from_slice::<DesktopState>(&text)
        .map(drop)
        .map_err(|_| "a written DesktopState did not parse back from its text")
}

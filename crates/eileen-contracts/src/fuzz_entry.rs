//! Entry points for the fuzz targets this crate owns (BUILD.md section 4.7): `uia_text` (UI
//! Automation strings through [`sanitize`]) and `desktop_state` (`DesktopState` snapshots).
//! The seed corpora under `fuzz/corpus/<target>/` replay through these functions as ordinary
//! tests (`it::fuzz_corpus`). Each returns normally for every input and asserts only the
//! invariant it guards.

use crate::desktop::{DesktopState, sanitize};

/// UI Automation text: the first byte picks the bound, the rest is the text, decoded lossily
/// the way the desktop server decodes a UTF-16 string it could not read whole.
pub fn uia_text(data: &[u8]) {
    let (max, text) = match data.split_first() {
        Some((first, rest)) => (usize::from(*first), String::from_utf8_lossy(rest)),
        None => (0, String::from_utf8_lossy(data)),
    };
    let out = sanitize(&text, max).into_inner();
    assert!(out.chars().count() <= max, "sanitize kept more than {max} characters");
    assert!(
        out.chars().all(|c| c == '\n' || c == '\t' || !c.is_control()),
        "sanitize kept a control character"
    );
    assert!(sanitize(&out, max).into_inner() == out, "sanitize is not idempotent");
}

/// A `DesktopState` snapshot from bytes: when it parses, it must write back and parse again.
pub fn desktop_state(data: &[u8]) {
    let Ok(state) = serde_json::from_slice::<DesktopState>(data) else { return };
    let written = serde_json::to_vec(&state);
    assert!(written.is_ok(), "a parsed DesktopState did not serialize");
    if let Ok(bytes) = written {
        assert!(
            serde_json::from_slice::<DesktopState>(&bytes).is_ok(),
            "a written DesktopState did not parse back"
        );
    }
}

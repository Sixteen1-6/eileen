//! `DesktopState` (BUILD.md section 3.3.5): what is on the screen, as cheap context, in physical
//! pixels and virtual-screen coordinates.
//!
//! Provisional: WP5 pins this module in its first pull request (BUILD.md section 3.3).
//!
//! [`sanitize`] is the only way text from UI Automation enters a contract type.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{Personal, Untrusted, WallTime};
use crate::schema::Registry;

/// A snapshot of the desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopState {
    /// When it was captured.
    pub captured_at: WallTime,
    /// The monitors.
    pub monitors: Vec<Monitor>,
    /// The top-level windows, in z-order.
    pub windows: Vec<WindowInfo>,
    /// The foreground window.
    pub foreground: Option<WindowRef>,
    /// The focused element.
    pub focused_element: Option<ElementInfo>,
    /// The user's selection.
    pub selection: Option<Selection>,
    /// The text caret.
    pub caret: Option<Rect>,
    /// What the user referred to recently, at most 8, newest first.
    pub recent_references: Vec<Reference>,
    /// Whether the user is presenting, recording or in quiet hours.
    pub presentation: PresentationState,
    /// Whether another app is using the microphone.
    pub mic_in_use_elsewhere: bool,
    /// Merged in by the core from `pc.apps`.
    pub now_playing: Option<MediaState>,
    /// Fields that timed out or are stale: unknown, not empty.
    pub degraded: Vec<String>,
}

/// A change to the desktop, from the desktop server's event hooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopDelta {
    /// The desktop server's own order.
    pub seq: u64,
    /// The changes, in order.
    pub changes: Vec<DesktopChange>,
}

/// One change to the desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesktopChange {
    /// A window opened.
    WindowOpened(WindowInfo),
    /// A window changed (moved, renamed, minimized, cloaked).
    WindowChanged(WindowInfo),
    /// A window closed.
    WindowClosed {
        /// The window.
        hwnd: i64,
    },
    /// The foreground window changed.
    Foreground(Option<WindowRef>),
    /// The focused element changed.
    Focus(Option<ElementInfo>),
    /// The selection changed.
    Selection(Option<Selection>),
    /// The caret moved.
    Caret(Option<Rect>),
    /// The monitors changed.
    Monitors(Vec<Monitor>),
    /// The presentation state changed.
    Presentation(PresentationState),
    /// Another app started or stopped using the microphone.
    MicInUseElsewhere(bool),
    /// The user referred to something.
    Reference(Reference),
}

/// One monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Monitor {
    /// "screen two": its ordinal, left to right from 1.
    pub ordinal: u8,
    /// Its device path, for example `\\.\DISPLAY1`.
    pub device_path: String,
    /// Its name from the display's own data.
    pub friendly_name: String,
    /// Its bounds.
    pub rect: Rect,
    /// Its bounds without the taskbar.
    pub work_rect: Rect,
    /// Its DPI.
    pub dpi: u16,
    /// Whether it is the primary monitor.
    pub is_primary: bool,
}

/// One top-level window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[expect(clippy::struct_excessive_bools, reason = "independent facts about one window")]
pub struct WindowInfo {
    /// Its handle (an `i64`, because `HWND` is not `Send`).
    pub hwnd: i64,
    /// Its process id.
    pub pid: u32,
    /// Its process name.
    pub process: String,
    /// Its executable's file version.
    pub exe_version: Option<String>,
    /// Its title.
    pub title: Untrusted<String>,
    /// Its window class.
    pub class: String,
    /// The monitor it is mostly on.
    pub monitor_ordinal: Option<u8>,
    /// Its frame.
    pub frame_rect: Rect,
    /// Its z-order, 0 at the top.
    pub z_order: u32,
    /// Whether it is minimized.
    pub is_minimized: bool,
    /// Whether it is cloaked (on another virtual desktop, or hidden by the shell).
    pub is_cloaked: bool,
    /// Whether it is the foreground window.
    pub is_foreground: bool,
    /// Whether it is one of Eileen's own windows.
    pub is_eileen: bool,
    /// Whether it is a private browser window or a private app.
    pub is_private: bool,
    /// A browser window's page host from its address bar over UI Automation; `None` for other
    /// windows, and for a browser window whose bar could not be read. For a private window it
    /// serves only the URL rule and is never shown or stored.
    pub url_host: Option<Personal<String>>,
}

/// A window, by handle and process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct WindowRef {
    /// Its handle.
    pub hwnd: i64,
    /// Its process id.
    pub pid: u32,
}

/// One UI Automation element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ElementInfo {
    /// Its control type, for example `Edit`.
    pub control_type: String,
    /// Its name.
    pub name: Untrusted<String>,
    /// Its automation id, when the app set one.
    pub automation_id: Option<String>,
    /// Its bounds.
    pub rect: Rect,
    /// The UI Automation patterns it supports.
    pub patterns: Vec<String>,
    /// Whether it is a password field.
    pub is_password: bool,
    /// Its window.
    pub window: WindowRef,
}

/// The user's selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Selection {
    /// The text, at most 4,000 characters.
    pub text: Untrusted<String>,
    /// Where it came from.
    pub source: SelectionSource,
}

/// Where a selection's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSource {
    /// The element's UI Automation text pattern.
    TextPattern,
    /// The clipboard, only when the request refers to something copied, never from clipboard
    /// data marked for exclusion from monitoring or history, and always after the Redactor.
    ClipboardFallback,
}

/// Something the user referred to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reference {
    /// What kind of thing.
    pub kind: ReferenceKind,
    /// What it is called on screen.
    pub label: Untrusted<String>,
    /// Its window, when it has one.
    pub window: Option<WindowRef>,
    /// Its path, for a file.
    pub path: Option<Personal<String>>,
    /// When the user referred to it.
    pub at: WallTime,
}

/// The kinds of thing a reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// A window.
    Window,
    /// An element.
    Element,
    /// A file.
    File,
    /// A selection.
    Selection,
    /// Media.
    Media,
}

/// Whether the user is presenting, recording or in quiet hours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PresentationState {
    /// Nothing special.
    #[default]
    Normal,
    /// The shell reports the user busy.
    Busy,
    /// A full-screen app.
    FullScreen,
    /// Presentation mode.
    Presentation,
    /// An app of the recorder category runs.
    Recording,
    /// Quiet hours.
    QuietHours,
}

/// A rectangle in physical virtual-screen pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

/// A point in physical virtual-screen pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
}

/// How an element is found again: by its process, window and its own identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Selector {
    /// The process name.
    pub process: String,
    /// The window class.
    pub window_class: Option<String>,
    /// The element's automation id.
    pub automation_id: Option<String>,
    /// The element's control type.
    pub control_type: Option<String>,
    /// The element's name.
    pub name: Option<String>,
    /// Its ancestors, nearest first.
    #[serde(default)]
    pub ancestors: Vec<SelectorStep>,
}

/// One ancestor of a selected element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SelectorStep {
    /// Its control type.
    pub control_type: Option<String>,
    /// Its name.
    pub name: Option<String>,
    /// Its automation id.
    pub automation_id: Option<String>,
}

/// A media session (the system's media transport controls).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaState {
    /// The session's id.
    pub session: String,
    /// The app that owns it.
    pub app: String,
    /// What is playing.
    pub title: Untrusted<String>,
    /// Who made it.
    pub artist: Option<Untrusted<String>>,
    /// The position, seconds.
    pub position_s: Option<f64>,
    /// The length, seconds.
    pub duration_s: Option<f64>,
    /// Whether it is playing.
    pub playing: bool,
}

/// What kind of field text goes into, for dictation and insertion.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FieldCategory {
    /// Plain text.
    Plain,
    /// An e-mail.
    Email,
    /// A chat message.
    Chat,
    /// A document.
    Document,
    /// Source code.
    Code,
    /// A terminal.
    Terminal,
    /// A search box.
    Search,
    /// A form field.
    Form,
}

/// Turns text from UI Automation into contract text: valid UTF-8 (the `&str` already is),
/// `\r\n` and lone `\r` turned into `\n`, every other control character except `\n` and `\t`
/// removed, the bidirectional formatting characters (embeddings, overrides, isolates and the
/// direction marks) removed, and at most `max_chars` characters kept. It reads the input only
/// until the output is full, so a huge input costs no more than a short one.
#[must_use]
pub fn sanitize(text: &str, max_chars: usize) -> Untrusted<String> {
    let mut out = String::with_capacity(text.len().min(max_chars.saturating_mul(4)));
    let mut kept = 0_usize;
    let mut chars = text.chars().peekable();
    while kept < max_chars {
        let Some(c) = chars.next() else { break };
        let c = match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                '\n'
            }
            '\n' | '\t' => c,
            c if c.is_control() || is_bidi_control(c) => continue,
            c => c,
        };
        out.push(c);
        kept += 1;
    }
    Untrusted::new(out)
}

/// The bidirectional formatting characters: ALM, LRM, RLM, the embeddings and overrides
/// (U+202A to U+202E) and the isolates (U+2066 to U+2069).
const fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<DesktopState>();
    r.add::<DesktopDelta>();
    r.add::<DesktopChange>();
    r.add::<Monitor>();
    r.add::<WindowInfo>();
    r.add::<WindowRef>();
    r.add::<ElementInfo>();
    r.add::<Selection>();
    r.add::<SelectionSource>();
    r.add::<Reference>();
    r.add::<ReferenceKind>();
    r.add::<PresentationState>();
    r.add::<Rect>();
    r.add::<Point>();
    r.add::<Selector>();
    r.add::<SelectorStep>();
    r.add::<MediaState>();
    r.add::<FieldCategory>();
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::sanitize;

    #[test]
    fn sanitize_keeps_ordinary_text() {
        assert_eq!(sanitize("Save changes?", 100).get(), "Save changes?", "plain");
        assert_eq!(sanitize("Grüße, 東京 🎬", 100).get(), "Grüße, 東京 🎬", "non-ASCII");
        assert_eq!(sanitize("a\tb\nc", 100).get(), "a\tb\nc", "tabs and newlines stay");
    }

    #[test]
    fn sanitize_normalises_line_endings() {
        assert_eq!(sanitize("one\r\ntwo\rthree", 100).get(), "one\ntwo\nthree", "line endings");
        assert_eq!(sanitize("one\r\ntwo", 4).get(), "one\n", "a CRLF counts as one character");
    }

    #[test]
    fn sanitize_removes_control_and_bidi_characters() {
        let hostile =
            "pay\u{202E}lmth.exe\u{0007}\u{0000}\u{001B}[31m\u{0085}\u{200F}\u{2066}x\u{2069}";
        assert_eq!(sanitize(hostile, 100).get(), "paylmth.exe[31mx", "removed");
    }

    #[test]
    fn sanitize_bounds_the_output_in_characters() {
        assert_eq!(sanitize("東京東京", 3).get(), "東京東", "three characters");
        assert_eq!(sanitize("abc", 0).get(), "", "zero");
        let huge = "x".repeat(1_000_000);
        assert_eq!(sanitize(&huge, 4_000).get().chars().count(), 4_000, "bounded");
    }

    proptest! {
        #[test]
        fn sanitize_output_is_clean_and_bounded(text in any::<String>(), max in 0_usize..64) {
            let out = sanitize(&text, max).into_inner();
            prop_assert!(out.chars().count() <= max);
            prop_assert!(out.chars().all(|c| c == '\n' || c == '\t' || !c.is_control()));
            prop_assert!(!out.contains('\r'));
            prop_assert!(!out.chars().any(super::is_bidi_control));
            prop_assert_eq!(sanitize(&out, max).into_inner(), out, "idempotent");
        }
    }
}

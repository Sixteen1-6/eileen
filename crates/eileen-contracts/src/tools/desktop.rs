//! The desktop toolset's arguments (`desktop_state` and `desktop_look` are the brain
//! toolset's). `hwnd` is an `i64` because `HWND` is not `Send`; coordinates are physical
//! virtual-screen pixels.
//!
//! Provisional: WP5 pins this module in its first pull request (BUILD.md section 3.3).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ToolInput, input};
use crate::desktop::{FieldCategory, Point, Rect, Selector};
use crate::schema::Registry;

const MODULE: &str = "tools/desktop";

/// `desktop_windows`. Tier 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopWindows {
    /// Include minimized windows.
    #[serde(default)]
    pub include_minimized: bool,
}

/// `desktop_tree`: a window's UI Automation tree. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopTree {
    /// The window.
    pub hwnd: i64,
    /// Only elements matching this text.
    pub query: Option<String>,
    /// How deep, 12 by default.
    pub max_depth: Option<u8>,
    /// How many elements, 250 by default.
    pub max_elements: Option<u16>,
}

/// `desktop_screenshot`. Tier 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopScreenshot {
    /// What to capture.
    pub target: ShotTarget,
    /// Draw numbered marks on the elements.
    #[serde(default)]
    pub marks: bool,
    /// The longest edge, pixels.
    pub max_edge_px: Option<u32>,
}

/// What a screenshot captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShotTarget {
    /// Every monitor.
    Screen,
    /// One monitor.
    Monitor {
        /// Its ordinal.
        ordinal: u8,
    },
    /// One window.
    Window {
        /// The window.
        hwnd: i64,
    },
    /// A region.
    Region {
        /// The region.
        rect: Rect,
    },
}

/// `desktop_click_element`: invoke an element through UI Automation. Tier 1, or 2 for
/// send-like names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopClickElement {
    /// The window.
    pub hwnd: i64,
    /// The element's index from `desktop_tree`.
    pub element: u32,
    /// How to find it again if the index moved.
    pub selector: Option<Selector>,
}

/// `desktop_set_value`. Tier 1; refuses password fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopSetValue {
    /// The window.
    pub hwnd: i64,
    /// The element's index.
    pub element: u32,
    /// The value.
    pub value: String,
}

/// `desktop_type_text`. Tier 1 (2 in terminals and unknown targets); takes the foreground.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopTypeText {
    /// The window.
    pub hwnd: i64,
    /// The element to focus first.
    pub element: Option<u32>,
    /// The text.
    pub text: String,
}

/// `desktop_press_keys`, for example `ctrl+s`. Tier 1, by arguments (Enter in mail or chat 2,
/// blocked combinations 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopPressKeys {
    /// The window.
    pub hwnd: i64,
    /// The keys.
    pub keys: String,
}

/// `desktop_click`. Tier 1, by arguments; takes the foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopClick {
    /// The window.
    pub hwnd: i64,
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
    /// The button.
    pub button: MouseButton,
    /// How many clicks.
    pub count: u8,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    /// The left button.
    #[default]
    Left,
    /// The right button.
    Right,
    /// The middle button.
    Middle,
}

/// `desktop_scroll`. Tier 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopScroll {
    /// The window.
    pub hwnd: i64,
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
    /// Horizontal amount.
    pub dx: i32,
    /// Vertical amount.
    pub dy: i32,
}

/// `desktop_drag`. Tier 1; takes the foreground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopDrag {
    /// The window.
    pub hwnd: i64,
    /// Where it starts.
    pub from: Point,
    /// Where it ends.
    pub to: Point,
    /// The button.
    pub button: MouseButton,
}

/// `desktop_window`. Tier 1; `Close` is tier 1 when the app reports nothing unsaved, else 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopWindow {
    /// The window.
    pub hwnd: i64,
    /// What to do.
    pub action: WindowAction,
}

/// What `desktop_window` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    /// Bring it to the front.
    Focus,
    /// Minimize it.
    Minimize,
    /// Maximize it.
    Maximize,
    /// Restore it.
    Restore,
    /// Move and size it.
    Move {
        /// Its new frame.
        rect: Rect,
    },
    /// Snap it to part of a monitor.
    Snap {
        /// The monitor's ordinal; the window's own when left out.
        monitor: Option<u8>,
        /// The zone.
        zone: SnapZone,
        /// How much of the monitor, percent.
        fraction_pct: Option<u8>,
    },
    /// Close it (`WM_CLOSE` through `SendMessageTimeout`).
    Close,
    /// Bring it forward because the user should look (section 2.6).
    Show {
        /// Why.
        reason: ShowReason,
    },
}

/// Where a window snaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapZone {
    /// The left part.
    Left,
    /// The right part.
    Right,
    /// The top part.
    Top,
    /// The bottom part.
    Bottom,
    /// The top-left quarter.
    TopLeft,
    /// The top-right quarter.
    TopRight,
    /// The bottom-left quarter.
    BottomLeft,
    /// The bottom-right quarter.
    BottomRight,
    /// Centred.
    Center,
}

/// Why a window is brought forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShowReason {
    /// The user must approve something in it.
    Approval,
    /// It shows a job's result.
    Result,
    /// It asks the user something only the app can ask.
    Question,
}

/// `desktop_launch_app`. Tier 1; an ambiguous name returns candidates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopLaunchApp {
    /// The app, as the user said it.
    pub name: String,
    /// Only resolve the name.
    #[serde(default)]
    pub dry_run: bool,
    /// Wait for its main window, ms.
    pub wait_main_window_ms: Option<u32>,
}

/// `desktop_wait_for`: at most 30,000 ms. Tier 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopWaitFor {
    /// What to wait for.
    pub condition: WaitCondition,
    /// How long, ms.
    pub timeout_ms: u32,
}

/// What `desktop_wait_for` waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    /// A window of this process opens.
    WindowOpen {
        /// The process name.
        process: String,
        /// Text its title contains.
        title_contains: Option<String>,
    },
    /// A window closes.
    WindowClosed {
        /// The window.
        hwnd: i64,
    },
    /// An element appears.
    Element {
        /// The window.
        hwnd: i64,
        /// The element.
        selector: Selector,
    },
    /// A window becomes the foreground window.
    Foreground {
        /// The window.
        hwnd: i64,
    },
}

/// `desktop_selection`. Tier 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopSelection {
    /// The window; the foreground one when left out.
    pub hwnd: Option<i64>,
    /// Fall back to the clipboard.
    #[serde(default)]
    pub allow_clipboard: bool,
}

/// `desktop_insert_text`: the insertion ladder. Tier 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopInsertText {
    /// The text.
    pub text: String,
    /// Where it goes.
    pub target: InsertTarget,
    /// What kind of field it is.
    pub category: Option<FieldCategory>,
}

/// Where inserted text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InsertTarget {
    /// The focused element.
    Focused,
    /// An element of a window.
    Element {
        /// The window.
        hwnd: i64,
        /// The element's index.
        element: u32,
    },
    /// A window's focused element.
    Window {
        /// The window.
        hwnd: i64,
    },
}

/// `desktop_volume`, served through the `SystemVolume` platform trait. Tier 1 (`Get` 0); the
/// inverse is journaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DesktopVolume {
    /// What to do.
    pub action: VolumeAction,
}

/// What `desktop_volume` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VolumeAction {
    /// Set the volume.
    Set {
        /// Percent.
        pct: u8,
    },
    /// Change the volume.
    Step {
        /// Percentage points, up or down.
        delta_pct: i8,
    },
    /// Mute or unmute.
    Mute(bool),
    /// Read the volume.
    Get,
}

pub(super) fn inputs(out: &mut BTreeMap<&'static str, ToolInput>) {
    input::<DesktopWindows>(out, MODULE, "desktop_windows");
    input::<DesktopTree>(out, MODULE, "desktop_tree");
    input::<DesktopScreenshot>(out, MODULE, "desktop_screenshot");
    input::<DesktopClickElement>(out, MODULE, "desktop_click_element");
    input::<DesktopSetValue>(out, MODULE, "desktop_set_value");
    input::<DesktopTypeText>(out, MODULE, "desktop_type_text");
    input::<DesktopPressKeys>(out, MODULE, "desktop_press_keys");
    input::<DesktopClick>(out, MODULE, "desktop_click");
    input::<DesktopScroll>(out, MODULE, "desktop_scroll");
    input::<DesktopDrag>(out, MODULE, "desktop_drag");
    input::<DesktopWindow>(out, MODULE, "desktop_window");
    input::<DesktopLaunchApp>(out, MODULE, "desktop_launch_app");
    input::<DesktopWaitFor>(out, MODULE, "desktop_wait_for");
    input::<DesktopSelection>(out, MODULE, "desktop_selection");
    input::<DesktopInsertText>(out, MODULE, "desktop_insert_text");
    input::<DesktopVolume>(out, MODULE, "desktop_volume");
}

pub(crate) fn register(r: &mut Registry) {
    r.add::<DesktopWindows>();
    r.add::<DesktopTree>();
    r.add::<DesktopScreenshot>();
    r.add::<ShotTarget>();
    r.add::<DesktopClickElement>();
    r.add::<DesktopSetValue>();
    r.add::<DesktopTypeText>();
    r.add::<DesktopPressKeys>();
    r.add::<DesktopClick>();
    r.add::<MouseButton>();
    r.add::<DesktopScroll>();
    r.add::<DesktopDrag>();
    r.add::<DesktopWindow>();
    r.add::<WindowAction>();
    r.add::<SnapZone>();
    r.add::<ShowReason>();
    r.add::<DesktopLaunchApp>();
    r.add::<DesktopWaitFor>();
    r.add::<WaitCondition>();
    r.add::<DesktopSelection>();
    r.add::<DesktopInsertText>();
    r.add::<InsertTarget>();
    r.add::<DesktopVolume>();
    r.add::<VolumeAction>();
}

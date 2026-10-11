"""What the DesktopState grounding service reports: monitors, windows, focus, selection."""

from datetime import datetime
from typing import Literal

from pydantic import BaseModel, Field

SelectionSource = Literal["uia_textpattern", "clipboard_fallback", "none"]


class Rect(BaseModel):
    """A rectangle in physical pixels; left and top are inclusive, right and bottom exclusive."""

    left: int
    top: int
    right: int
    bottom: int

    @property
    def width(self) -> int:
        """Width in physical pixels."""
        return self.right - self.left

    @property
    def height(self) -> int:
        """Height in physical pixels."""
        return self.bottom - self.top


class Monitor(BaseModel):
    """One display, with a left-to-right ordinal we assign ourselves."""

    ordinal: int = Field(ge=1, description="Left-to-right ordinal assigned by Eileen, from 1.")
    gdi_name: str = Field(description=r"GDI device name, for example \\.\DISPLAY1.")
    friendly_name: str = Field(description="EDID or driver friendly name, for example DELL U2720Q.")
    rect: Rect
    work_rect: Rect = Field(description="Rect minus taskbar and other app bars.")
    dpi: int = Field(ge=1, description="Effective DPI for this monitor (96 means 100 percent).")
    is_primary: bool = False


class Window(BaseModel):
    """A top-level window as seen through EnumWindows, DWM and MonitorFromWindow."""

    hwnd: int
    pid: int
    process: str = Field(description="Executable name without path, for example chrome.exe.")
    title: str = ""
    monitor_ordinal: int = Field(ge=1)
    rect: Rect = Field(description="DWM extended frame bounds in physical pixels.")
    is_cloaked: bool = False
    z_order: int = Field(ge=0, description="0 is topmost in the visible stack.")
    is_foreground: bool = False


class FocusedElement(BaseModel):
    """The UI Automation element that has keyboard focus."""

    control_type: str
    name: str = ""
    automation_id: str = ""
    rect: Rect
    patterns: list[str] = Field(default_factory=list, description="UIA pattern names available.")
    window_hwnd: int


class Selection(BaseModel):
    """Selected text, if any, and how it was obtained."""

    text: str | None = None
    source: SelectionSource = "none"


class DesktopState(BaseModel):
    """One snapshot of what the user is looking at, built by the grounding service."""

    monitors: list[Monitor]
    windows: list[Window]
    foreground_hwnd: int = Field(description="0 when no window is in the foreground.")
    focused_element: FocusedElement | None = None
    selection: Selection = Field(default_factory=Selection)
    caret_rect: Rect | None = None
    virtual_desktop_id: str | None = None
    recent_references: list[str] = Field(
        default_factory=list,
        description="Short names of recently focused or mentioned windows, newest first.",
    )
    captured_at: datetime

    @property
    def foreground_window(self) -> Window | None:
        """The window that owns foreground_hwnd, if it is in the snapshot."""
        for window in self.windows:
            if window.hwnd == self.foreground_hwnd:
                return window
        return None

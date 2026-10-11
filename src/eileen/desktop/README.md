# desktop

Grounding and action on the Windows desktop. Two halves: the DesktopState service (what is the user looking at) and the Cua Driver adapter (do something about it). Both speak MCP to the Python core.

## Builds on

| Half | Choice |
|---|---|
| DesktopState service | Our own sidecar on Win32 + DWM + UI Automation events. Reference code: terminator, Windows-MCP, mcp-windows, Microsoft UFO (all MIT). |
| Sidecar language | Rust (uiautomation-rs plus the windows crate) or C# (FlaUI.UIA3). The first implementer decides. The Rust toolchain is not installed on the owner's machine today. |
| Actions | Cua Driver (trycua/cua, MIT): UI Automation first, pixel fallback, synthetic cursor. |
| Schema | `eileen.schemas.desktop.DesktopState` |

## First task

The sidecar, exposing `desktop.state`, `window.move|snap|focus|minimize|maximize` and a focus/foreground event stream. Every rect is physical pixels under per-monitor-V2 DPI awareness. Monitor ordinals are ours, left to right. Wrap every UIA call in a timeout.

## Must never

Never reach for a screenshot when a UIA, COM or DOM path exists. Never bundle komorebi, GlazeWM or screenpipe; their licenses do not allow it. Talk to them over IPC if the user installed them.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), sections 2.2 and 9.

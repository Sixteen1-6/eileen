# apps

Deterministic adapters for applications that expose a real API. Each adapter reads state (selection, active cell, playhead, now playing) and performs actions without touching the mouse.

## Builds on

| App | Hook |
|---|---|
| Word, Excel, PowerPoint, Outlook | COM through the running object table; Selection, ActiveCell, ActiveExplorer |
| Explorer | `Shell.Application` COM: selected files per window |
| Spotify, Chrome, Media Player and any media app | Windows SMTC (`Windows.Media.Control`): now playing, position, transport |
| DaVinci Resolve 21 (later) | Python API via fusionscript on Studio |
| VEGAS Pro 23 (later) | A small Application Extension DLL that opens a named pipe |

## First task

COM adapters for Office selection and Explorer selected files, plus SMTC now-playing and play/pause/next/seek. Attach to running instances only. Each adapter returns pydantic models and is exposed to the core as MCP tools.

## Must never

Never click, type or screenshot when a COM, SMTC or scripting path exists. Never install plugins into a user's app without asking.

Background: [docs/RESEARCH.md](../../../docs/RESEARCH.md), section 2.4.

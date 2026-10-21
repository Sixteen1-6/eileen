# Deterministic deep hooks for the owner's apps

Research pass 2026-10-11, refreshed 2026-10-20. Question: for the apps in the owner's examples (DaVinci Resolve, Photoshop, Premiere Pro, the browser, media, Office, Explorer, VS Code, Windows Terminal, and "the file I was working on"), what can Eileen drive through a real API instead of clicking, which rung of the action ladder does each sit on, and which language does each adapter need now that the always-on runtime is Rust-first?

This extends [RESEARCH.md section 2.4](../RESEARCH.md#24-app-hooks-that-beat-clicking) and the ladder in [ARCHITECTURE.md section 3](../ARCHITECTURE.md#3-action-layer-and-the-escalation-ladder). It is written for contributors who will build the adapters under M2 and the v0.2 app items in [ROADMAP.md](../ROADMAP.md).

## How to read this

Every claim carries a tag saying how it was checked:

- **[ran]**: run or measured on the owner's PC (Windows 11 Home build 26200, Core Ultra 7 155H, 16 GB RAM, no NVIDIA GPU). Unmarked [ran] means 2026-10-11. **[ran 10-20]** means the 2026-10-20 refresh.
- **[primary]**: read from the vendor's own docs, the scripting README shipped with the app, release notes, or a project's own README or changelog.
- **[secondary]**: press, vendor blogs about competitors, forum answers, directory sites. Treat as soft.
- **[inferred]**: our reasoning from the above, not tested.

Rung numbers follow ARCHITECTURE.md section 3: 1 App Actions, 2 COM and app scripting, 3 UI Automation patterns, 4 coordinate input with foreground escalation, 5 browser DOM, 6 vision.

Numbers are single-machine measurements, not benchmarks. The 2026-10-11 numbers were taken on a mostly idle machine and are the baseline. The 2026-10-20 numbers were taken while other workloads held the CPU at 90 to 100 percent with 1.1 to 1.9 GB of RAM free [ran 10-20]; they are labelled "loaded" and show how hooks behave under pressure, not their best case.

## 0. What changed between 2026-10-11 and 2026-10-20

- **Resolve 21.1.1 shipped (2026-10-06)** with asynchronous scripting APIs for Quick Export, transcription, scene cut detection, Smart Reframe and subtitle creation, and an option for `ApplyGradeFromDRX` to apply to all node layers [secondary, Digital Production]. Blackmagic's 21.1 readme (2026-09-08) lists the 21.1 scripting additions as capabilities, not function names [primary].
- **Resolve's native MCP server is `ResolveMCP.exe`, a stdio server** with a small tool set that reportedly includes `run_script`, which runs Python with a `resolve` object injected, using Resolve's bundled Python [secondary, voyager.so]. It is not on this machine (21.0.1) [ran 10-20].
- **davinci-resolve-mcp is at v4.14.4 (2026-10-19)**, with a run of Windows fixes from contributors in 4.13.x [ran 10-20 for the version; primary for the changelog].
- **The Lua bridge under `fuscript.exe` now runs supervised from Rust** (it was driven from Python on 10-11) [ran 10-20].
- **Rust now drives SMTC control, not just reads.** Pause, seek and play from the Rust probe were accepted in 0.6 to 2.2 ms against a test session and took effect [ran 10-20].
- **YouTube in Chrome publishes a seekable SMTC session.** In a scratch Chrome 155 profile, the `&t=930s` parameter landed playback at 15:30 after a preroll ad; Rust read the video's title, position and length through SMTC and paused it (the page confirmed `paused`). An SMTC seek was accepted, YouTube answered it with a mid-roll ad, and Chrome's SMTC session disappeared while the ad played [ran 10-20].
- **Claude in Chrome is an account-level connection, not only a local pipe.** This session's `list_connected_browsers` returned two Windows browsers "connected to this account" with device ids and no default selection [ran 10-20], and Anthropic's docs name `bridge.claudeusercontent.com` in the connection error table [primary].
- **Premiere 26.2 added UXP Hybrid plugins** (native C++ `.uxpaddon` modules loaded by a UXP plugin) and 26.3 added `ObjectMaskUtils`, which only reports whether an object mask exists [primary].
- **Measured under load:** a fresh SMTC manager request costs 63 to 83 ms p50 and 0.7 to 2.2 s p95, while re-reading sessions from a manager that is kept alive costs about 1 microsecond; `yt-dlp --version` took 106 s [ran 10-20]. Section 8 explains why this matters more than raw call speed.

## 1. Summary

| Target | Recommended adapter | Rung | Language | Status |
|---|---|---|---|---|
| DaVinci Resolve Studio 21 | Small Lua bridge run by Resolve's own `fuscript.exe`, supervised by the Rust core over stdio. On 21.1 and later, also `ResolveMCP.exe` as an MCP server that the Claude and Codex workers use, and that the Rust core can call as an MCP client. Python fusionscript sidecar only as a fallback | 2, with rung 4 keystrokes for play, blade, tracking and Analyze | Lua (inside Resolve's LuaJIT), Rust supervisor | Bridge runs under Rust supervision [ran 10-20]; Resolve calls untested because Resolve was not running on either date |
| Photoshop 2026 | COM `Photoshop.Application` with `DoJavaScript`, called late-bound from Rust; an Eileen UXP plugin for batchPlay-only features | 2 | Rust for COM; JavaScript inside Photoshop for UXP | Research only (not installed) |
| Premiere Pro 2026 (26.5.2) | An Eileen UXP plugin that connects out to a loopback WebSocket served by the Rust core. A Hybrid plugin with a native module is the fallback if WebSocket proves fragile | 2 | JavaScript inside Premiere; Rust server | API checked in Adobe's reference [primary]; plugin not built |
| Browser (Chrome 155, Edge 155) | Known URL: open it with `ShellExecute`. Otherwise Playwright MCP with its extension. Claude in Chrome only inside the Claude worker | 2 (URL launch), 2 (SMTC), 5 | Rust for launch and SMTC; Node sidecar for Playwright MCP | `&t=` landing and SMTC read and pause on a YouTube tab [ran 10-20]; Claude in Chrome connection listed, no actions taken |
| Media (any player) | SMTC (`Windows.Media.Control`) straight from the `windows` crate, one long-lived manager, event-driven | 2 | Rust | Read and control from Rust, on a test session and on YouTube in Chrome [ran 10-20] |
| Word, Excel | Late-bound `IDispatch` from Rust, attach via the running object table, or bind to the foreground window | 2 | Rust | Selection and ActiveCell read from Rust [ran] |
| Explorer | `Shell.Application` (or native `IShellWindows`) from Rust | 2 | Rust | Selection read from Rust [ran] |
| VS Code 1.137 | `code --goto` for open-at-line; a small Eileen extension for editor state | 2 | Rust for CLI; TypeScript inside VS Code | CLI checked [ran] |
| Windows Terminal 1.24 | `wt -w 0 new-tab / split-pane` only | 2 | Rust | Version [ran 10-20]; arguments from docs [primary] |
| "The file I was working on" | An in-memory recent-work index in the Rust core: jump lists, Recent folder, Office MRU, app-native recents, a file-change watcher. Windows Search as slow fallback | 2 | Rust | Jump lists, Recent, MRU, Windows Search measured [ran] |

Four findings change the plan in ARCHITECTURE.md:

1. **Resolve does not need Python of our own.** Resolve ships `fuscript.exe`, a script interpreter with LuaJIT 2.1 and the `bmd` scripting table built in. A Lua bridge under it answered stdio requests with a 0.05 ms round trip from a Python driver on an idle machine [ran], and 0.51 to 0.58 ms p50 from a Rust supervisor under load [ran 10-20]. The "Python sidecar is unavoidable for Resolve" assumption in the task brief does not hold. Python stays as a fallback because the smauto code path already proves it.
2. **Resolve 21.1 (September 2026) ships its own MCP server in Studio**, `ResolveMCP.exe`, configured from File > Setup AI Assistants for Claude Desktop, Claude Code, Codex, Antigravity and Grok [secondary]. The same release moved Python scripting to Studio [primary]. The owner runs 21.0.1, so neither is on this machine yet [ran 10-20].
3. **For most hooks, Rust is not faster at the call itself.** Cross-process COM into Word costs about 10 ms per property read from Rust [ran]; Windows Search costs 0.4 to 1 s per query from Rust and from Python alike [ran]. What Rust buys is startup (83 ms for the probe binary against 431 to 675 ms for Python plus pywin32 or winrt imports [ran]), a 0.3 to 0.45 MB adapter with no runtime to install [ran], one process that owns its COM apartments and timeouts, and removing Python from the Resolve path.
4. **Warm state beats fast calls.** Under load, anything that starts a process or re-requests a system object per utterance becomes slow and unpredictable (SMTC manager request p95 up to 2.2 s, `fuscript.exe` spawn up to 9 s, `yt-dlp` start 106 s), while reads against handles a resident process already holds stay in microseconds [ran 10-20]. The adapters should be long-lived inside the Rust core, subscribe to change events, and answer the coordinator from memory.

## 2. DaVinci Resolve Studio 21

### State on this machine

- Installed: DaVinci Resolve Studio 21.0.1.11 (`Resolve.exe` file version) [ran, re-checked 10-20]. Not running on either date, so no live API calls were made and nothing was launched [ran 10-20]. `ResolveMCP.exe` is absent from the install folder; `fuscript.exe` is present [ran 10-20].
- Scripting docs shipped with the install: `C:\ProgramData\Blackmagic Design\DaVinci Resolve\Support\Developer\Scripting\README.txt` (113 KB, dated June 2026) and `CHANGELOG.txt` [primary].
- Newer releases exist:
  - 21.0.4 (August 2026) added `Timeline.GetSelectedClips()`, `MediaPoolItem.GetTimeline()` and new render keys [secondary, Digital Production 2026-08-25].
  - 21.1 (2026-09-08) added the native MCP server, about 20 new scripting capabilities (keyboard and render presets, transcription with speaker and timing data, multicam create, flatten and SmartSwitch, auto-align, transitions, fades, speed changes, audio normalize, project-setting presets, output blanking, DCTL validation), built-in Python for the console, dropped Python 2, and made "advanced scripting" Studio-only [primary, Blackmagic 21.1 readme]. Projects opened in 21.1 cannot be opened in 20.3 [secondary].
  - 21.1.1 (2026-10-06) added asynchronous APIs for Quick Export, transcription, scene cut detection, Smart Reframe and subtitles, and an all-node-layers option for `ApplyGradeFromDRX` [secondary, Digital Production; the article notes signatures were not published].
- None of these releases is reported to add a call that creates a color node or places an OFX effect [secondary, all release coverage read on 10-20].

### What the scripting API can do for "go to 15:30 and fix the face"

All rows are from the 21.0.1 README [primary] unless tagged otherwise.

| Need | API | Notes |
|---|---|---|
| Is Resolve there | `Resolve.GetProductName()`, `GetVersionString()`, `GetCurrentPage()` | Free tier: external scripting is Studio-gated |
| Move the playhead | `Timeline.SetCurrentTimecode(tc)`, `GetCurrentTimecode()` | Works on Cut, Edit, Color, Fairlight and Deliver pages only, not Media or Fusion. Refuses a timecode before the timeline start with a bare `False` [secondary, davinci-resolve-mcp changelog, measured on 19.1.3.7] |
| Timeline geometry for "15:30" | `Timeline.GetStartTimecode()`, `GetStartFrame()`, `GetSetting("timelineFrameRate")` | Spoken "15:30" is an offset; the timeline usually starts at 01:00:00:00 |
| Switch page | `Resolve.OpenPage("color")` | |
| Clip under the playhead | `Timeline.GetCurrentVideoItem()` | Returns one item; which track it picks with stacked video is not documented [inferred: check live] |
| Selected clips | `MediaPool.GetSelectedClips()` (media pool only in 21.0.1); `Timeline.GetSelectedClips()` from 21.0.4 [secondary] | |
| Frame for grounding | `Project.ExportCurrentFrameAsStill(path)`, `Timeline.GetCurrentClipThumbnailImage()` (Color page only), `Timeline.GrabStill()` | The still is what a face detector or vision model looks at |
| Grade a clip | `TimelineItem.SetCDL`, `Graph.SetLUT`, `Graph.ApplyGradeFromDRX(path, mode)`, `TimelineItem.CopyGrades`, color versions | No call adds a node or an OFX effect; see below |
| AI tools | `TimelineItem.Stabilize()`, `SmartReframe()`, `CreateMagicMask(mode)`, `RegenerateMagicMask()`; `MediaPoolItem.RemoveMotionBlur({...})` (new in 21.0); `AnalyzeForIntellisearch(identifyFaces, ...)` | `CreateMagicMask` only tracks an existing mask. Magic Mask v2 isolates through operator clicks the API cannot place, so on a fresh item it returns `False` [secondary, davinci-resolve-mcp changelog] |
| Fusion | `TimelineItem.AddFusionComp()`, `ImportFusionComp(path)`, `GetFusionCompByIndex`, then the full Fusion comp API (`AddTool`, `SetInput`, connections) | davinci-resolve-mcp reports Fusion writes under `Comp.Lock()` that read back but do not render [secondary] |
| Swap in a fixed version of a clip | `TimelineItem.AddTake(mediaPoolItem, start, end)`, `SelectTakeByIndex`, `FinalizeTake` | With `RemoveMotionBlur` this is a fully scripted deblur path [inferred: untested] |
| Markers | `AddMarker`, `GetMarkers`, `DeleteMarker*` on timeline, item and media pool item, with `customData` | |
| Render | `SetRenderSettings`, `AddRenderJob`, `StartRendering`, `GetRenderJobStatus`, `RenderWithQuickExport` | Proven in smauto (`resolve_render.py`). On 21.1, `StartRendering` sometimes returns `False` and leaves a job Ready [secondary, contributor report in davinci-resolve-mcp 4.13.0] |

### What it cannot do (21.0.1)

From reading the full object list in the README [primary]:

- No call adds a color node, adds a ResolveFX or OFX plugin to a node, or sets plugin parameters. `Graph` exposes only node count, LUT, cache mode, label, tools-in-node, enable, `ApplyGradeFromDRX`, ARRI CDL and reset. The workaround is to write a `.drx` that already holds the nodes and apply it; davinci-resolve-mcp generates DRX files with OFX nodes whose parameters bind on 21.0 [secondary, its v4.8.19 changelog].
- No power window, qualifier or tracker control. No way to press Face Refinement's **Analyze** button, which the effect needs before it does anything [secondary, UT Austin and PremiumBeat guides].
- No Magic Mask strokes (see above).
- No play, stop, shuttle or loop. No blade or split. No trim, slip or move setters for timeline items [secondary, PilotCut ledger; voyager.so lists clip splitting and speed ramps as Edit-page-only for the native MCP too].
- No node-graph tidy ("Cleanup Node Graph" is UI only) [secondary, davinci-resolve-mcp].
- `GetCurrentClipThumbnailImage` needs the Color page.

So "make her face less blurry" has no single API call. The deterministic part covers navigation, picking the clip, grounding the face and applying a prepared grade; tracking and Analyze fall to keystrokes or, last, vision.

### Worked example: "open DaVinci, go to 15:30 and make the girl on the right's face less blurry"

| Step | How | Rung | Verified |
|---|---|---|---|
| 1. Resolve open and scriptable | Check for `Resolve.exe` in DesktopState before any script call. If absent, launch it (tier 1, reversible). Then `hello` through the bridge | 2 | Bridge under Rust [ran 10-20]; launch not tried by rule |
| 2. Go to 15:30 | Bridge `timeline_info` returns start TC and fps. Rust adds 930 s worth of frames to the start TC (drop-frame uses `;`) and calls `goto_tc` with an absolute timecode such as `01:15:30:00`, never the raw spoken offset, which Resolve refuses silently | 2 | [inferred]; API [primary]; refusal [secondary] |
| 3. Clip under playhead | `OpenPage("color")`, `GetCurrentVideoItem()` | 2 | [primary] |
| 4. Find "the girl on the right" | `ExportCurrentFrameAsStill` to a temp PNG. Run a local face detector in Rust (YuNet, MIT, OpenCV Zoo, via `ort`) and take the right-most face. Ask the vision model only if the detector is unsure whether that face is "the girl" | 2 plus local inference | Detector choice [primary]; not run |
| 5a. Motion blur | `RemoveMotionBlur({UseMarkInMarkOut=true, ...})` on the source around the range (renders new media, never touches the source), then `AddTake` and `FinalizeTake` to swap it in | 2 | [inferred] |
| 5b. Soft focus on the face | Generate a DRX still holding a node with a circular window at the face box plus sharpening (or a Face Refinement node), apply with `ApplyGradeFromDRX`. On 21.1.1 the new all-node-layers option matters for clips that already carry layer nodes | 2 | [inferred]; DRX OFX generation [secondary] |
| 6. Track the window or run Analyze | Keystroke into Resolve (Track Forward is a default Color page shortcut; Analyze for Face Refinement has no known default shortcut; 21.1's keyboard-preset API may allow binding one). Resolve is a custom-drawn Qt app, so expect UIA to show little | 4, then 6 for Analyze | [inferred] |
| 7. Check the result | Export the frame again and compare sharpness (variance of Laplacian) inside the face box, in Rust | 2 | [inferred] |

Step 6 is the only part that needs the screen. Everything else is scripted and fast enough to narrate as it happens.

### Transports, compared

| Option | What it is | Fit |
|---|---|---|
| **A. Lua bridge under `fuscript.exe` (recommended)** | Resolve ships `fuscript.exe` (95 KB) with options `-l lua|py3`, `-x`, `-i`, `-s` (script server) [ran]. It runs LuaJIT 2.1.0-beta3 with FFI and a `bmd` table of 52 helpers including `scriptapp`, `readfile`, `writestring` [ran]. Idle, driven from Python: spawn to first output 594 ms; round trip p50 0.053 ms, p95 0.104 ms over 200 requests [ran]. Loaded, driven from Rust: spawn to first output 1.1, 3.4 and 9.0 s over three runs; round trip p50 0.51 to 0.58 ms, p95 0.78 to 0.95 ms over 500 requests [ran 10-20] | No Python on the user's machine. Same "External scripting = Local" gate as Python. Spawn once and keep it alive |
| B. Python fusionscript sidecar | `fusionscript.dll` is a CPython extension (`PyInit_fusionscript` export) that also links `lua5.1.dll` and WinSock [ran, dumpbin]. Proven from Python 3.13 by smauto | Fallback; keeps parity with community code |
| C. davinci-resolve-mcp | MIT, v4.14.4 on npm (published 2026-10-19) [ran 10-20]; about 3.7k stars, 37 compound tools by default, 390 granular, 18 offline `.drp/.drt/.drx` tools; Python 3.10+ server (3.10 to 3.12 lowest risk) plus a Node "advanced" server [primary]. The maintainer measures on Studio 19.1.3.7 on macOS; Windows and 21.1 behaviour comes from contributor reports, and 4.13.1 to 4.13.5 fixed Windows-only bugs such as a liveness check that killed the process it checked [primary, changelog] | Optional breadth server for the Claude and Codex workers. Too large to embed; spawn if the user installs it |
| D. Resolve 21.1 native MCP (`ResolveMCP.exe`) | Studio only; plain stdio, no network mode; File > Setup AI Assistants writes client configs, and other clients can point at the executable path. A forum user counted 14 tools; names reported by a third party include `get_resolve_status`, `search_scripting_api`, `get_scripting_api`, `run_script` and `run_script_unsafe`. Resolve 21.1 bundles Python 3.14 as `ResolvePython` [secondary, voyager.so]. Whether it needs External scripting = Local is reported both ways [secondary] | Best path for the Claude and Codex workers after an upgrade. The Rust core can also speak to it as an MCP client over stdio through `rmcp` [inferred], which would give a Blackmagic-maintained transport with no Eileen-owned interpreter. Measure its latency against A before switching |
| Rust calling fusionscript.dll directly | The DLL's API is MSVC-mangled C++ (`Fusion::ScriptVal`, `RemoteApp::Connect`) with no header [ran] | Not viable. The wire protocol to Resolve is undocumented |

### Sample code shape

The bridge, smoke-tested under `fuscript.exe` on this machine with Resolve closed (it answered `pong`, `unknown_method` and `resolve_not_reachable` correctly) [ran, ran 10-20]. Abridged:

```lua
-- eileen_resolve_bridge.lua, run as: fuscript.exe -q -l lua eileen_resolve_bridge.lua
-- Request: "<id>\t<method>\t<arg>..." per line. Response: one JSON object per line.
local resolve = nil
local function app()
  if resolve == nil then resolve = bmd.scriptapp("Resolve") end
  if resolve == nil then error("resolve_not_reachable", 0) end
  return resolve
end

local methods = {}
function methods.ping() return "pong" end
function methods.goto_tc(tc)
  local tl = app():GetProjectManager():GetCurrentProject():GetCurrentTimeline()
  if not tl:SetCurrentTimecode(tc) then error("set_timecode_refused", 0) end
  return tl:GetCurrentTimecode()
end
function methods.apply_drx(path)
  local tl = app():GetProjectManager():GetCurrentProject():GetCurrentTimeline()
  return tl:GetCurrentVideoItem():GetNodeGraph():ApplyGradeFromDRX(path, 0)
end

for line in io.stdin:lines() do
  local f = split(line)                                  -- tab split, 5 lines
  local ok, result = pcall(methods[f[2]], unpack(f, 3))
  print(encode({ id = f[1], ok = ok, result = ok and result or nil, error = (not ok) and tostring(result) or nil }))
end
```

The Rust supervisor, as run on 10-20 (trimmed from the probe; the real one adds a reader thread and a per-request timeout) [ran 10-20]:

```rust
let mut child = Command::new(r"C:\Program Files\Blackmagic Design\DaVinci Resolve\fuscript.exe")
    .args(["-q", "-l", "lua", script])
    .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
    .spawn()?;
let mut stdin = child.stdin.take().ok_or(Missing::Stdin)?;
let mut out = BufReader::new(child.stdout.take().ok_or(Missing::Stdout)?);
let mut ready = String::new();
out.read_line(&mut ready)?;                     // {"id":"ready","ok":true,"result":{"lua":"Lua 5.1","jit":"LuaJIT 2.1.0-beta3"}}
writeln!(stdin, "{id}\tgoto_tc\t01:15:30:00")?;
stdin.flush()?;
let mut reply = String::new();
out.read_line(&mut reply)?;                     // parse with serde_json
```

The full bridge (about 130 lines with the JSON encoder) belongs in `adapters/resolve/` with the Rust supervisor next to it. Lua's `pairs` order is unspecified, so JSON key order varies between runs [ran 10-20]; parse, never compare strings.

### Resolve pitfalls

- `bmd.scriptapp("Resolve")` blocks about 4 to 4.6 s before returning nil when Resolve is not running [ran, ran 10-20], and the bridge as written pays it again on every request (two consecutive `hello` calls took 4174 and 4065 ms) [ran 10-20]. The Rust side must check the process list first and only forward calls while `Resolve.exe` is alive.
- External scripting must be set to Local in Preferences > System > General. With None, connections fail silently [secondary].
- Resolve 21.1 removed Python scripting from the free edition; Lua scripts still list in the free Scripts menu [secondary, davinci-resolve-mcp and a community Lua MCP]. External scripting stays Studio-only either way.
- `SetCurrentTimecode` refuses sub-start timecodes with a bare `False` [secondary]. Compute absolute timecodes, with the drop-frame separator, in Rust, never in the model.
- A project's frame rate locks once its first timeline exists, and `SetRenderSettings` rejects a mismatched FrameRate (learned in smauto).
- On 21.1, mutating a project-settings preset before the project's first save destroyed the project in a contributor's test, although every call returned `True` [secondary, davinci-resolve-mcp 4.13.0]. Eileen must not write presets on unsaved projects.

## 3. Photoshop 2026 (research only, not installed)

`Photoshop.Application` is not registered on this machine; attaching from Rust returned "Invalid class string" in 1.9 ms [ran, ProgID absence re-checked 10-20]. Photoshop 2026 is the 27.x line (27.4 to 27.10 appear in Adobe forums and MCP test reports) [secondary]. A 10-20 search found no Adobe-published MCP server for desktop Photoshop; every listing was community-built [secondary].

| Surface | What it gives | Status |
|---|---|---|
| COM `Photoshop.Application` | Out-of-process automation on Windows, including `Open`, documents, and `DoJavaScript(code)` which runs ExtendScript inside Photoshop. Version-specific ProgIDs exist (`Photoshop.Application.<n>`) | [secondary, Adobe community and photoshop-python-api]. Same `IDispatch` helper as Word from Rust [inferred] |
| ExtendScript DOM | `app.recentFiles` (the File > Open Recent list; also readable with `executeActionGet` on the `recentFiles` key), `ArtLayer.duplicate()`, `applyUnSharpMask`, `applySharpen`, selections, `executeAction` for anything recordable | [secondary, Adobe community answers]. Adobe: "ExtendScripts are not affected by this transition" [primary, Adobe developer blog, September 2026] |
| UXP (plugins and `.psjs` scripts) | `batchPlay` reaches features the DOM lacks. Generative Fill works through a `syntheticFill` descriptor with a Firefly service id [secondary, CC developer forum]. UXP plugins can only open sockets as clients [primary, adb-mcp README] | New features land in UXP only [primary, Adobe blog] |
| Neural Filters | Run as an internal UXP component (`com.adobe.nfp.gallery`) [primary, Adobe help]. No documented descriptors; photoshop-mcp-imt reports a `neural_filter` operation through its UXP bridge [secondary] | Treat as unverified until tested |
| CEP | Marketplace stops new CEP submissions in March 2027; CEP disabled by default from December 2027 in Photoshop [primary, Adobe blog] | Do not build on CEP |
| Adobe's hosted connector | A hosted MCP connector launched with Anthropic on 2026-04-28, at Express and Firefly level, not the desktop Photoshop session [secondary] | Useful to the Claude worker; not a desktop hook |

Existing Photoshop MCP servers:

| Project | License | Transport | Note |
|---|---|---|---|
| mikechambers/adb-mcp | MIT, about 730 stars [primary, 10-20] | Python MCP servers per app, Node proxy on `ws://localhost:3001`, UXP plugins for Photoshop, Premiere and InDesign, CEP for After Effects and Illustrator; Photoshop 26.0+; tested with Claude Desktop on Windows | Proof of concept by an Adobe engineer, not supported by Adobe; plugins reload by hand after each app restart; no file access by default [primary] |
| alisaitteke/photoshop-mcp | MIT, about 620 stars | ExtendScript and a UXP plugin; 111 atomic tools plus recipes | Anonymous usage analytics on by default; optional routing to a third-party API [primary]. Do not ship with defaults |
| Immersive-Media-Technologies/photoshop-mcp-imt | IMT Non-Commercial (upstream MIT) | ExtendScript through a cscript VBScript shim on Windows, plus a UXP bridge polling 127.0.0.1:38452 that answers while ExtendScript is blocked by a modal | Never bundle (license). Its modal-safe UXP idea is worth copying [primary] |

### Worked example: "open Photoshop and open the image where I was editing the girl at the beach, make her face less blurry"

| Step | How | Rung |
|---|---|---|
| 1. Candidates | Merge, newest first: Photoshop's `app.recentFiles` through `DoJavaScript` if Photoshop is running; the recent-work index (section 7) filtered to image types (`psd`, `psb`, `tif`, `jpg`, `png`, `heic`, raw); Photoshop's jump list once its AppID is known | 2 |
| 2. Rank | File name and folder tokens ("beach"), recency. If two or more remain, show the vision model small composites (PSD files usually embed one; the MIT `psd` crate can read it, or `IShellItemImageFactory` where a thumbnail handler exists) and ask which shows a girl at a beach | 2, then content vision (not UI vision) |
| 3. Confirm | One sentence naming the file and its date when the top two are close | voice |
| 4. Open | `app.Open(path)` over COM, or ShellExecute the file with Photoshop | 2 |
| 5. Fix | Duplicate the layer (keeps it reversible, tier 1). Detect faces locally (YuNet), take the girl's face box, make a feathered elliptical selection with `executeAction`, apply `applyUnSharpMask` or Smart Sharpen on the duplicate. Neural Filters or Generative Fill go through the UXP plugin and spend cloud credits, so they are tier 2 and confirm first | 2 |

Rust sketch, using the same helper that read Word's selection on this machine [inferred for Photoshop]:

```rust
let ps = Auto::attach("Photoshop.Application")?;
let js = "var a=[];for(var i=0;i<app.recentFiles.length;i++)a.push(File(app.recentFiles[i]).fsName);a.join('\\n')";
let recent = ps.invoke("DoJavaScript", vec![Var::str(js)])?.text();
```

## 4. Premiere Pro 2026

### State on this machine

- Installed: Premiere Pro 26.5.2 (file version 26.5.2.5) at `C:\Program Files\Adobe\Adobe Premiere Pro 2026`, with `UXP\`, `CEP\` and `extendscript.dll` present [ran, version re-checked 10-20].
- No COM automation: no `Premiere.Application` ProgID is registered [ran, re-checked 10-20]. The only Premiere URL handlers are `adobeppro:` and a Frame.io sign-in protocol [ran].
- Adobe ships an internal UXP panel `com.adobe.dva.creative-copilot` ("Assistant") with loopback network permission [ran, its manifest]. It is Adobe's, not an API for us.
- Adobe's plugin installer agent (`UnifiedPluginInstallerAgent.exe`) is present, which is how a packaged `.ccx` plugin gets installed [ran for presence; command-line use not tried]. The UXP Developer Tool is not installed [ran 10-20].

### API

The UXP Premiere API (25.6 and later) has what the timeline examples need [primary, Adobe reference and changelog]:

- `Project.getActiveProject()`, `project.getActiveSequence()`, `Project.open(path, options)`.
- `Sequence.getPlayerPosition()` and `setPlayerPosition(TickTime)`; `TickTime.createWithSeconds(s)`, `createWithFrameAndFrameRate`; `SourceMonitor.setPosition` (26.3).
- `Sequence.getSelection()`, `setSelection()` (synchronous from 26.3, and since 26.3 actions must be created inside `project.lockedAccess()`), `clearSelection()`, in and out point actions, `WorkAreaUtils` (26.5), track access.
- `VideoFilterFactory.createComponent(matchName)` and `getMatchNames()` for effects. Inserting the component into a clip's chain goes through the component-chain action API [inferred from the class layout].
- `Transcript.transcribeClipProjectItem` and `isLanguagePackAvailable` (26.5), useful for "go to where she says X".

Gaps: no face tracking or mask creation API. `ObjectMaskUtils` (26.3) has one method, `hasObjectMask(projectOrSequence)`, which only reports whether an object mask exists [primary]. A forum report says `getPlayerPosition()` returns a stale value until the sequence is switched, on 2025 and 2026 builds [secondary].

ExtendScript still works and Adobe "currently has no plan or schedule for removing ExtendScript" [primary, Premiere scripting guide, page dated 2026-10-05], but in Premiere it is reachable only from inside the app (CEP panels or startup scripts). CEP in Premiere stops taking new Marketplace submissions in December 2027, is disabled by default in December 2028, and is removed in December 2029 [primary, Adobe blog]. So an Eileen Premiere adapter is a UXP plugin.

**Hybrid plugins (26.2 and later)** let a UXP plugin `require()` a native C++ `.uxpaddon` for Windows x64 and macOS. Adobe lists platform-only APIs and hardware SDKs among the intended uses; they need UXP Developer Tool 2.2 and Creative Cloud Desktop 5.10 [primary, Adobe Hybrid Plugins guide and blog, April 2026]. For Eileen this is a fallback transport: a native module could open a named pipe to the Rust core instead of relying on a WebSocket from the plugin [inferred]. A pre-release forum comment says the native SDK does not expose Premiere's own API [secondary], so the timeline calls still go through JavaScript.

Existing servers: adb-mcp (MIT, UXP, "assumes a single sequence") [primary]; ayushozha/AdobePremiereProMCP (MIT, CEP and ExtendScript bridge with Go, Rust, Python and TypeScript parts, 74 curated tools, tested on 26.5.2 on macOS, Windows supervision incomplete) [primary].

### Adapter shape

UXP plugins cannot listen, so the Rust core serves `ws://127.0.0.1:<port>` with a per-boot token and the plugin connects out [primary for the constraint; code is a sketch]:

```js
const ppro = require("premierepro");
const ws = new WebSocket(`ws://127.0.0.1:${PORT}/premiere?token=${TOKEN}`);
ws.onmessage = async (event) => {
  const req = JSON.parse(event.data);
  try {
    const seq = await (await ppro.Project.getActiveProject()).getActiveSequence();
    if (req.op === "seek") await seq.setPlayerPosition(ppro.TickTime.createWithSeconds(req.seconds));
    const pos = await seq.getPlayerPosition();
    ws.send(JSON.stringify({ id: req.id, ok: true, seconds: pos.seconds }));
  } catch (err) {
    ws.send(JSON.stringify({ id: req.id, ok: false, error: String(err) }));
  }
};
```

Rung 2. Language: JavaScript inside Premiere (unavoidable), Rust for the server. Install friction is the risk: during development the plugin loads through the UXP Developer Tool; for users it needs a packaged `.ccx`.

## 5. Browser

### What is on this machine

Chrome 155.0.8059.79 on 10-20 (154.0.8037.98 on 10-11) [ran 10-20, ran]; Edge 155.0.4283.45 on 10-11 [ran]. Latest packages on npm on 10-20: `@playwright/mcp` 0.0.83 (2026-09-28, unchanged), `chrome-devtools-mcp` 1.11.0 (2026-10-15), both Apache-2.0 [ran 10-20]. chrome-devtools-mcp 1.11.0 added ffmpeg-free screencast tools, on by default, and config-file discovery [primary, its releases].

Native messaging hosts registered for Chrome: `com.anthropic.claude_browser_extension` (written by the Claude desktop app), plus two unrelated hosts; for Edge, none from Anthropic [ran 10-20]. The Claude Code CLI's own host, `com.anthropic.claude_code_browser_extension`, is not registered, so the standalone CLI has not been set up for Chrome on this machine [ran 10-20, compared with the host names in Anthropic's docs].

### Options

| Path | Who can drive it | Notes |
|---|---|---|
| Claude in Chrome | Claude Code with `--chrome` (or `/chrome`, or "Enabled by default"), Claude desktop sessions, the VS Code extension | Needs a direct paid plan and a `/login` session; sessions on an API key or `claude setup-token` keep it off (since v2.1.216); a project's `.claude/settings.json` cannot turn it on (`CLAUDE_CODE_ENABLE_CFC` is ignored there); works with Chrome, Edge and other Chromium browsers; tabs go into a per-session tab group; Windows named-pipe conflicts are a known issue [primary, code.claude.com/docs/en/chrome, read 10-20]. The connection is per account: this session's `list_connected_browsers` showed two connected Windows browsers with device ids and none selected, and the tool requires the user to pick one before any action [ran 10-20]. It belongs to the Claude worker. Eileen must not talk to its native host itself (D4); it starts the worker with `--chrome` when the user has opted in |
| Playwright MCP `--extension` | Any MCP client | Attaches to the running Chrome or Edge profile through the Playwright Extension; `--profile-dir-name` picks the profile (default: the last used profile with the extension); stdio or `--port` HTTP [primary]. Default for Eileen's own rung 5 and for the Codex, Gemini and Grok workers |
| Chrome DevTools MCP `--autoConnect` | Any MCP client | Chrome 144+; the user first enables remote debugging at `chrome://inspect/#remote-debugging`; Chrome asks for consent on every session and shows the automation banner [primary]. Chrome only |
| Rust CDP client | Eileen core | The same consent flow is reachable from Rust (`chromiumoxide` 0.9.1, MIT or Apache-2.0) [inferred]. A later option to drop Node |

The `claude -p` check could not run on either date. On 10-11 the CLI in the agent's shell reported "Not logged in"; on 10-20 it was 2.1.304 and reported "OAuth session expired and could not be refreshed" after 14.8 s [ran, ran 10-20]. No login was attempted, by rule. Whether browser tools load in print mode is still open.

### "Go to a specific video"

1. **Resolve the video.** If the URL is known (from the conversation, bookmarks or the recent-work index), skip search. Otherwise open `https://www.youtube.com/results?search_query=...` in the user's own browser through the extension and read the result links from the DOM. Headless search is the wrong default: YouTube's robots.txt disallows `/results` and `/youtubei/` [ran]. The YouTube Data API needs a key, which the owner wants to avoid.
2. **yt-dlp is optional and slow.** Idle, yt-dlp 2026.08.19 (still the latest on PyPI on 10-20) resolved `ytsearch5:` in 5.8, 10.3 and 24.4 s over three runs; `--version` alone took 1.1 s [ran]. Loaded, `--version` took 106 s and a flat `ytsearch3:` took 97 s [ran 10-20]. A raw fetch of the results page took 0.9 to 1.6 s idle and 5.8 s loaded, about 1.2 to 1.6 MB, and both times its first video id matched yt-dlp's first result [ran, ran 10-20]. Leave yt-dlp to users who install it.
3. **Open at a time.** `https://www.youtube.com/watch?v=<id>&t=930s`. When no tab control is needed, Rust opens it with `ShellExecuteW` in the default browser; rung 2, no extension, tier 1. Checked on 10-20 in a scratch Chrome 155 profile (own `--user-data-dir`, `--mute-audio`, a debug port so a Node script could read the `<video>` element): a fresh profile got a 15 s preroll ad first, then the main video (length 2677 s) played from 930 s [ran 10-20]. `ShellExecuteW` itself was not run, to keep the user's own browser untouched.
4. **Control playback.** Two deterministic routes, both checked on that tab [ran 10-20]:
   - **SMTC from Rust (rung 2, no extension).** Chrome published a session whose AUMID contains "Chrome", with the video's title, position (964.1 s against 968 s in the DOM), end time (2676.9 s against 2677 s), and seek and pause enabled. `TryPauseAsync` was accepted in 47.6 ms and the page reported `paused`. `TryChangePlaybackPositionAsync(60 s)` was accepted in 1.5 ms; YouTube then started a 15 s mid-roll ad, and while the ad played Chrome published no SMTC session at all. Each probe was a fresh process, so the 1.7 to 3.7 s manager requests under load are the cost section 6 says to avoid.
   - **DOM through the extension (rung 5).** `document.querySelector("video").currentTime = 930` and `.pause()` work: after 30 scripted seeks to 600 to 629 s and a pause, the page reported `currentTime` 629 and `paused` [ran 10-20]. Round trips through CDP were p50 4.9 s with several 30 s timeouts, because the loaded machine starved the page while YouTube buffered after each seek; that number says nothing about an idle machine and was not measured idle. Prefer one seek and an event over polling.
   - YouTube's keyboard shortcuts (k, j, l, digits) need the foreground (rung 4) and are the last resort.
   - Ads are the main source of surprise: after any seek or load, check `.ad-showing` (DOM) or wait for the SMTC session to come back with the expected title before reporting success.

## 6. Media, Office, Explorer, VS Code, Windows Terminal

All "from Rust" results below come from `hookprobe`, a release binary built with Rust 1.99.0 and `windows` 0.62.2 (313 KB on 10-11, 449 KB on 10-20 after adding process supervision; clippy clean both times) [ran, ran 10-20]. `windows` 0.62.2 is still the newest umbrella crate on 10-20; `windows-core` 0.100.0 on crates.io does not mix with it, so take core types from `windows::core` [ran].

### Media keys and SMTC

- Python: `winrt-runtime` and `winrt-Windows.Media.Control` 3.2.1 wheels install on CPython 3.13.5 through uv 0.12.13 in a scratch venv [ran]; 3.2.1 is still the latest on 10-20 [ran 10-20]. Import 319 ms, first `request_async` 160 ms, warm request plus properties p50 15.2 ms [ran].
- Control from a second process (Python): a test session registered by `Windows.Media.Playback.MediaPlayer` playing a silent WAV; pause returned in 4.9 ms, seek in 4.1 ms, play in 14.4 ms, and the seek took effect (position 1:30 after seeking to 90 s) [ran].
- Rust reads: `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.join()?` read the same session (status Playing, can seek) in 57 to 79 ms on a fresh process, warm p50 8.7 to 16 ms, p95 27 to 76 ms [ran].
- Rust control [ran 10-20]: against the same silent-WAV test session, `TryPauseAsync` was accepted in 2.2 ms and the next read showed Paused (5); `TryChangePlaybackPositionAsync(90 s)` in 0.9 ms, next read position 90.0 s; `TryPlayAsync` in 0.6 ms, next read Playing (4). The status read in the same call as the action was still the old one, so confirm on the next event, not inline. After play, the published position stayed at 90.0 s: SMTC position is a snapshot the app pushes, so derive the live position from `Position`, `LastUpdatedTime` and the playback rate. Against YouTube in Chrome, see section 5.
- Under load, with no session present, a fresh `RequestAsync` per read cost 389 to 923 ms on first call, then p50 63 to 83 ms and p95 0.71 to 2.18 s over 50 requests. Holding one manager and re-reading `GetSessions` and `GetCurrentSession` cost p50 0.001 ms and max 0.043 ms over 200 reads [ran 10-20]. No session existed during that run, so the per-session property reads were skipped; the point is the gap between a held manager and a fresh request, not an absolute number.
- Recommendation: Rust, rung 2, no Python. Request the manager once at startup, subscribe to `SessionsChanged` and `CurrentSessionChanged` (and each session's `PlaybackInfoChanged` and `MediaPropertiesChanged`), and keep DesktopState's "now playing" current from those events so the coordinator never waits on SMTC [inferred from the measurements]. Fallback when no session exists: `SendInput` with `VK_MEDIA_PLAY_PAUSE` [inferred].

```rust
let mgr = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.join()?; // once
if let Ok(s) = mgr.GetCurrentSession() {
    let props = s.TryGetMediaPropertiesAsync()?.join()?;
    s.TryChangePlaybackPositionAsync(930 * 10_000_000)?.join()?; // 100 ns units
}
```

### Word and Excel

One late-bound `IDispatch` helper in Rust (about 120 lines: an owned `VARIANT` that clears on drop, `GetIDsOfNames` plus `Invoke`, property get, put and method) drove both apps [ran]:

- Hidden Word started with `CoCreateInstance` in 7.8 s; after selecting a word in a new unsaved document, `Selection.Text` returned "THIS" with p50 9.8 ms, p95 24.9 ms over 50 reads [ran].
- Hidden Excel started in 3.6 s; `ActiveCell.Address` returned "$C$7", p50 12.9 ms, p95 33.1 ms [ran].
- Both quit without saving; Excel's process lingered for a few seconds before exiting [ran].
- With neither app running, `GetActiveObject` failed in 8 to 12 ms with MK_E_UNAVAILABLE ("Operation unavailable") [ran].

```rust
let word = Auto::attach("Word.Application")?;
let selected = word.obj("Selection")?.get("Text")?.text();
let excel = Auto::attach("Excel.Application")?;
let cell = excel.obj("ActiveCell")?.get("Address")?.text();
```

Office pitfalls:

- Office registers in the running object table only after the app first loses focus, by design; until then `GetActiveObject` returns 0x800401E3 [primary, Microsoft KB 238610].
- `GetActiveObject` returns one instance. To act on the window the user is looking at, bind from DesktopState's foreground HWND with `AccessibleObjectFromWindow(..., OBJID_NATIVEOM, ...)` on the document child window [inferred, widely used technique].
- Activating with `CLSCTX_ALL` returned an object without `IDispatch` (E_NOINTERFACE); `CLSCTX_LOCAL_SERVER` works [ran].
- A modal dialog in Office blocks or rejects calls. Run each target on its own STA thread with a call timeout and an `IMessageFilter` for "call rejected by callee" [inferred].

Recent documents for Office also sit in the registry under `HKCU\Software\Microsoft\Office\16.0\<App>\User MRU\...\File MRU`; on this machine Word had 25 and 30 entries in two identities, Excel 21 and 27, PowerPoint 13 and 3, read in 6 ms [ran].

### Explorer

`Shell.Application` through the same helper listed windows, resolved each folder and counted selected items in 92 to 103 ms including object creation [ran]. After opening a scratch folder and selecting one file, Rust read `selected=1` and PowerShell read the file's full path through the same COM objects [ran]. A native route (`IShellWindows`, `IShellBrowser`, `IFolderView::Items(SVGIO_SELECTION)`) avoids late binding [inferred]. Pitfall: on Windows 11 with Explorer tabs, several shell-window entries share one top-level HWND, so "the selection in the foreground window" needs the active tab [inferred].

### VS Code

VS Code 1.137.0's CLI offers `-g file:line[:col]`, `-r`, `-n`, `--add-mcp`, `--status` and a `chat` subcommand, and nothing that reports the active file or selection [ran]. VS Code's jump list exists under AppID `Microsoft.VisualStudioCode` (99 items, read in 125 to 282 ms) [ran]. For editor state, ship a small TypeScript extension that exposes the active editor, selection and visible range over a named pipe to the Rust core. TypeScript is unavoidable there. Rung 2 for both.

### Windows Terminal

Installed: Windows Terminal 1.24.12741.0 [ran 10-20]. `wt -w 0 new-tab` and `split-pane` with `-d`, `-p` and `--size` [primary, Microsoft Learn]. Sending text into an existing pane stays off the table (rejected upstream, already in M2's criteria). Reading the visible buffer through UIA TextPattern is plausible because Terminal supports screen readers [inferred]. Better still, agent work runs in its own pseudo-consoles, not in the user's tabs.

## 7. "Find the file I was working on"

| Source | Speed here | Coverage | Language |
|---|---|---|---|
| Jump lists via `IApplicationDocumentLists` (`SetAppID`, `GetList(ADLT_RECENT)`) | Word 42 items in 189 ms cold, 39 ms warm; VS Code 99 items in 125 ms; Chrome 866 items in 804 ms cold [ran] | Per app, ordered, includes files never opened through a dialog; needs the app's AppUserModelID | Rust [ran] |
| Recent folder (`%APPDATA%\Microsoft\Windows\Recent`, 261 `.lnk` here) | List and sort 22 to 73 ms. Resolving targets with `IShellLinkW` took 29.8 s on the very first run, then 1.5 s (Load p50 4.2 ms, p95 9.9 ms, max 94 ms) [ran] | Everything opened through shell dialogs; 152 of 261 were folders | Rust; parse the `.lnk` bytes directly (`lnk` crate 0.6.4, MIT) to avoid the cold COM cost [inferred] |
| Office MRU in the registry | 6 ms [ran] | Office only, with timestamps | Rust (`windows-registry`) |
| App-native recents | Photoshop `app.recentFiles`; Resolve projects through `ProjectManager` | Exact for that app | Through each app's adapter |
| Windows Search (`Search.CollatorDSO` through ADODB) | Rust: open 42 to 250 ms, query and fetch p50 410 to 1007 ms. Python ADO: p50 401 to 749 ms [ran] | Only indexed locations; a `.mp4` query found 3 files while the Recent folder pointed at 20 [ran] | Rust works through the same `IDispatch` helper [ran]; latency is the indexer's |
| Everything 1.5 | Not installed (no `Everything.exe`, no `es.exe`) [ran 10-20] | Whole NTFS volumes | Named pipe `\\.\PIPE\Everything IPC`, reachable from Rust [secondary, voidtools forum]; optional |
| Eileen's own watcher | Not built | Files the user saves, as they happen | `ReadDirectoryChangesW` on user folders, in Rust [inferred] |

Recommendation: the Rust core keeps a small in-memory recent-work index fed by jump lists for known AppIDs, the Recent folder, Office MRU, adapter recents and a watcher, refreshed off the hot path. A spoken "the beach image I was editing" then ranks a few dozen candidates in microseconds; Windows Search is called only when the index has nothing. Windows 11's semantic file search needs a Copilot+ NPU, and this machine's NPU is not in that class [inferred], so content matching is Eileen's job (thumbnails plus a model).

Privacy: the probe printed counts and extensions only. These sources reveal what the user works on, so the index stays local, encrypted at rest with the rest of Eileen's state, and never enters a prompt beyond the few candidates being ranked.

## 8. Where the language boundaries fall

| Rust calls it directly | Needs another language, and why |
|---|---|
| SMTC (`windows::Media::Control`), read and control | Resolve: Lua inside `fuscript.exe` (or Python with `fusionscript.dll`), because the only API is a scripting module with an undocumented wire protocol. On 21.1+, `ResolveMCP.exe` runs Blackmagic's own Python; Rust talks MCP to it |
| Any OLE Automation target: Word, Excel, Shell.Application, ADODB for Windows Search, Photoshop COM with ExtendScript strings | Photoshop and Premiere UXP: JavaScript running inside the host app, which can only connect out to the Rust core (or, in Premiere 26.2+, through a native Hybrid module) |
| Jump lists, shell links, Recent folder, registry MRU, `ShellExecute` URLs, media keys through `SendInput` | VS Code editor state: a TypeScript extension inside VS Code |
| UI Automation (`uiautomation` 0.25.1, Apache-2.0) and Win32 window state | Playwright MCP: a Node sidecar (third-party), until a Rust CDP path replaces it |
| The loopback WebSocket server for UXP plugins, the stdio supervisor for the Resolve bridge, MCP servers and clients through `rmcp` (3.6.0 on 2026-10-20, Apache-2.0) | |

Measured on this machine, idle [ran], process start to exit with no work, 12 runs:

| Process | p50 | min |
|---|---|---|
| Rust probe (313 KB) | 83 ms | 73 ms |
| `python -I -c pass` | 204 ms | 180 ms |
| Python plus `import win32com.client` | 431 ms | 383 ms |
| Python plus the winrt SMTC import | 675 ms | 443 ms |

Honest reading: for long-lived adapters, startup matters once; per-call latency is set by the target (Word 10 ms, Windows Search about half a second, YouTube over the network about a second). Under load the picture sharpens [ran 10-20]: process starts and fresh system-object requests degrade by one to three orders of magnitude (yt-dlp start 1.1 s idle against 106 s loaded; `fuscript.exe` spawn 0.6 s against up to 9 s; SMTC request p95 76 ms against 2.2 s), while calls on handles a resident process already holds do not. Rust's real wins are no interpreter to install or keep warm, one native process that owns COM apartments, message pumps, event subscriptions and timeouts, and a Resolve path without Python. Design every adapter as resident and event-fed; never spawn per utterance.

## 9. How this maps to the roadmap

- **M2 (action layer)**: the Office, Explorer and SMTC acceptance criteria are shown to work from Rust on this machine (selection, active cell, selected files, session read and control, including a YouTube tab). Build them as one Rust `eileen-apps` process with a thread per COM target, call timeouts, event subscriptions for SMTC, and `rmcp` as its MCP face. Add the recent-work index here; the voice demo will lean on it.
- **M2 or M6, proposed**: a minimal Resolve adapter (`hello`, `timeline_info`, `goto_tc`, `current_item`, `export_still`, `marker`) through the Lua bridge, with the process-alive gate in front of it. The owner's first example is a Resolve one, and these six calls are cheap.
- **v0.2**: full Resolve (DRX application, takes, Fusion templates, render), the Premiere UXP plugin, the Photoshop COM adapter plus UXP plugin, the VS Code extension. Detect `ResolveMCP.exe` (21.1+) and register it with the Claude and Codex workers; measure it against the Lua bridge for Eileen's own calls.
- **Decisions to record** (proposals, not made here): Resolve through `fuscript.exe` Lua, Python only as fallback, `ResolveMCP.exe` preferred for workers on 21.1+; Adobe apps through Eileen-owned UXP plugins connecting out to the core; YouTube search only in the user's own browser; all adapters resident and event-fed.

## 10. Pitfalls (consolidated)

1. Resolve: 4 to 4.6 s stall per call when not running, not cached by the bridge; Local scripting setting; no node, OFX, window, tracker, play or blade API; `SetCurrentTimecode` refused on Media and Fusion pages and for sub-start timecodes, both with a bare `False`; Fusion writes under `Comp.Lock()` may not render [secondary]; Magic Mask v2 needs clicks.
2. Resolve 21.1 moved Python to Studio and may tighten again; keep the adapter on the documented API and on Lua. Preset writes before a project's first save can destroy it on 21.1 [secondary].
3. Office: ROT registration after focus loss; one instance per ProgID; modal dialogs block; `CLSCTX_LOCAL_SERVER`, not `CLSCTX_ALL`; Excel lingers after `Quit`.
4. First-run shell link resolution took 30 s here; never do it on the voice path.
5. Windows Search is slow and its scope is partial.
6. SMTC: apps decide what they publish; the test session reported position 0 until a seek; Python's session appeared under an Anaconda AUMID; a fresh manager request per read reaches 2 s p95 under load; the status returned with an action is the pre-action one; the published position is a snapshot.
7. YouTube: robots.txt disallows headless search; yt-dlp takes seconds idle and minutes under load; preroll and mid-roll ads can follow a load or a seek, and Chrome's SMTC session vanishes during an ad.
8. Claude in Chrome is only for the Claude worker, needs a `/login` session on a direct plan, is enabled per user (never by project settings), and with several browsers connected it asks the user to pick one before acting, so Eileen must surface that choice by voice or remember it.
9. Chrome DevTools MCP needs a user toggle and per-session consent, and is Chrome only. Launching Chrome with `--remote-debugging-port` works only with a non-default `--user-data-dir` (Chrome 136+) [primary, Chrome blog 2025-03-17; consistent with the scratch-profile runs here]. Eileen must never relaunch the user's own profile with a debug port; use the extension routes for the real profile.
10. UXP plugins are clients only, load through a developer tool until packaged, and the loopback token is readable by any process of the same user.
11. MCP server licenses: photoshop-mcp-imt is non-commercial; alisaitteke/photoshop-mcp sends analytics by default.
12. Explorer tabs on Windows 11 make "the foreground selection" ambiguous.
13. Generative Fill and cloud Neural Filters spend Adobe credits: tier 2.
14. Load: per-utterance process spawns are unpredictable on a busy machine (section 8).

## 11. Open risks

- Resolve API call latency is unmeasured here (Resolve was closed on both dates). Measure `SetCurrentTimecode`, `GetCurrentVideoItem` and `ExportCurrentFrameAsStill` the next time Resolve is open.
- Whether a DRX carrying a Face Refinement node arrives analyzed, or always needs Analyze pressed in the UI. Whether 21.1's keyboard-preset API can bind Analyze to a key.
- `ResolveMCP.exe`: its exact tool list, latency, whether it needs External scripting = Local, and whether `run_script` is exposed to every client. Only secondary sources so far.
- Everything about Photoshop is untested on this machine, including its AppUserModelID for jump lists and whether Neural Filters can be driven at all.
- The Premiere UXP plugin's install path for ordinary users, and whether Adobe keeps allowing loopback connections from plugins.
- Claude in Chrome in `claude -p` mode, and how the worker behaves when several browsers are connected and none is selected.
- The YouTube checks ran in a scratch profile with no sign-in. A signed-in or Premium profile (fewer or no ads), Edge, and the Playwright extension route on the real profile were not tested.
- Blackmagic and Adobe could gate scripting further (21.1 is the precedent).

## 12. What was run

All scratch work lived in this session's scratchpad (`scratchpad/app-hooks/`, with the 10-20 additions under `refresh-1020/`) and is not checked in.

2026-10-11:

- Read Resolve's shipped `README.txt` and `CHANGELOG.txt` in full for the object model; `dumpbin /exports` and `/dependents` on `fusionscript.dll`; `fuscript.exe` usage strings; Lua probes for `bmd` helpers; the bridge smoke test and a 200-request stdio round-trip test from Python.
- `uv venv` on Python 3.13.5 with `yt-dlp` 2026.08.19, `winrt-*` 3.2.1, `pywin32` 312, `comtypes` 1.4.17. Scripts: SMTC reader, a silent-WAV SMTC player, an SMTC controller, an ADO Windows Search comparison.
- `hookprobe`, a Rust 1.99.0 binary on `windows` 0.62.2 with subcommands `smtc`, `recent`, `jumplist`, `explorer`, `search`, `office-attach` and `office-selftest`. `office-selftest` started hidden Word and Excel, worked in new unsaved documents and quit them.
- Read-only registry and file-system checks for ProgIDs, Premiere's install layout, native messaging hosts, Office MRU counts and jump-list files.
- `npm view` for package versions; `code --help`; `curl` for YouTube's results page and robots.txt.

2026-10-20 (machine under 90 to 100 percent CPU from other workloads):

- Re-checked installed versions (Resolve, Premiere, Chrome, VS Code, Windows Terminal), ProgIDs, native messaging host names, `ResolveMCP.exe` and Everything presence; confirmed Resolve, Photoshop, Premiere and Office were not running.
- Added to `hookprobe`: `smtc-cached` (reads from a held manager), `smtc-control <title> read|pause|play|seek=<s>` (acts only on the session whose title matches, prints the AUMID only as a yes or no for "chrome"), and `resolve-bridge` (spawns `fuscript.exe` with the Lua bridge and measures 500 pings, two `hello` calls and an unknown method). Rebuilt and ran clippy clean.
- Rust SMTC control against the silent-WAV test session from 10-11 (Python `MediaPlayer` at volume 0).
- A scratch Chrome 155 with its own `--user-data-dir`, `--mute-audio` and a debug port, opened on one public YouTube match video with `&t=930s`; a Node CDP script (`yt_smtc_cdp3.mjs`) read the `<video>` element, called the Rust probe for SMTC read, pause, seek and play, timed DOM seeks, then closed that browser. Two earlier attempts were spoiled by load and a preroll ad and are not counted.
- yt-dlp `--version` and a flat `ytsearch3:`; one `curl` of a YouTube results page.
- One `claude --version` and one `claude -p` attempt (failed on an expired session; no login attempted). One `list_connected_browsers` call from this session; no browser actions.
- `npm view`, crates.io and PyPI lookups for current versions; web reading listed below.
- Not done, by rule: launching Resolve, modifying any Resolve project, reading vendor credential files, any login flow, touching the user's own browser profile.

## Sources

Local, installed with the apps (read 2026-10-11, re-checked 2026-10-20):

- DaVinci Resolve scripting README and CHANGELOG: `C:\ProgramData\Blackmagic Design\DaVinci Resolve\Support\Developer\Scripting\`

Resolve:

- [samuelgursky/davinci-resolve-mcp](https://github.com/samuelgursky/davinci-resolve-mcp) and its [CHANGELOG](https://github.com/samuelgursky/davinci-resolve-mcp/blob/main/CHANGELOG.md) (read 2026-10-20 at v4.14.4)
- [Blackmagic: About DaVinci Resolve 21 (21.1 readme, 2026-09-08)](https://www.blackmagicdesign.com/support/content/readme/baf7c071c0524fbf8ccc961925c9f443) and [an earlier 21.x readme](https://www.blackmagicdesign.com/support/content/readme/59dd4eef1f4941c29fb8dc48b33f5c87)
- [Digital Production: DaVinci Resolve 21.1.1 adds async APIs (2026-10-06)](https://digitalproduction.com/2026/10/06/davinci-resolve-21-1-1/)
- [No Film School: DaVinci Resolve 21.1.1](https://nofilmschool.com/davinci-resolve-update-21-1-1)
- [Digital Production: Resolve 21.1 adds MCP](https://digitalproduction.com/2026/09/08/resolve-21-1-adds-mcp-and-finally-gets-presets/)
- [Digital Production: 21.0.4 scripting additions](https://digitalproduction.com/2026/08/25/blackmagic-adds-script-hooks-and-pre-record/)
- [Voyager: DaVinci Resolve MCP, Studio setup and community servers](https://voyager.so/blog/davinci-resolve-mcp) (secondary; source for `ResolveMCP.exe`, stdio and tool names)
- [explainx: Resolve 21.1 MCP server](https://www.explainx.ai/blog/davinci-resolve-21-1-mcp-server-ai-agents-2026) (secondary)
- [Puget Systems: Resolve Free 21.1 scripting changes](https://www.pugetsystems.com/blog/2026/09/10/how-davinci-resolve-free-v21-1-scripting-changes-affect-puget-bench/)
- [Y.M.Cinema: Resolve 21.1 MCP](https://ymcinema.com/2026/09/17/davinci-resolve-21-1-mcp-ai-assistants-control-edit/)
- [Bottle Rocket: connecting Claude to Resolve](https://www.bottlerocketcontent.com/how-to-connect-claude-davinci-resolve/)
- [PilotCut: Resolve 21.1 MCP and API gaps](https://www.pilotcut.com/blog/davinci-resolve-studio-21-1-mcp-api-gaps-vs-pilotcut) (vendor blog)
- [UT Austin: applying Face Refinement](https://cloud.wikis.utexas.edu/wiki/spaces/comm/pages/33424871/DaVinci+Resolve+-+How+to+apply+Face+Refinement+Studio+Only)
- [OpenCV Zoo: YuNet face detection](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)

Adobe:

- [Adobe developer blog: UXP comes to the flagship apps, CEP timeline](https://blog.developer.adobe.com/en/publish/2026/09/investing-in-the-future-of-creative-cloud-extensibility-uxp-comes-to-our-flagship-applications)
- [Premiere UXP changelog](https://developer.adobe.com/premiere-pro/uxp/changelog) and [CC developer forum: Premiere 26.5 API updates (2026-09-10)](https://forums.creativeclouddeveloper.com/t/premiere-26-5-api-updates/12159)
- [Premiere UXP reference: Sequence](https://developer.adobe.com/premiere-pro/uxp/ppro_reference/classes/sequence/), [TickTime](https://developer.adobe.com/premiere-pro/uxp/ppro_reference/classes/ticktime/), [Project](https://developer.adobe.com/premiere-pro/uxp/ppro_reference/classes/project/), [VideoFilterFactory](https://developer.adobe.com/premiere-pro/uxp/ppro_reference/classes/videofilterfactory/), [ObjectMaskUtils](https://developer.adobe.com/premiere-pro/uxp/ppro-reference/classes/objectmaskutils.md)
- [Premiere Hybrid Plugins](https://developer.adobe.com/premiere-pro/uxp/plugins/hybrid-plugins) and [Adobe blog: UXP Hybrid Plugins now available for Premiere](https://blog.developer.adobe.com/en/publish/2026/04/uxp-hybrid-plugins-now-available-for-premiere)
- [Premiere Pro Scripting Guide](https://ppro-scripting.docsforadobe.dev/)
- [CC developer forum: stale getPlayerPosition](https://forums.creativeclouddeveloper.com/t/uxp-getplayerposition-returns-wrong-stale-timecode-until-sequence-is-switched/11766)
- [CC developer forum: Generative Fill from UXP](https://forums.creativeclouddeveloper.com/t/can-i-use-generative-fill-in-uxp-photoshop-plugin/11638)
- [UXP for CEP developers](https://developer.adobe.com/photoshop/uxp/2022/guides/uxp-for-you/uxp-for-cep-devs/)
- [Adobe community: reopening recent files by script](https://community.adobe.com/questions-712/script-to-open-the-last-closed-file-1088506)
- [Adobe help: Neural Filters list and FAQ](https://helpx.adobe.com/photoshop/using/neural-filters-list-and-faq.html)
- [mikechambers/adb-mcp](https://github.com/mikechambers/adb-mcp), [alisaitteke/photoshop-mcp](https://github.com/alisaitteke/photoshop-mcp), [photoshop-mcp-imt](https://github.com/Immersive-Media-Technologies/photoshop-mcp-imt), [ayushozha/AdobePremiereProMCP](https://github.com/ayushozha/AdobePremiereProMCP)
- [Carly: Adobe's official MCP connector](https://www.usecarly.com/blog/adobe-mcp/) (secondary)

Browser:

- [Claude Code: use Claude Code with Chrome](https://code.claude.com/docs/en/chrome) (read 2026-10-20)
- [microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp)
- [ChromeDevTools/chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp), its [releases](https://github.com/ChromeDevTools/chrome-devtools-mcp/releases), [configuration](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/configuration.md) and [tool reference](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/tool-reference.md)
- [Chrome blog: let your coding agent debug your browser session](https://developer.chrome.com/blog/chrome-devtools-mcp-debug-your-browser-session)
- [Chrome blog: changes to remote debugging switches (2025-03-17)](https://developer.chrome.com/blog/remote-debugging-port)
- [YouTube robots.txt](https://www.youtube.com/robots.txt)

Windows:

- [Microsoft KB 238610: GetObject or GetActiveObject cannot find a running Office application](https://support.microsoft.com/kb/238610)
- [Windows Terminal command-line arguments](https://learn.microsoft.com/windows/terminal/command-line-arguments)
- [voidtools: Everything 1.5 SDK](https://www.voidtools.com/forum/viewtopic.php?p=75613) and [IPC](https://www.voidtools.com/support/everything/sdk/ipc)
- [microsoft/windows-rs releases](https://github.com/microsoft/windows-rs/releases)
- API references used for the probe: [GlobalSystemMediaTransportControlsSessionManager](https://learn.microsoft.com/uwp/api/windows.media.control.globalsystemmediatransportcontrolssessionmanager), [GlobalSystemMediaTransportControlsSession](https://learn.microsoft.com/uwp/api/windows.media.control.globalsystemmediatransportcontrolssession), [IApplicationDocumentLists](https://learn.microsoft.com/windows/win32/api/shobjidl_core/nn-shobjidl_core-iapplicationdocumentlists), [Windows Search SQL](https://learn.microsoft.com/windows/win32/search/-search-sql-windowssearch-entry)

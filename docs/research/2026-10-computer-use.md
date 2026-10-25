# Computer use: the PC control layer

Research passes on 2026-10-11 and 2026-10-20, written up on 2026-10-20. Question: how does Eileen let a Claude worker do anything a human can do on the PC (see the screen, read any window's controls, click, type, scroll, drag, press keys, launch any installed app, move windows), fast and safely, on the owner's subscription and without an API key?

The short answer is an Eileen MCP server written in Rust on `rmcp`. It gives the worker desktop grounding, the UI Automation tree with indexed elements, screenshots with set-of-mark overlays, input by element or by coordinates, fuzzy app launch and window management. The worker is `claude -p` on the owner's plan, and it drives the PC through these tools.

This report feeds [ARCHITECTURE.md section 2](../../ARCHITECTURE.md#2-desktopstate) (DesktopState), [section 3](../../ARCHITECTURE.md#3-action-layer-and-the-escalation-ladder) (the escalation ladder) and [section 7](../../ARCHITECTURE.md#7-permission-tiers) (permission tiers); decisions [D5](../DECISIONS.md#d5-cua-driver-is-the-windows-action-layer) and [D6](../DECISIONS.md#d6-our-own-desktopstate-grounding-service); and milestones M1 and M2 in [ROADMAP.md](../ROADMAP.md). It builds on [2026-10-subscription-harnesses.md](2026-10-subscription-harnesses.md) (how workers run and see MCP images), [2026-10-app-hooks.md](2026-10-app-hooks.md) (app scripting that beats clicking) and [2026-10-prior-art.md](2026-10-prior-art.md) (who else does this).

## How to read this

Every claim carries a tag saying how it was checked:

- **[ran 10-11]**: run or measured on the owner's PC on 2026-10-11, by the first earlier attempt. The machine was mostly idle and the owner had not touched the keyboard or mouse for 2 to 6 minutes.
- **[ran 10-20]**: run or measured on 2026-10-20 by the second earlier attempt. The machine was loaded the whole time: other agents were compiling Rust, free RAM ranged from 0.7 to 2.7 GB, and the owner was using the PC. Treat these numbers as "under pressure", not best case.
- **[ran, this pass]**: the one new measurement taken while writing this up (section 3.5).
- **[primary]**: a project's own README, release notes, tool descriptions, source, or vendor docs.
- **[secondary]**: blogs, press, search snippets.
- **[inferred]**: our reasoning from the above, not tested.

The machine: Windows 11 Home build 26200, Core Ultra 7 155H (Arc iGPU, NPU, no NVIDIA), 16 GB RAM, one 1920x1200 display at 144 DPI (150 percent scaling), Rust 1.99.0 MSVC.

**Provenance.** Two earlier attempts at this topic did nearly all of the work below (about 100 to 130 tool calls each). The first was stopped by a usage limit on 2026-10-11. The second ran out of memory on 2026-10-20 and the session ended. Neither wrote this report. Their commands, outputs and scratch code are condensed in the owner's local work folder, `Projects/eileen-workfiles/predecessor-notes/computer-use.md` and `Projects/eileen-workfiles/2026-10-scratch/computer-use/`, which are not in this repository. Unless a line says "this pass", every measurement here comes from those attempts. Section 12 lists what was run and where its output lives.

## 1. Verdict

1. **Build the control layer as Eileen's own Rust MCP server, and revise D5.** A Rust proof (`eileen-desk-proof`: 1,720 lines in 10 library modules plus a 389-line server binary) did the job with 12 tools in an 8.8 KB tool list. Cua Driver 0.37.0 exposes 63 tools in a 216 KB tool list, ships about 95 MB of binaries, and sends telemetry by default [ran 10-20]. On the same Win32 test window, both did background UIA actions correctly. The proof grounded faster: `list_windows` took 30 to 128 ms against Cua's 152 to 5,729 ms, and full screenshots took 232 to 787 ms against 799 to 2,442 ms [ran 10-20]. Cua stays useful as an MIT reference for techniques (MSAA fallback, PostMessage delivery, Electron drags), not as the default dependency. Section 9 has the proposed decision text.
2. **Background first, foreground only when needed, and honest when Windows says no.** UIA patterns worked without touching focus or the cursor in both servers [ran 10-20]: Invoke, Value with 60 lines of text, and Value-based typing. Every `SendInput` path in the proof was refused on 10-20, because Windows would not hand the foreground to a background process while the owner was active ("Windows refused to bring the target window forward; no input was sent"). On 10-11, with the owner idle past the foreground-lock timeout, the same code took the foreground in 0.4 ms [ran 10-11]. Section 4 turns this into rules.
3. **Typing with synthetic key events is unreliable on modern apps; set the value instead.** In Windows 11 Notepad, unpaced `SendInput` text was garbled in every variant. Only Unicode events paced at 30 ms per character arrived intact, which took 2.07 s for about 65 characters. Virtual-key typing dropped Shift state, so `:` came out as `;` [ran 10-11]. `ValuePattern.SetValue` wrote the same text in 110 ms in the background [ran 10-11]. Text entry order: Value or Text pattern, then clipboard paste, then paced Unicode keys with a read-back check.
4. **Chromium and Electron need the browser rung for content and the foreground for input.** Against an Edge page this research launched, Cua's UIA tree held 6 elements, all browser chrome; none of the page's textarea, password field, button or slider appeared [ran 10-20]. Cua refused background text and key combos for the `Chrome_WidgetWin_1` window class and asked for foreground delivery [ran 10-20]. That class also covers Chrome, Edge, VS Code and the Claude desktop app itself [ran 10-20].
5. **DPI is a correctness issue, not polish.** This PC reports 1280x800 to DPI-unaware code and 1920x1200 to per-monitor-v2 code [ran 10-11]. The proof declares per-monitor-v2 at startup, keeps every rect in physical virtual-screen pixels, captures windows at exactly their DWM frame rect (checked: true for Notepad and Explorer), and returns the image-to-screen mapping with every screenshot (`screen = origin + image_px * 1.5`) [ran 10-11, 10-20]. More than one monitor has not been tested (section 11).
6. **Claude on the owner's plan can see Eileen's screenshots.** Claude Code 2.1.304, run headless on the owner's Max plan, called the proof server's `screenshot` tool and read the returned JPEG in 3 turns and 6.2 s [ran, this pass]. Together with the harness report's PNG probe, this closes the "computer use without an API key" question for Claude.
7. **Grounding is cheap; app indexing and cold starts are not.** Monitors take 0.01 ms, the window list 1 to 2 ms, a cached UIA snapshot 160 to 410 ms, a window capture about 50 ms, and JPEG encoding at 1280 px 14 to 18 ms [ran 10-11]. Enumerating installed apps takes 1.5 to 3.4 s idle [ran 10-11] and 14 to 41 s under load [ran 10-20], while matching a name against the index takes under 0.3 ms. Index once at startup and on change, never per request. Server cold start went from 0.4 s idle to 6 to 8 s under memory pressure [ran 10-11, 10-20], which argues for one long-lived desktop service in the runtime rather than one per worker (section 2.1).
8. **Safety rules belong in the server, not in the prompt.** The proof refused to read or write UIA password fields (refusal in 11.8 ms) and refused `Win+R` and seven other blocked combos (6.3 ms) [ran 10-20]. Cua 0.37.0 posted `Win+R` to the test window without objection [ran 10-20]. Its saved element records carry no password flag, and its tree did not mark the test window's password box [ran 10-20]. Cua has permission modes and capability manifests [primary], but those do not replace per-field and per-combo rules.

## 2. Recommended architecture

### 2.1 Process picture

```
mic / HUD / hotkeys
      |
eileen runtime (Rust, always on)
  |-- job registry, permission tiers, learning recorder
  |-- desktop service (Rust): UIA worker thread, capture, input lock, app index, window cache
  |        ^ named pipe, per-session token
  |        |
  |   eileen-desktop (thin stdio MCP shim, Rust, rmcp)  <-- one per worker session
  |        ^ stdio
  |        |
  +-- Claude worker: claude -p --mcp-config <generated> --strict-mcp-config --allowedTools mcp__eileen-desktop__*
```

- **One desktop service, many shims [inferred].** The proof ran as a stdio child of each client. Its start cost was 0.4 s idle and 6.1 to 8.3 s when memory was tight [ran 10-11, 10-20], and its app index took up to 41 s to build under load [ran 10-20]. Keep the UIA thread, the app index and the window cache warm inside the runtime. Give each worker a tiny stdio shim that forwards to the runtime over a named pipe, with a per-session token in the generated MCP config. This also gives one place to hold the input lock across parallel workers (section 4) and one place to record every tool call for the learning loop (section 8).
- **No persistent registration.** Workers get a generated config through `--mcp-config` and `--strict-mcp-config`, which is how this pass ran (section 3.5). Never `claude mcp add`, which writes into the owner's `~/.claude.json` [primary, [Claude Code MCP docs](https://code.claude.com/docs/en/mcp)].
- **Server name.** Use `eileen-desktop`, as ARCHITECTURE.md section 2 already says. The proof used `eileen-desk`.
- **The brain does not drive the mouse.** Computer-use jobs go to worker sessions, as the harness report specifies. The brain only dispatches and narrates.

### 2.2 What is Eileen's own code and what is reused

| Piece | Choice | License | Why |
|---|---|---|---|
| MCP protocol | [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) 3.6.0 (released 2026-10-20; proof built on 3.5.1) | Apache-2.0 | Official Rust SDK; `#[tool_router]` macros; returned images to Claude Code [ran, this pass] |
| Win32, COM, UIA, DWM, input bindings | [`windows`](https://github.com/microsoft/windows-rs) 0.62.2 | MIT OR Apache-2.0 | Microsoft's own bindings; the proof calls UIA directly so that it controls threading |
| UIA snapshot and patterns | **Own code** (proof `uia.rs`): one `IUIAutomationCacheRequest` per snapshot on a dedicated MTA thread, with a timeout on every call | Eileen (MIT) | 2.0 to 2.4 times faster than a naive walker [ran 10-11]. Index assignment, password marking and pruning are product logic |
| UIA convenience crate | [`uiautomation`](https://github.com/leexgone/uiautomation-rs) 0.25.1, evaluated, not used | Apache-2.0 | Fine wrapper with cache-request calls, but `UIAutomation::new()` calls `CoInitializeEx` on every construction [primary, source]; we want one owned MTA thread |
| Screen and window capture | **Own code**: GDI `BitBlt` for screen and region, `PrintWindow` with `PW_RENDERFULLCONTENT` for covered windows | Eileen | 50 to 75 ms, comparable to `xcap` [ran 10-11] |
| Continuous capture (watch mode, HUD, change detection) | [`windows-capture`](https://github.com/NiiightmareXD/windows-capture) 2.0.1 (DXGI duplication, Windows.Graphics.Capture) | MIT | Steady DXGI frames in 16.5 ms p50 [ran 10-11]; too slow to start for one-shot screenshots |
| `xcap` | Evaluated, not needed | Apache-2.0 | Monitor 65 ms, window 117 to 184 ms [ran 10-11]; no gain over own code |
| Resize and encode | [`fast_image_resize`](https://crates.io/crates/fast_image_resize) 6.1.0, [`jpeg-encoder`](https://crates.io/crates/jpeg-encoder) 0.7.1, [`image`](https://crates.io/crates/image) 0.25.10 (PNG) | MIT OR Apache-2.0; jpeg-encoder is `(MIT OR Apache-2.0) AND IJG` | IJG needs an attribution line and a cargo-deny allow entry |
| Fuzzy app match | [`nucleo-matcher`](https://crates.io/crates/nucleo-matcher) 0.3.1 | **MPL-2.0** | Works (under 0.3 ms per query). MPL-2.0 must be allowed in `deny.toml`, or swap in a small own scorer |
| App index and launch | **Own code** (proof `apps.rs`): `shell:AppsFolder` enumeration, then launch by AUMID through `IApplicationActivationManager` or by known-folder path | Eileen | Same list Start shows: Win32, Store and PWAs; 222 apps here [ran 10-11] |
| Input | **Own code** (proof `input.rs`): `SendInput` with absolute virtual-desk coordinates, cursor restore, a foreground guard before every event, and the blocked-combo list | Eileen | `enigo` 0.6.1 (MIT) was looked at; it adds nothing over 300 lines of `SendInput` and hides the guards we need [inferred] |
| Set-of-mark overlay | **Own code** (proof `som.rs`) | Eileen | Draw cost 2.1 to 3.3 ms [ran 10-11] |
| Background PostMessage delivery, MSAA fallback for VCL and SAL apps, Electron drags | **Study [Cua Driver](https://github.com/trycua/cua)** and port with attribution where needed | MIT | Its [Windows design write-up](https://cua.ai/blog/inside-windows-computer-use) [primary] and source are the best public reference |
| Vision fallback (rung 6) | The worker's own model looking at Eileen screenshots with marks | n/a | No OmniParser in v0.1: its repo shows both CC-BY-4.0 and MIT labels, and its older detectors carry AGPL [primary, [README](https://github.com/microsoft/OmniParser)] |

Reference projects, all read and none taken as dependencies: [Windows-MCP](https://github.com/CursorTouch/Windows-MCP) 0.8.8 (MIT, Python 3.14 or newer, 21 tools) [primary]; [terminator](https://github.com/mediar-ai/terminator) (MIT on GitHub, but "non-standard" license metadata on crates.io; last release 2026-04-05) [primary]; [Microsoft UFO](https://github.com/microsoft/UFO) (MIT, Python, tags at v3.0.10 on 2026-09-22) [primary]; [FlaUI](https://github.com/FlaUI/FlaUI) (MIT, .NET) [primary].

### 2.3 Tool surface for v0.1

Keep the list under about 16 tools and under about 15 KB. Each worker session pays for the tool list in context. Cua's 216 KB list is roughly 50,000 tokens before the first action [inferred, at about 4 bytes per token].

Use snake_case names. The dotted names in ARCHITECTURE.md section 2 (`desktop.state`, `window.move`) should be renamed: Claude Code exposes tools as `mcp__<server>__<tool>`, and Anthropic tool names allow only letters, digits, `_` and `-` [inferred from the Anthropic tool-name rule; check before freezing names].

| Tool | Proof status | Notes |
|---|---|---|
| `desktop_state` | Not in proof (D6, M1) | Foreground window, focused element, selection, monitors, focus history |
| `list_windows` | Built | Monitors plus visible top-level windows in z-order, DWM frame rects, class, process, minimized flag |
| `uia_tree` | Built | Indexed actionable elements; indices valid until the next call; password fields marked and never read. Add `query`, `max_depth`, `max_elements` as Cua has |
| `screenshot` | Built | Screen, region or window; covered windows work; `marks` draws indices; returns JPEG plus the mapping line. Text and image content only, never `structuredContent`, because Claude Code dropped images when both were present (see the harness report) |
| `click_element` | Built | UIA Invoke, Toggle, SelectionItem or ExpandCollapse; falls back to a real click at the element's center |
| `set_value` | Built | ValuePattern; refuses password fields |
| `type_text` | Built | Real key events; takes the foreground first, refuses when it cannot, re-checks before every character, refuses password fields |
| `press_keys` | Built | Blocked-combo list enforced |
| `click`, `scroll`, `drag` | Built | Physical virtual-screen pixels; cursor restored after |
| `window` | Built | Focus, minimize, maximize, restore, move (visible-frame rect). Add `snap` from ARCHITECTURE.md |
| `launch_app` | Built | Fuzzy name, `dry_run`, returns the pid |
| `wait_for` | Not built | Poll for a UIA condition, window or title instead of fixed sleeps (Windows-MCP has `WaitFor`) |
| `selection_get` | Not built | From D6: TextPattern first, then a clipboard sequence-number fallback |

### 2.4 The order Eileen tries things for one action

This refines rungs 3 and 4 of the ladder in ARCHITECTURE.md section 3. Rungs 1, 2 and 5 (App Actions, scripting, browser DOM) come first whenever they apply; see the app-hooks report.

1. **UIA pattern on an indexed element** (Invoke, Toggle, SelectionItem, ExpandCollapse, Value, RangeValue, Scroll). Background, no focus change, no cursor movement [ran 10-20, both servers].
2. **For text:** `ValuePattern.SetValue` when replacing the whole value is acceptable. Otherwise focus the element and paste through the clipboard, saving and restoring the user's clipboard. Use paced Unicode keys only as a last resort, and always read the value back [ran 10-11 for SetValue and paced keys; clipboard path inferred].
3. **Coordinates, key combos, drags, wheel:** `SendInput`, which needs the foreground. Escalate under the rules in section 4.
4. **Vision:** a screenshot with marks, the model picks a point, then step 3 runs with a post-condition check.

Every action returns a verification hint (the value read back, a title change, or the new focused element), so the worker and the learning recorder know whether the action worked, not only whether the call returned.

## 3. Measurements

### 3.1 In-process benchmark, 2026-10-11 (mostly idle)

`bench.exe` from the proof, 1920x1200 at 144 DPI. Times in ms; "run 2" is a second pass the same morning.

| Operation | p50 | First call | Notes |
|---|---|---|---|
| Enumerate monitors | 0.01 | 0.15 | |
| Enumerate windows (10) | 2.25 (run 2: 0.98) | 10.5 | |
| Full screen, GDI `BitBlt` (own) | 75 (68) | 157 | |
| Full screen, `xcap` | 65 (77) | 66 | |
| Full screen, DXGI duplication, create plus first frame | 176 (166) | 394 | |
| Full screen, DXGI steady acquire | 16.5 (16.8) | 20 | 20 of 20 new frames; an idle desktop times out instead |
| Full screen, Windows.Graphics.Capture start, frame, stop | 367 (311) | 512 | |
| Downscale 1920 to 1280 long edge | 23 (16) | 28 | bilinear |
| JPEG q80 at 1280 | 18 (14) | 16 | 114 to 125 KB |
| PNG fast at 1280 | 14 (11) | 15 | 378 to 467 KB |
| Window capture, `PrintWindow` full content, Notepad 1426x829 | 51 | 81 | Origin equals DWM frame rect |
| Window capture, `PrintWindow`, Explorer 1059x636 | 50 | 52 | Origin equals DWM frame rect |
| Window capture, `xcap` | 184 / 117 | | Notepad / Explorer |
| Window capture, WGC | 380 / 356 | | Notepad / Explorer |
| UIA snapshot, one cache request, Notepad (58 nodes, 43 indexed) | 193 | 820 | |
| UIA naive walker, 5 properties per node, Notepad | 382 | 398 | |
| UIA snapshot, one cache request, Explorer (168 nodes, 84 indexed) | 409 | 584 | |
| UIA naive walker, Explorer | 973 | 1004 | |
| Set-of-mark overlay draw | 2.5 to 2.7 | 2.9 to 3.3 | |
| `shell:AppsFolder` enumeration (222 apps) | 1,868 to 3,091 | 2,346 to 3,357 | |
| Fuzzy app lookup | 0.03 to 0.3 | 7.2 once | |
| Notepad `ValuePattern.SetValue` | 110.5 | | Background, focus unchanged |
| Take foreground (owner idle) | 0.4 | | |
| `SendInput` 49 characters / one click with cursor restore / Ctrl+S | 18.6 / 23.2 / 1.5 | | Send time only; fidelity in section 3.4 |

A typical "look at this window" therefore costs about 50 ms capture, 15 ms resize and 15 ms encode, plus 160 to 410 ms if the tree is wanted too, before any model time.

### 3.2 Over MCP, 2026-10-11 (mostly idle)

Python MCP client 2.3.0 over stdio against each server, on the same Notepad window.

| Call | Eileen proof (9 tools then) | Cua Driver 0.34.1 |
|---|---|---|
| Spawn plus initialize | 412 ms | 400 to 885 ms |
| `tools/list` | 5.2 ms, 6.1 KB | 25 to 56 ms, 59 tools, 191 KB |
| List windows | 4 ms median (6 first) | 2,081 ms first (UIA desktop enumeration hit its 2 s limit and fell back to Win32), then 26 to 293 ms |
| Screenshot of the window | 56.5 ms, JPEG 37 KB | 860 ms first `get_window_state` with tree, PNG 64 KB |
| UIA tree | 115 ms median (166 first) | included above |
| Full desktop | | 166 to 276 ms, PNG 362 KB |
| List installed apps | (in-process, 1.9 to 3.1 s) | 8,163 ms (its UWP package query passed 4 s and was skipped) |

Cua behavior on 10-11 [ran 10-11]: it launched packaged Notepad by AUMID, which is needed because the System32 `notepad.exe` on Windows 11 is a stub that exits [primary, Cua tool description]. `set_value` and token-based `type_text` (routed through ValuePattern) both verified. `type_text` without a token went through PostMessage and reported "not verified". `press_key End` in background reported "not verified". `hotkey ctrl+s` against WinUI Notepad failed in both background and foreground modes: Cua found no UIA accelerator, and posted `WM_KEYDOWN` is ignored by XAML hosts.

### 3.3 End to end, 2026-10-20 (loaded)

Targets were windows the research launched itself. `testpad.exe` is a 160 KB Win32 window from the proof with an edit box, a password box, a Count button and a slider. Edge was launched with an isolated profile on a local test page. Both publish their own state in their title (`clicks=N slider=V line=L len=C`), and the harness (`desk_e2e.py`) read that state with `GetWindowTextW` to verify each action independently of the server under test. The Edge window was paged out and unresponsive when the Eileen run started, so the Eileen run covers testpad only.

| Action on testpad | Eileen proof (12 tools, 8.8 KB) | Cua Driver 0.37.0 (63 tools, 216 KB) |
|---|---|---|
| Spawn plus initialize | 6,108 to 8,298 ms (paged out) | not recorded |
| List windows | 128 ms, then 30 ms | 5,729 ms, then 152 ms |
| Full screenshot | 787 ms, then 232 ms (JPEG 135 KB at 1280 px; the server reported 520 ms capture inside 745 ms total once) | 2,442 ms, then 799 ms (PNG 568 KB) |
| Window screenshot | 249 ms (JPEG 24 KB) | 1,353 ms with tree (PNG 234 KB) |
| Screenshot with marks | 3,574 ms | n/a |
| UIA tree | 676 ms, then 313 ms (16 nodes, 10 indexed) | 580 ms, then 359 ms |
| Click Count by element | 1,124 ms, **verified** (UIA Invoke; the foreground did change) | 302 ms, **verified** (UIA Invoke, foreground unchanged) |
| Click Count by coordinates | refused: no foreground (401 ms) | failed: "UIA pixel click busy", no input sent (628 ms) |
| Set 60 lines of text | 2,166 ms, **verified**, foreground unchanged | 1,715 ms, **verified** |
| Type text | refused: no foreground (121 ms) | with token (ValuePattern): 1,384 ms, **verified**. Without token (PostMessage): 1,785 ms, not verified |
| Scroll up / down 5 | refused: no foreground | 345 ms verified / 3,152 ms not verified |
| Drag slider | refused: no foreground | 4,127 ms, posted, not verified |
| Set value into password box | **refused** by policy (11.8 ms) | not attempted: the password box was not identifiable in Cua's tree |
| `Win+R` | **refused** by policy (6.3 ms) | **posted** (5,544 ms) |
| Move window and move back | 10 ms and 8 ms | n/a |
| `launch_app` dry run: resolve, premiere, photoshop, calculator | Correct matches (DaVinci Resolve, Adobe Premiere Pro 2026, none, Calculator), but each call rebuilt the 221-app index: 14 to 41 s | n/a |

| Action on the Edge test page | Cua Driver 0.37.0 |
|---|---|
| Window state with image | 7,981 ms first |
| Tree | 949 to 1,125 ms, 6 elements, all browser chrome (title, caption buttons, address bar) |
| Type text | `background_unavailable` for `Chrome_WidgetWin_1`; retry with foreground suggested |
| Key combo | same |

The harness also recorded whether the cursor moved or the foreground changed around each call. The owner and other agents were using the PC during both runs, and the harness saw the cursor move even around read-only calls such as `list_windows`. Those two columns in `results.json` are therefore not evidence of background behavior on 10-20. The verification column is: it reads the target's own state.

### 3.4 Typing fidelity, 2026-10-11

Text sent to the owner's Notepad tab that the test had created, then read back through UIA. "Send" is the time `SendInput` took; "settled" is how long until the value stopped changing.

| Method | Send | Settled | Exact? | What arrived |
|---|---|---|---|---|
| Unicode events, one burst | 18 to 29 ms | 296 to 359 ms | no | first word, then a run of repeated `z` |
| Unicode, one event pair per call, no pause | 37 to 41 ms | 373 ms to 2 s | no | first word only, or the first word and a run of `z` |
| Unicode, 1 ms or 4 ms pause | 122 to 349 ms | 0.8 to 2 s | no | runs of spaces and `z` |
| Unicode, 10 ms pause | 746 to 753 ms | 0.1 to 2 s | no | repeated characters, dropped characters |
| **Unicode, 30 ms pause** | 2,074 ms | 1 ms | **yes** | |
| Virtual keys via `VkKeyScanW`, no pause | 68 ms | 453 ms | no | everything present but Shift lost (`;` for `:`) |
| Virtual keys, 2 ms or 10 ms pause | 214 to 773 ms | 2 s | no | dropped characters |
| `ValuePattern.SetValue` | 110 ms | | yes | |

Windows 11 Notepad is a WinUI app with a heavy text stack, so it is the hard case. Win32 edit controls are likely more forgiving [inferred]. The rule holds either way: never trust synthetic typing without reading the value back.

### 3.5 Claude Code reading an Eileen screenshot, this pass

Run on 2026-10-20 during this write-up. `claude auth status` reported `loggedIn: true, authMethod: claude.ai, subscriptionType: max` with the parent session's `CLAUDE*`, `ANTHROPIC*` and `MCP_*` variables removed. No login flow was run and no credential file was read.

```
claude -p --model haiku --mcp-config mcp-config.json --strict-mcp-config \
  --allowedTools mcp__eileen-desk__screenshot --output-format json \
  "Call the eileen-desk screenshot tool once with hwnd <testpad> ... answer button=; clicks=; slider="
```

Result: `is_error: false`, 3 turns, `duration_ms` 6,207, `duration_api_ms` 4,909, model `claude-haiku-5-5`. The answer read `clicks=0` correctly from the window's title bar in the image. It also reported that the button was cut off at the bottom edge and that no slider was visible. That was accurate: the testpad fixture lays out its lower controls past the bottom of its own window at 144 DPI, which the earlier UIA rects also show (window bottom at y=814, button at 789 to 837, slider at 864 to 924). The image reached the model, and the model read it rather than guessing. The testpad layout is a fixture bug to fix before the next end-to-end run.

## 4. Background input versus foreground escalation

What Windows allows, and what we saw:

- **UIA patterns need no focus.** Invoke, Value and Scroll worked on a background window from both servers [ran 10-20]. One caveat: invoking a Win32 button through UIA changed the foreground in the proof's run [ran 10-20]. The Win32 button proxy probably activates its window when clicked [inferred]. Do not promise "no focus change" for Invoke on Win32 buttons.
- **`SendInput` goes to whatever has the foreground**, so the target must be brought forward first. `SetForegroundWindow` from a background process is refused unless rules such as "the user has been idle past the foreground-lock timeout" or "this process received the last input event" apply [primary, [SetForegroundWindow remarks](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow), not refetched this pass]. On 10-11 the owner had been idle 144 to 355 s and the proof took the foreground in 0.4 ms. On 10-20 the owner was active and every attempt was refused, including the Alt-tap workaround in the proof [ran 10-11, 10-20].
- **PostMessage delivery is background but unverifiable for many targets.** Cua's posted typing, scroll-down and drag all came back "not verified" on 10-20. XAML, WinUI and UWP hosts ignore posted keyboard messages [ran 10-11, 10-20; primary, Cua tool descriptions].
- **Cua's foreground mode** is "a brief SetForegroundWindow swap + SendInput, restoring the prior foreground afterward", and its descriptions tell the agent to try background first and escalate only on a `background_unavailable` error [primary, Cua 0.37.0 tool descriptions]. Cua also ships a UIAccess worker (`cua-driver-uia.exe`) [ran 10-11, file listing]. UIAccess processes can drive higher-integrity windows and are exempt from some focus rules, but they must be signed and installed in a protected location such as Program Files [inferred, from Windows UIAccess requirements; not tested].

Rules for Eileen:

1. Try the background rung first (section 2.4). Escalate only when the server reports that the background path is impossible or a check shows it had no effect.
2. Before taking the foreground, check `GetLastInputInfo`. If the owner touched the keyboard or mouse in the last second or two, wait briefly, then ask through the HUD and by voice ("I need the Resolve window for a moment"). Never fight the owner for the keyboard [inferred; `GetLastInputInfo` read worked, ran 10-11].
3. When Windows refuses the foreground, say so and stop. Do not loop on workarounds. The proof's message ("Windows refused to bring the target window forward; no input was sent") is the right shape, because the worker can report it or ask the owner to click the window.
4. After a foreground action, restore the previous foreground window and the cursor. The proof restores the cursor today; restoring the foreground is not built yet.
5. Re-check the foreground before every synthetic key, as the proof's `guard` does. If focus moves mid-text, stop at once.
6. **One input lock across all workers** [inferred]. "Launch three agents" means three jobs, but only one may hold the foreground and `SendInput` at a time. Background UIA actions on different windows can run in parallel. The HUD shows who holds the lock.
7. Decide later whether to ship a signed UIAccess helper. It would make foreground escalation reliable and reach elevated windows, but it needs a code-signing certificate and an install under Program Files, which is a real cost for an open-source project (section 11).

## 5. Chromium and Electron

- **Web content is not in the UIA tree by default.** The Edge test page exposed only browser chrome to Cua's walker. None of its controls appeared, even though they all had labels [ran 10-20]. ARCHITECTURE.md already says Chrome builds web accessibility trees only after it notices an assistive-technology client; Chromium's own notes on [native UIA in Chromium](https://developer.chrome.com/blog/windows-uia-support-update) are the reference [secondary for this pass; search result only]. The second attempt wrote `chromium_probe.py` to measure how long until web content appears over UIA, but it never ran. It also needs fixing first, because it uses the old camelCase attributes (`structuredContent`, `isError`) that MCP client 2.3.0 renamed.
- **Use the browser rung for page content**: Playwright MCP with its extension, or Claude in Chrome inside the Claude worker (see the app-hooks report). Use UIA only for tabs, address bar, permission bubbles and dialogs. Cua warns that browser permission bubbles can be composited outside the window being captured [primary, Cua 0.37.0 `get_window_state` description], so take a desktop-scope screenshot when a window action has no effect.
- **Input to Chromium content needs the foreground.** Cua returned `background_unavailable` for text and key combos on `Chrome_WidgetWin_1` [ran 10-20]. Its newest release added "deliver background drags in Electron windows through the main process" [primary, [cua-driver-rs v0.37.0 notes](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.37.0)], which is worth porting later.
- **`Chrome_WidgetWin_1` is also the Claude desktop app, VS Code and Teams-style Electron apps** [ran 10-20, window list]. Never type into the Claude app's own window or Eileen's HUD. Exclude them by process, not by title.
- **A paged-out Chromium window can block the caller.** On 10-20, `ShowWindow` on the minimized Edge window, parked at (-32000, -32000), blocked for more than 60 s while Edge's UI thread was paged out [ran 10-20]. Use the async or timeout variants (`ShowWindowAsync`, `SendMessageTimeout`) and bound every cross-process call.
- **Big trees.** Cua caps walks at 250 elements by default and says Electron and large web apps produce trees of more than 10,000 elements [primary]. Keep `max_elements` and `max_depth` on `uia_tree`, and keep tree text under Claude Code's 50,000-character default for tool results. Claude Code writes larger results to disk unless the tool sets `_meta["anthropic/maxResultSizeChars"]`, which can go up to 500,000 [primary, [Claude Code MCP docs](https://code.claude.com/docs/en/mcp)]. The proof's trees were 0.9 to 7.4 KB [ran 10-11, 10-20].

## 6. DPI and multiple monitors

- **Per-monitor-v2 before anything else.** The proof calls `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` first thing. A DPI-unaware PowerShell query on this PC reported a 1280x800 screen; the aware process saw 1920x1200 at 144 DPI [ran 10-11]. Mixing the two silently shifts every click by 50 percent.
- **One coordinate system everywhere:** physical pixels in virtual-screen coordinates, origin at the primary monitor's top-left, negatives allowed (ARCHITECTURE.md section 2). UIA rects, DWM frames, `SendInput` and screenshots all use it in the proof [ran 10-11].
- **Visible frame, not window rect.** `GetWindowRect` includes invisible resize borders, about 9 px a side at 150 percent: testpad was 900x700 by window rect and 882x691 by DWM frame [ran 10-20]. Report and move by `DWMWA_EXTENDED_FRAME_BOUNDS`, as the proof's `window` tool does. Window captures came back exactly at the DWM frame [ran 10-11].
- **Every screenshot says how to map back.** Example reply: `image 1280x800 covers screen rect x=0 y=0 w=1920 h=1200; screen = origin + image_px * 1.5000` [ran 10-20]. The model can then click a pixel it saw without knowing the DPI. Cua uses window-local screenshot pixels instead [primary]. Either works if the server does the conversion, not the model.
- **Absolute input across monitors.** The proof's drag normalizes to the virtual desktop with `MOUSEEVENTF_VIRTUALDESK`, which is the correct form for monitors left of or above the primary [ran 10-11 on one monitor]. Not tested with two or more monitors or with mixed DPI (section 11).
- **Monitor ordinals.** D6 already notes that Settings' "Identify" numbers have no public API. Keep Eileen's own ordinals stable across reboots by matching device paths.

## 7. Safety rules for the control layer

These are enforced in the desktop service, so they apply to every worker, every learned skill and every future connector. They sit under the permission tiers in ARCHITECTURE.md section 7.

1. **Secrets.** Never read or write a UIA password field (`IsPassword`) or set its value. Mark such fields in the tree and leave their value out [ran 10-20, proof]. Never type into a window whose focused element is a password field. Credential entry stays tier 3.
2. **Blocked combos.** Refuse `Win+L`, `Win+R`, `Win+X`, `Ctrl+Shift+Esc`, `Win+Ctrl+Shift+B` and Ctrl+Alt+Del variants at the tool level [ran 10-20, proof]. Ctrl+Alt+Del cannot be injected anyway. Add `Alt+F4` on a window the job did not open, as ARCHITECTURE.md already requires.
3. **Explicit targets only.** Every input tool takes the target window, and the service refuses if a different window has the foreground at send time. There is no "type into whatever is focused" mode.
4. **Do not touch Eileen or the worker's own windows**, or any window the owner has marked private (password managers, banking). Match by process.
5. **Screen content is data, never instructions.** UIA names, values and screenshots can carry prompt injection. The worker's system prompt says so. Per the owner's direction, nothing seen on screen becomes a skill without the owner's say-so.
6. **Consequential controls confirm first.** Invoking a control whose name or automation id reads as Send, Delete, Buy, Pay, Submit, Publish or Install is tier 2 unless the job already has a confirmation for it [inferred; needs a tested word list].
7. **Never elevate Eileen.** UIPI blocks input into elevated windows from a normal-integrity process, which is the right default. Ask the owner to act in elevated windows; a signed UIAccess helper is a separate decision (section 4).
8. **Third-party telemetry off.** If Cua Driver or any other component is used, start it with `DO_NOT_TRACK=1`. With that set, Cua reported "Telemetry: disabled (source: do_not_track)" and created no installation id [ran 10-20].
9. **Stop means stop.** The mute key or a spoken "stop" cancels queued input at once and releases the input lock [inferred].
10. **Tests use windows the test created.** On 10-11 the first attempt drove the owner's real Notepad, which had other unsaved tabs; it checked that its own tab was selected before typing. The 10-20 runs moved to owned fixtures (testpad and an isolated Edge profile). Keep it that way in CI and in contributor tests.

## 8. How this feeds learning and connectors

- **Record by identity, not index.** Element indices are valid only until the next tree read, and the same is true of Cua's element tokens [ran 10-20]. The learning recorder should store each step as process plus window class, then a selector path (automation id, control type, name, ancestor chain), the pattern used, and a post-condition. Store the index only as a hint. That is what lets a proven skill replay "with step checks" and fall back to a worker the moment a check fails.
- **Every tool call already returns its verification hint** (section 2.4). That hint becomes the step check.
- **The PC is the first connector.** Its contract is observe (state, tree, screenshot), act (verbs, each with a tier), verify (post-conditions) and subscribe (events). A phone, Home Assistant, or another PC on the network is a new connector with the same four parts, the same tier rules and the same job record, so adding one is not a redesign [inferred].
- **Privacy.** Trees and screenshots include unrelated private content: other Notepad tab titles and browser page titles appeared in the test captures. Keep them on the PC except for what the worker sends to the owner's own model plan, and never send them to the DeepSeek route without asking (harness report rule).

## 9. Proposed change to D5

Proposed text for DECISIONS.md, for the owner to accept or reject:

> **D5 (revised 2026-10-20). Eileen's own Rust desktop server is the Windows action layer.** Supersedes the 2026-09-19 decision that Cua Driver is the action layer. Why: a Rust proof on `rmcp` and `windows` covered grounding, UIA actions, capture, input and app launch in about 2,100 lines, with a 12-tool, 8.8 KB tool list and faster grounding than Cua Driver 0.37.0 (63 tools, 216 KB, about 95 MB of binaries, telemetry on by default, and no password-field or blocked-combo refusals at the tool level). Owning the layer lets Eileen enforce safety rules, an input lock across parallel workers, and step-level recording for the learning loop in one place. Consequences: Cua Driver stays an MIT reference for MSAA fallback, PostMessage delivery and Electron drags; any component Eileen runs has telemetry disabled; ARCHITECTURE.md section 3 rungs 3, 4 and 6 name the Eileen desktop service instead of Cua Driver.

## 10. Pitfalls

- **Foreground lock.** Code that works while the owner is idle fails as soon as they move the mouse (section 4). Test both states.
- **DPI-unaware helpers.** Any helper process or script that is not per-monitor-v2 sees a 1280x800 screen here.
- **`HWND` is not `Send` in windows 0.62.** Pass `isize` across threads and rebuild the `HWND` on the UIA worker thread [ran 10-11].
- **Cargo features.** `IUIAutomationElement::GetCachedPropertyValue` and `VARIANT` need the `Win32_System_Ole` and `Win32_System_Variant` features; `VARIANT` is not in `windows::core` [ran 10-11].
- **rmcp macros and clippy.** `#[tool_handler]` trips `clippy::unused_async_trait_impl` under `-D warnings`. The proof uses `#[expect(..., reason = ...)]`. With `pedantic` on, clippy also flagged `chunks_exact_mut` with a constant size, missing doc backticks, `map().unwrap_or()`, a too-long `main` and unnecessary `Result` wrapping. All fixed; `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` were clean on 10-20 [ran 10-20].
- **A hung UIA provider stalls everything behind it.** The proof's UIA calls time out, but a worker that stays hung must be replaced. Cua logged the same issue ("no other UIA worker will start until this provider call returns") [ran 10-11].
- **The app index is slow; queries are fast.** Never rebuild per call (14 to 41 s under load). Watch the Start Menu folders and package events to refresh it.
- **Fuzzy ties.** "davinci" scored DaVinci Resolve and DaVinci Control Panels equally (192), and "word" ranked Windows Memory Diagnostic second [ran 10-11]. Launch only when the top match beats the next by a margin; otherwise ask.
- **Packaged built-ins.** Notepad, Calculator and Paint must be launched by AUMID; their System32 executables are stubs [primary, Cua; ran 10-11].
- **WinUI hotkeys.** Background hotkeys do not reach XAML hosts (Ctrl+S on Notepad failed in Cua) [ran 10-11]. Expect foreground for shortcuts in modern apps.
- **Images plus `structuredContent`.** Claude Code dropped image blocks when both were present (harness report). The proof returns text and image content only.
- **Cua telemetry registers on first run.** On 10-20 the 0.37.0 binary ran once (`--version`, help) before the opt-out was set, and `telemetry status` then showed telemetry enabled from the default, an installation id and "Registration recorded: true", against a PostHog EU endpoint compiled into the binary [ran 10-20; primary, source]. Set `DO_NOT_TRACK=1` before the first invocation, every time.
- **Test labels with colons.** The 10-20 harness named output files after labels such as `testpad:uia_tree`, which on NTFS creates alternate data streams. The `05-testpad` to `20-testpad` files look empty; their content is in the streams (`Get-Content <file> -Stream uia_tree.txt`). Use filesystem-safe labels.
- **MCP Python client 2.3.0** renamed result attributes to snake_case (`structured_content`, `is_error`, `mime_type`). `chromium_probe.py` still uses the old names.
- **Memory.** On 10-20 a release build of the small proof took 19 min 39 s while other agents compiled, and `ShowWindow` on a paged-out Edge blocked for over a minute. Build one crate at a time on this PC and never benchmark while a build runs.

## 11. Open risks and next measurements

1. **Foreground escalation while the owner is active** is the biggest open problem. Measure three options on the owner's PC: (a) the runtime takes the foreground right after the owner's own hotkey press, (b) an explicit "may I take the window" voice and HUD prompt, (c) a signed UIAccess helper, including what signing and installing it costs an open-source project.
2. **Multi-monitor and mixed DPI** are untested. Plug in a second display at a different scale and rerun the coordinate, drag and window-move checks, including a monitor left of the primary.
3. **Chromium time-to-tree.** Fix `chromium_probe.py` and measure how long after a UIA client attaches Chromium exposes web content, and whether foreground typing into a page field lands.
4. **Real apps.** All end-to-end action checks so far used a Win32 fixture and Notepad. Run the same suite against Explorer, Settings (WinUI), Office, VS Code (Electron), Resolve and Premiere (custom-drawn, mostly rung 2 and vision per the app-hooks report).
5. **Windows-MCP was set up but never run** (Python 3.14.7 venv in scratch); there is no measurement of it. Optional, as a third data point.
6. **No tests in the proof.** Port it into the repo as a crate (for example `eileen-desktop`) with unit tests for geometry, DPI mapping, combo parsing and the fuzzy-match margin, plus an opt-in desktop test suite against owned fixtures.
7. **Fix the testpad fixture** so its controls lie inside its window at any DPI.
8. **The Cua installation id** registered on 10-20 can be erased with `cua-driver telemetry reset-id` or by deleting the scratch copies; that is the owner's call.
9. **Windows' own agent platform.** Microsoft's agent workspace and on-device MCP registry for Windows 11 are moving [secondary, e.g. [Petri](https://petri.com/windows-11-agentic-computing-workspaces/)]. Recheck before v0.2 whether Eileen should register its desktop server there.

## 12. What was run

| When | What | Output (owner's local work folder `Projects/eileen-workfiles/2026-10-scratch/computer-use/`) |
|---|---|---|
| 10-11 | Crate and repo metadata for the candidate components (crates.io, GitHub, PyPI) | in the notes file |
| 10-11 | Cua Driver 0.34.1 Windows binary, SHA256 checked against the release checksums; `--help`, `describe` for core tools, telemetry status with opt-out | `cua-help.txt`, `cua-describe.txt` |
| 10-11 | Cua over MCP against the owner's Notepad (launch run and attach run) | `out-cua/`, `out-cua2/`, `cua_test.py` |
| 10-11 | Rust proof `eileen-desk-proof` built (rmcp 3.5.1, windows 0.62.2); in-process benchmark; typing fidelity | `proof/`, `out-bench/` |
| 10-11 | Proof server over MCP | `out-mcp-check/`, `mcpbench.py`, `steps-check.json` |
| 10-11 | UIA crate source read for the API shape | `uia_core.rs`, `uia_patterns.rs`, `uia_types.rs` |
| 10-20 | Metadata refresh: Cua 0.37.0, rmcp 3.6.0, Windows-MCP 0.8.8, UFO, terminator | `meta/` |
| 10-20 | Cua Driver 0.37.0 downloaded, SHA256 checked, `describe` for input tools | `meta/cua037-*.txt` |
| 10-20 | Proof extended (window tool, drag, password refusal, wider blocked list, testpad fixture), fmt and clippy clean | `proof/`, `proof-src-before/`, `bench_tail.rs`, `splice_bench.py` |
| 10-20 | Edge test page in an isolated profile | `web/eileen-chromium-test.html`, `edge-relaunch.ps1` |
| 10-20 | End-to-end runs, Eileen proof and Cua 0.37.0 | `out-e2e-eileen/`, `out-e2e-cua/` (each with `results.json` and `tools.json`), `desk_e2e.py` |
| 10-20 | Chromium probe written, not run | `chromium_probe.py` |
| 10-20, this pass | Claude Code 2.1.304 headless reads a proof screenshot | `cc-test/cc-run.json`, `cc-test/mcp-config.json` |

Cleanup checked in this pass: no `claude mcp add` registration exists. `~/.claude.json` has no MCP servers at user scope or under any project, there is no `.mcp.json` in the scratch folders, and this pass used `--mcp-config` with `--strict-mcp-config`. No test processes (testpad, the proof server, Cua, scratch Edge) were left running.

## 13. Sources

- Cua: [repository](https://github.com/trycua/cua), [cua-driver-rs v0.37.0 release](https://github.com/trycua/cua/releases/tag/cua-driver-rs-v0.37.0), [Windows skill guide](https://github.com/trycua/cua/blob/main/libs/cua-driver/rust/Skills/cua-driver/WINDOWS.md), [Inside Windows computer use](https://cua.ai/blog/inside-windows-computer-use) (2026-05-27), telemetry source `libs/cua-driver/rust/crates/cua-driver/src/telemetry.rs`.
- MCP and Claude Code: [rmcp / Rust SDK](https://github.com/modelcontextprotocol/rust-sdk), [rmcp on crates.io](https://crates.io/crates/rmcp), [Claude Code MCP docs](https://code.claude.com/docs/en/mcp).
- Windows bindings and capture: [windows-rs](https://github.com/microsoft/windows-rs), [uiautomation-rs](https://github.com/leexgone/uiautomation-rs), [windows-capture](https://github.com/NiiightmareXD/windows-capture), [xcap](https://github.com/nashaofu/xcap), [SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow).
- Other computer-use projects: [Windows-MCP](https://github.com/CursorTouch/Windows-MCP), [terminator](https://github.com/mediar-ai/terminator), [Microsoft UFO](https://github.com/microsoft/UFO), [FlaUI](https://github.com/FlaUI/FlaUI), [OmniParser](https://github.com/microsoft/OmniParser).
- Chromium: [Native UI Automation for Windows in Chromium](https://developer.chrome.com/blog/windows-uia-support-update).
- Windows agent platform: [Petri on Windows 11 agent workspaces](https://petri.com/windows-11-agentic-computing-workspaces/).
- Sibling reports: [subscription harnesses](2026-10-subscription-harnesses.md), [app hooks](2026-10-app-hooks.md), [prior art](2026-10-prior-art.md).

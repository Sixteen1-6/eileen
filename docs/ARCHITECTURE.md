# Eileen architecture

This is the engineering design contributors build against. It is written so that two people can implement different modules without talking to each other: every process boundary, message shape and policy file is spelled out here. Evidence for the choices lives in [RESEARCH.md](RESEARCH.md); this document cites it by section number instead of repeating it.

Six principles constrain everything below. The user is the supreme router. Conversation never blocks on work. Deterministic tools come before vision. Reversible actions run immediately, consequential ones confirm first. We wrap agent harnesses and never touch their credentials. We compose instead of forking.

## 1. Overview and the two-loop model

An MCP voice tool inside an agent cannot listen while a tool call runs (RESEARCH.md section 4). So Eileen has two kinds of loop:

| Loop | Owns | Blocks on work | Model |
|---|---|---|---|
| Voice loop | Microphone, speaker, the conversation transcript | Never | Fast conversational model (Haiku 4.5 or a local 4B to 8B model) |
| Agent loops | One coding, research or simulation task each | Yes, that is their job | Whatever the router picked |

The voice loop treats every agent session as an async job. It can start one, ask how it is going, redirect it, cancel it, or answer an approval request, all while the job keeps running. Jobs report back through an event bus; the voice loop decides when and whether to speak about them.

### Components and process boundaries

```
  mic / speaker                                                phone
       ^                                                         ^
       |                                              Telegram / ntfy
+------+-----------------------------------------+          +-----+-----------+
|  eileen-core  (Python 3.12, one process)       |  WS +    | eileen-channels |
|                                                |  token   | Telegram, ntfy  |
|  +------------+  +---------------+  +--------+ |<-------->| (Python)        |
|  | voice loop |<>| coordinator   |<>| router | |          +-----------------+
|  | AEC VAD    |  | job registry  |  | policy | |
|  | STT LLM    |  | event bus     |  | LiteLLM| |
|  | TTS        |  +--+---------+--+  +--------+ |
|  +------------+     |         |                |
|             MCP clients    ACP client          |
+---------------------+---------+----------------+
                      |         |
  MCP (stdio / pipe)  |         |  ACP (JSON-RPC over stdio)
  +--------+----------+---+     +---------+-----------+
  |        |          |   |     |         |           |
+-v------+ +v-------+ +v----+ +v------+ +v-------+ +v-------+ +v---------+
|Desktop | |Cua     | |Play-| |eileen-| |claude- | |codex-  | |later:    |
|State   | |Driver  | |wright| |apps  | |agent-  | |acp     | |gemini,   |
|sidecar | |daemon  | |MCP  | |COM +  | |acp     | |        | |opencode  |
|Rust/C# | |Rust    | |Node | |SMTC   | |        | |        | |          |
+--------+ +--------+ +-----+ +-------+ +--------+ +--------+ +----------+
```

| Process | Language | Transport to core | Why separate |
|---|---|---|---|
| eileen-core | Python 3.12 | n/a | The only process that holds conversation state |
| DesktopState sidecar | Rust or C# | MCP over stdio | Needs COM apartments, WinEvent hooks and a message pump; must not stall the asyncio loop |
| Cua Driver daemon | Rust (upstream, MIT) | MCP over its named pipe (RESEARCH.md section 2.2) | Third-party binary with its own lifecycle |
| Playwright MCP | Node 24 | MCP over stdio | Third-party, talks to the browser extension |
| eileen-apps | Python | MCP over stdio | COM calls can hang on modal dialogs; isolate them |
| Harness adapters | Vendor binaries | ACP, JSON-RPC over stdio | One process per job, killable |
| eileen-channels | Python | Loopback HTTP and WebSocket, token protected | Keeps network-facing code out of the core |

## 2. DesktopState

DesktopState answers "what is the user looking at" as one object. Nothing off the shelf exposes it (RESEARCH.md section 2.2), so it is our sidecar. All rects are physical pixels in virtual-screen coordinates. The origin is the primary monitor's top-left; negative values are normal for monitors to the left or above.

```json
{
  "captured_at": "2026-09-19T21:04:12.318Z",
  "virtual_desktop_id": "{6B2F0A1C-9E3D-4F7A-8C21-0D5E7B9A4F10}",
  "monitors": [
    {
      "ordinal": 1,
      "gdi_name": "\\\\.\\DISPLAY2",
      "friendly_name": "DELL U2723QE",
      "rect": { "x": -3840, "y": 0, "w": 3840, "h": 2160 },
      "work_rect": { "x": -3840, "y": 0, "w": 3840, "h": 2100 },
      "dpi": 144,
      "is_primary": false
    },
    {
      "ordinal": 2,
      "gdi_name": "\\\\.\\DISPLAY1",
      "friendly_name": "LG ULTRAGEAR",
      "rect": { "x": 0, "y": 0, "w": 2560, "h": 1440 },
      "work_rect": { "x": 0, "y": 0, "w": 2560, "h": 1392 },
      "dpi": 96,
      "is_primary": true
    }
  ],
  "windows": [
    {
      "hwnd": 1247890,
      "pid": 20312,
      "process": "chrome.exe",
      "title": "Router explanation by Sixteen1-6 - Pull Request #12 - GitHub",
      "monitor_ordinal": 1,
      "rect_px": { "x": -3840, "y": 0, "w": 3840, "h": 2100 },
      "is_cloaked": false,
      "z_order": 0,
      "is_foreground": true
    }
  ],
  "foreground": { "hwnd": 1247890, "pid": 20312 },
  "focused_element": {
    "control_type": "Edit",
    "name": "Leave a comment",
    "automation_id": "new_comment_field",
    "rect_px": { "x": -2100, "y": 1410, "w": 900, "h": 180 },
    "patterns": ["Text", "Value", "Scroll"],
    "window_hwnd": 1247890
  },
  "selection": {
    "text": "the router explains its choice in one sentence",
    "source": "uia_textpattern"
  },
  "caret_rect_px": { "x": -1620, "y": 1462, "w": 1, "h": 22 },
  "recent_references": [
    { "kind": "selection", "label": "\"the router explains...\" in Chrome", "at": "2026-09-19T21:03:58Z" },
    { "kind": "job", "job_id": "01K5J8Q2X7", "label": "wind-sim", "at": "2026-09-19T21:01:44Z" }
  ],
  "degraded": []
}
```

- `selection.source` is one of `uia_textpattern`, `clipboard_fallback`, `none`. When `none`, `text` is null. `caret_rect_px` is null when no caret is known.
- `recent_references` holds at most eight entries, newest first, of kind `window`, `element`, `selection`, `file` or `job`. The core appends `job` entries; the sidecar appends the rest when focus, foreground or selection changes.
- `degraded` lists field names that hit a timeout on this capture. Consumers treat those fields as unknown, not empty.

### How it is built

| Concern | Mechanism |
|---|---|
| Window list | `EnumWindows`, then `DwmGetWindowAttribute` for `DWMWA_CLOAKED` and `DWMWA_EXTENDED_FRAME_BOUNDS`; the extended frame is the rect we report, so invisible resize borders are excluded. Sorted by z-order, top first; cloaked, zero-size and tool windows are dropped unless asked |
| Window to monitor | `MonitorFromWindow` with `MONITOR_DEFAULTTONEAREST` |
| Monitor list | `EnumDisplayMonitors` joined with `QueryDisplayConfig` so each GDI name gets its EDID friendly name |
| Monitor ordinals | Our own: sort by `rect.x`, then `rect.y`, left to right. Settings' Identify numbers are not exposed by any public API (RESEARCH.md section 2.2) |
| Foreground, focus, move, name, cloak events | `SetWinEventHook` on a dedicated thread that runs a message pump; events are coalesced for 30 ms before a new capture |
| Focused element | UI Automation focus-changed handler registered from an MTA thread; the handler only records the element and wakes the capture thread |
| Selection | `IUIAutomationTextPattern::GetSelection` on the focused element. If the pattern is missing or times out, the sidecar watches `GetClipboardSequenceNumber` and reports the user's last copy as `clipboard_fallback`. It never sends a synthetic copy keystroke of its own |
| DPI | Process is per-monitor-V2 DPI aware; every rect is physical pixels; `dpi` per monitor lets consumers convert |
| Timeouts | Every UIA call runs under a timeout (250 ms default, 1 s for TextPattern). A timeout marks the field in `degraded` and never blocks the capture |
| Virtual desktop | `IVirtualDesktopManager::GetWindowDesktopId` for the foreground window |

### MCP tools exposed

The core registers the sidecar under the MCP server name `eileen-desktop` so its tools never collide with Cua Driver's.

| Tool | Arguments | Returns |
|---|---|---|
| `desktop.state` | `since` (optional timestamp) | Full state, or a delta when `since` is given |
| `desktop.subscribe` | `events`: subset of `foreground`, `focus`, `selection`, `move`, `monitors` | Acknowledgement; the sidecar then sends MCP notifications named `notifications/eileen/desktop_event` carrying a delta |
| `window.move` | `hwnd`, `x`, `y`, `w`, `h` | Final rect. Uses `SetWindowPos` and compensates for the extended frame so the visible edge lands where asked |
| `window.snap` | `hwnd`, `monitor_ordinal`, `zone` (`left`, `right`, `top`, `bottom`, `top_left`, `top_right`, `bottom_left`, `bottom_right`, `full`), `fraction` (default 0.5) | Final rect, computed against `work_rect` |
| `window.focus` | `hwnd` | `taken: bool`. Uses `SetForegroundWindow` with the thread-input attach workaround; reports honestly when Windows refused |
| `window.minimize`, `window.maximize` | `hwnd`, `restore` (optional, maximize only) | ok |
| `monitor.list` | none | The `monitors` array alone |
| `selection.get` | `hwnd` (optional) | `{ text, source }` |

## 3. Action layer and the escalation ladder

The core never chooses a rung by habit. For each action it walks the ladder top down and takes the first rung whose adapter reports `can_handle(target) == True`. The chosen rung is logged with the job so "why did you click that" has an answer.

| Rung | Interface | Used for | Source |
|---|---|---|---|
| 1 | App Actions (Windows, typed, ISV opt-in) | Any app that publishes them | RESEARCH.md section 2.4 |
| 2 | COM and app scripting | Word, Excel, PowerPoint, Outlook via the running object table (`GetActiveObject`); Explorer via `Shell.Application`; Photoshop via `Photoshop.Application`; DaVinci Resolve Studio via its Python API; VEGAS via our in-process extension DLL over a named pipe; media via SMTC (`Windows.Media.Control`) | RESEARCH.md section 2.4 |
| 3 | UI Automation patterns | Invoke, Value, Toggle, Scroll, Text via Cua Driver background element actions; window state via the DesktopState sidecar | RESEARCH.md section 2.2 |
| 4 | Coordinate input with foreground escalation | Custom-drawn controls, games, apps with no UIA tree; Cua Driver brings the window forward only when a background action is refused | RESEARCH.md section 2.2 |
| 5 | Browser DOM | Playwright MCP with the browser extension: accessibility snapshot with refs, find, JS eval, real logged-in tabs in Chrome and Edge | RESEARCH.md section 2.3 |
| 6 | Vision | Anthropic `computer_toolset_20260801` driving the screenshot-and-input loop through Cua Driver | RESEARCH.md section 2.5 |

Rung 5 sits below rung 4 in the table but is tried first whenever the target is web content inside a browser window. The order for a browser tab is 5, then 3 for browser chrome, then 4, then 6.

Rung-specific notes:

- Resolve has no split API: "split at playhead" is `SetCurrentTimecode` then Ctrl+B through rung 4. VEGAS has no out-of-process API; the extension DLL is a v0.2 item, so VEGAS is rungs 3, 4 and 6 until then (RESEARCH.md section 2.4).
- Chromium and Electron targets need foreground delivery for input (RESEARCH.md section 2.2). Expect rung 4 to escalate for them.
- Rung 6 is slow. Multi-step vision runs take tens of minutes on the public benchmarks (RESEARCH.md section 2.5). Any job that hits rung 6 more than three times gets `salience` raised to 0.9 so the user hears about it.

Chrome constraints from RESEARCH.md section 2.3, restated as rules:

1. Never open raw CDP against the default profile. Since Chrome 136 it is refused unless the user opts in through `chrome://inspect`. The extension path is the default; Chrome DevTools MCP with `--autoConnect` is an opt-in alternative for Chrome 144 and later, and does not cover Edge.
2. Do not use UIA to read web content. Chrome enables web accessibility trees only after it notices an assistive-technology client. Use the extension for the page and UIA for tabs, address bar and dialogs.
3. Selection in a page is a JS snippet (`getSelection`, `setBaseAndExtent`) plus a copy keystroke. No tool exposes it as a field, so the browser adapter implements `selection.get` itself and DesktopState reports `clipboard_fallback` for browser windows.

Every adapter, whatever its rung, implements `ActionAdapter` in `eileen.desktop.actions`: a `rung` number, `can_handle(target)`, `async perform(action, target)` returning an `ActionResult`, and `reversible(action)`. The last one feeds the permission tiers in section 7.

## 4. Voice loop

The voice loop lives in `eileen.voice`. It is Pipecat if Pipecat runs natively on Windows, otherwise a custom asyncio loop on RealtimeSTT and RealtimeTTS (RESEARCH.md section 4; native Windows support is unverified, see section 12). The rest of this section holds either way.

### Audio ownership and echo cancellation

One audio task owns capture and playback. Nothing else opens the microphone. Played PCM is fed to pywebrtc-audio AEC3 as the far-end reference so the assistant does not hear itself. Capture is 16 kHz mono; playback is whatever the TTS produces, resampled once.

### VAD and turn end

Silero VAD gates the microphone. Smart Turn v3 decides semantic end-of-turn on CPU (8 MB model, RESEARCH.md section 4). A turn ends when Smart Turn says so, or after 800 ms of silence if it is undecided. Wake word is off by default; when the loop is always on, a "was that for me" check on the conversational model replaces it (RESEARCH.md section 4).

### STT

| Stage | Default | Alternative |
|---|---|---|
| Live partials | sherpa-onnx with Nemotron streaming 0.6B INT8 | Moonshine v2 Medium (pure MIT, CPU) |
| Finals | sherpa-onnx with Parakeet TDT 0.6B v3 | Deepgram Flux or Nova-3 when the hosted profile is selected |

Partials drive barge-in and the "was that for me" check. Finals go to the conversational model.

### Conversational model and per-turn context

Default: Claude Haiku 4.5 through LiteLLM. Free option: a local 4B to 8B instruct model. The system prompt is fixed; three blocks are rebuilt every turn and appended after it. Together they stay under roughly 600 tokens.

```
[desktop]
monitor 1 (left, 3840x2160, 144 dpi): Chrome "Pull Request #12 - GitHub" (foreground)
monitor 2 (right, 2560x1440, 96 dpi): Windows Terminal "pwsh in eileen"; Spotify
focused: Edit "Leave a comment" in Chrome
selection (uia): "the router explains its choice in one sentence"
recent: selection above; window Windows Terminal; job wind-sim

[jobs]
* login-tests   coding      Claude Agent  running 4m    2 of 5 tests passing
  wind-sim      simulation  Codex         blocked       needs approval: pip install numpy
  cite-check    research    Flash         done 2m ago   12 citations verified
focus: wind-sim

[policy]
coding -> Claude Agent / Sonnet 5; research -> Flash; simulation -> Codex; private -> local
active overrides: none
```

Rules for the blocks: windows are capped at twelve, sorted by z-order; selection text is truncated to 300 characters; jobs are recency-ordered with the focus job marked; the policy line is generated from the YAML in section 6 and any live `Override`.

### TTS

Kokoro-82M through RealtimeTTS or kokoro-onnx. Chatterbox-Turbo when the user configures a cloned voice; Pocket TTS on CPU as fallback (RESEARCH.md section 4). TTS emits word-boundary timestamps so the loop knows what was actually spoken.

### Barge-in

When VAD reports speech while playback is running, playback stops within 150 ms. The loop then:

1. Reads the last word-boundary timestamp from TTS.
2. Commits only the spoken prefix to the transcript as the assistant turn, followed by `[interrupted]`.
3. If the interrupted text was an announcement, marks that event `partial` and leaves it queued so the model can finish the thought when asked.
4. Feeds the user's speech to the model with the prefix visible, so the model knows what the user heard.

### Announcement policy

| Event | Behavior at the PC | When away |
|---|---|---|
| `ApprovalRequested` | `interrupt`: short chime, then one sentence, interruptible | `phone` with Approve and Deny buttons |
| `TaskBlocked` | `interrupt` | `phone` |
| `TaskFailed` | `interrupt` | `phone` |
| `TaskCompleted` | `when_idle`: end of the user's turn plus one second of silence | `phone` |
| `TaskProgressed` | `silent` unless the user asked about that job in the last two minutes | `silent` |
| `TaskStarted`, `TaskAccepted` | `silent` | `silent` |
| `TaskCancelled` | `when_idle` if the user did not cancel it themselves | `phone` |

Presence rules that decide "away" are in section 9. The table is data, stored in `announcements.yaml`, and the user can change a row by voice ("tell me about progress on the simulation").

## 5. Job registry and event bus

Both live in `eileen.core`. The registry is SQLite at `%LOCALAPPDATA%\eileen\eileen.db`; the bus is in-process asyncio pub/sub with a loopback HTTP and WebSocket endpoint for other processes. Every event is also appended to an `events` table (`job_id`, `type`, `payload` JSON, `at`, `delivered`) for replay and audit.

### Schema

```sql
CREATE TABLE jobs (
  id              TEXT PRIMARY KEY,       -- ULID
  name            TEXT NOT NULL,          -- short label, e.g. "fix login tests"
  slug            TEXT NOT NULL UNIQUE,   -- e.g. "login-tests"
  kind            TEXT NOT NULL,          -- coding | research | architecture | simulation | trivial
  harness         TEXT NOT NULL,          -- claude-agent | codex | gemini | opencode | direct
  model           TEXT,                   -- alias from routing.yaml, null for harness default
  status          TEXT NOT NULL,          -- accepted | running | blocked | awaiting_approval
                                          -- | completed | failed | cancelled
  origin          TEXT NOT NULL,          -- voice | telegram | ntfy | cli | cron
  created_at      TEXT NOT NULL,          -- ISO 8601 UTC
  updated_at      TEXT NOT NULL,
  last_progress   TEXT,                   -- one line, model-visible
  salience        REAL NOT NULL DEFAULT 0.5,  -- 0 to 1, drives announcement order
  transcript_path TEXT,                   -- JSONL of the ACP session
  cwd             TEXT
);
```

### Event types

| Event | Payload | Sets status |
|---|---|---|
| `TaskAccepted` | `job_id`, `name`, `kind`, `harness`, `model`, `route_reason` | `accepted` |
| `TaskStarted` | `job_id`, `pid`, `cwd` | `running` |
| `TaskProgressed` | `job_id`, `line` | `running`, updates `last_progress` |
| `TaskBlocked` | `job_id`, `reason` | `blocked` |
| `ApprovalRequested` | `job_id`, `approval_id`, `tier`, `action`, `summary` | `awaiting_approval` |
| `TaskCompleted` | `job_id`, `summary`, `artifacts[]` | `completed` |
| `TaskFailed` | `job_id`, `error` | `failed` |
| `TaskCancelled` | `job_id`, `by` (`user`, `policy`, `timeout`) | `cancelled` |

Pydantic models for these live in `eileen/core/events.py`. The event name is the class name; the wire form is `{"type": "TaskProgressed", "at": "...", "payload": {...}}`.

### How events are published

- ACP adapters translate `session/update` notifications into `TaskProgressed` with a compact line, `session/request_permission` into `ApprovalRequested`, and end-of-turn into `TaskCompleted` or `TaskBlocked`.
- Harnesses with native hooks (Claude Code fires `agent_needs_input` and `agent_completed`, RESEARCH.md section 6) run a tiny hook script that POSTs to `http://127.0.0.1:<port>/events` with a per-session token. This is a backstop; ACP is the primary path.
- Processes outside the core (channels, eileen-apps) subscribe over the WebSocket endpoint and receive the same wire form.

### Job names

The conversational model produces a two to four word label from the prompt at accept time, in the style of Claude Code's agents view labels (RESEARCH.md section 6). The slug is the label lowercased, hyphenated, with stop words removed. If the slug collides with any job from the last seven days, a distinguishing noun from the prompt is appended, then a numeric suffix as a last resort. Labels are never regenerated after the user has heard them.

### Pronoun resolution

Order of rules, first match wins:

1. An explicit name or slug in the utterance.
2. A kind word ("the simulation", "that research") filters by `kind`.
3. A harness word ("what is Codex doing") filters by `harness`.
4. Among the remaining candidates, prefer the focus job, then the most recently updated.
5. If two candidates remain and neither is the focus job, ask a one-line question naming both.
6. "It" immediately after an announcement means the announced job; announcing sets the focus job.

## 6. Routing

The router in `eileen.core.router` maps a task to a harness and a model. Inputs, in order of precedence: a live `Override`, then `routing.yaml`, then built-in defaults. Direct model calls (kind `research`, `trivial`, and anything with `harness: direct`) go through LiteLLM (MIT, RESEARCH.md section 6) with per-request parameters. When the user does not name a kind, the conversational model classifies the prompt and falls back to `coding`.

### Policy file

Model names in the policy are aliases. The LiteLLM model string for each alias lives in `litellm.yaml`, because vendor IDs change and the policy should not.

```yaml
defaults:
  harness: claude-agent
  model: sonnet-5
  cost_tier: standard      # free | cheap | standard | frontier
  local_only: false
  confirm_above_tier: 1    # permission tier that needs confirmation (section 7)

conversation:
  model: haiku-4-5
  local_fallback: local-8b # any 4B to 8B instruct model served locally

kinds:
  coding:
    harness: claude-agent
    model: sonnet-5
    cost_tier: standard
  research:
    harness: direct
    model: gemini-3-8-flash
    cost_tier: cheap
  architecture:
    harness: claude-agent
    model: opus-5
    cost_tier: frontier
  simulation:
    harness: codex
    model: gpt-5-6-terra
    cost_tier: standard
    cwd: ~/sims
  trivial:
    harness: direct
    model: haiku-4-5
    cost_tier: cheap
  private:
    harness: direct
    model: local-8b
    cost_tier: free
    local_only: true       # never leaves the machine, whatever the override says

workers:
  subscription_backed: true  # false disables Claude Agent and Codex sessions that draw on a personal plan
  api_keys_first: true       # prefer ANTHROPIC_API_KEY, CODEX_API_KEY, Bedrock, Vertex, Foundry when set

debate:
  members: [claude-agent, codex, gemini]
  judge: opus-5
```

`local_only: true` on a kind is the one thing an override cannot lift. It exists so "private" stays private even when the user says "send everything to Claude" in the same breath.

### Spoken overrides

The conversational model parses phrases such as "no, send this straight to Claude", "keep this local" and "have all three debate it" into one `Override`:

```python
class Override(BaseModel):
    target: str | None = None          # job slug, or None for the next task
    harness: list[str] | None = None   # ["claude-agent"], or several for fanout or debate
    model: str | None = None           # alias
    local_only: bool | None = None
    mode: Literal["single", "fanout", "debate"] = "single"
    scope: Literal["next_task", "this_job", "session"] = "next_task"
    spoken: str                        # the phrase, verbatim, for the explanation
```

An `Override` with `target` set reroutes a running job: the current session is cancelled with `TaskCancelled(by="user")`, and a new job is accepted with the same prompt, the same `cwd` and the old transcript attached as context. Scope `session` overrides last until the user says otherwise or the core restarts. Overrides are stored in memory only; the policy file is never rewritten by voice.

### Explaining a choice

When asked why, the router answers in one sentence built from a template: "`<kind>` goes to `<harness>` on `<model>` because `<reason>`", where reason is either "that is the policy for `<kind>`" or "you said `<spoken>`". Example: "Simulation goes to Codex on GPT-5.6 Terra because that is the policy for simulation."

## 7. Permission tiers

| Tier | Name | Examples | Behavior |
|---|---|---|---|
| 0 | Observe | Read DesktopState, take a screenshot, list files, read a page | Silent |
| 1 | Reversible | Move, snap or focus a window; open a site; run tests; draft an email; edit files in a git repository; create a branch; play or pause media | Execute immediately, log it |
| 2 | Consequential | Send a message or email; delete files outside a repository; spend money; push to a shared branch; install software; change settings outside Eileen's own | Confirm first |
| 3 | Blocked | Enter credentials; make payments; permanently delete without confirmation; disable security features; bypass a CAPTCHA | Never. Explain and hand the step to the user |

"Reversible" means: undoable within Eileen in one step, or confined to a git working tree, or a change of view rather than data. Each `ActionAdapter.reversible()` decides per action; adapters must default to tier 2 when unsure.

Confirmation by voice: one sentence naming the action and its object ("Send the draft to Priya?"), then a 20 second window for yes or no. An unclear answer gets one repeat. Silence counts as no and is logged as `declined`. "Always allow that for this job" grants a per-job exception stored on the job, never a global one. Global exceptions are edited in `permissions.yaml` by hand.

Confirmation by phone: the same sentence as a Telegram message with Approve, Deny and Show more buttons, or an ntfy notification with action buttons. Every approval carries a nonce; a stale nonce is ignored. The message names the harness, the job, and the exact command or a diff summary.

Hard blocks, enforced in the core regardless of harness or adapter:

- No credential entry of any kind, including into vendor login prompts.
- No payments and no financial transfers.
- No permanent deletes without an explicit confirmation naming the target.
- No changes to system or security settings.
- No keystroke sequences on a hard-blocked list (`Win+L`, `Alt+F4` on an unknown window, `Ctrl+Alt+Del`), in the spirit of Hermes's blocked combos (RESEARCH.md section 6).

## 8. Harness adapters

ACP (Agent Client Protocol) is the contract (RESEARCH.md section 6). Eileen is an ACP client; each harness is an ACP agent spawned as a subprocess. First adapters: `claude-agent-acp` for Claude and `codex-acp` for Codex. Gemini CLI and OpenCode follow once the first two are stable.

What every adapter in `eileen.harness` must do:

1. Spawn the unmodified vendor binary in the user's own environment. No patched binaries, no spoofed headers, no look-alike system prompts (RESEARCH.md section 3).
2. Never touch credentials. Do not read `~/.claude` or `~/.codex/auth.json`, do not run OAuth flows, do not proxy or store tokens. If the binary is not signed in, surface the vendor's own message and stop.
3. Treat API keys as first-class: `ANTHROPIC_API_KEY`, `CODEX_API_KEY`, Bedrock, Vertex and Foundry configurations are passed through the environment untouched.
4. Honor `workers.subscription_backed: false` by refusing to start a session that would draw on a personal plan, so users can comply the day a policy changes (RESEARCH.md section 3).
5. Stream ACP session updates to the event bus as the events in section 5.
6. Map `session/request_permission` to the tiers in section 7. The adapter never auto-approves anything above the job's `confirm_above_tier`.
7. Run one session per job in the job's `cwd`, kill the subprocess on `TaskCancelled`, and write the full ACP transcript to `transcript_path` as JSONL.

```python
class HarnessAdapter(Protocol):
    name: str                                   # "claude-agent" | "codex" | "gemini" | "opencode"
    display_name: str                           # "Claude Agent" | "Codex"
    async def start(self, job: Job) -> None: ...
    async def prompt(self, job_id: str, text: str, attachments: list[Path] = ()) -> None: ...
    async def approve(self, job_id: str, approval_id: str, allow: bool) -> None: ...
    async def cancel(self, job_id: str) -> None: ...
    def events(self) -> AsyncIterator[Event]: ...
```

Naming rule: workers are called "Claude Agent" or "Codex" in every prompt, log line, phone message and doc. Never "Claude Code <anything>" (RESEARCH.md section 3). The `display_name` field is the only place the string is defined.

Parallelism comes from many sessions, not from harness-level teams, because Claude Code agent teams do not spawn in headless mode (RESEARCH.md section 6). Fanout and debate modes start one session per member and a judge call through LiteLLM.

## 9. Channels

`eileen-channels` is a separate Python process. It holds the only network-facing code and talks to the core over the loopback endpoint with a token.

| Channel | Transport | Used for |
|---|---|---|
| Telegram bot | Long polling (works behind NAT, RESEARCH.md section 5) | Announcements, approvals with inline buttons, voice notes both ways (transcribed by the same STT as the voice loop), new tasks |
| ntfy | Push to a self-hosted or public topic | Short notifications with action buttons when Telegram is too heavy |
| Tailscale, Discord | Private network; voice connection | SSH and a later PWA; live two-way voice in v0.2 (RESEARCH.md section 5) |

### Presence

The voice loop writes `%LOCALAPPDATA%\eileen\presence.json` every 30 seconds with `last_voice_activity`, `last_input_activity` (from `GetLastInputInfo`) and `state`. State is `present`, `away` or `phone`.

- `present` while there has been voice or input activity in the last N minutes (default 10).
- `away` after N minutes of neither, or immediately after "I'm heading out".
- `phone` while the last user message came from Telegram, until voice activity resumes at the PC.

### Routing events to the phone

The channel process subscribes to the same bus. For each event it reads the announcement table (section 4) and the presence state. When state is `away` or `phone`, rows marked `phone` are sent; `interrupt` and `when_idle` rows are held and the core replays them when the user is back, collapsed into one summary if more than three accumulated. Replies from the phone, including approvals, are events with `origin: telegram` and follow the same path as spoken ones.

## 10. Security posture

- Secrets never enter model-visible text. API keys and bot tokens are read from the environment or from an encrypted store, injected into subprocess environments, and redacted from every transcript and log with a regex pass before write (the IronClaw boundary, RESEARCH.md section 2.1).
- Encryption at rest. The SQLite database, transcripts and the secret store are encrypted with a key held in Windows DPAPI for the current user. OpenClaw's unencrypted store is the cautionary example (RESEARCH.md section 2.1).
- Sandbox by default. Harness sessions run with the vendor's own sandbox on, in the job's `cwd`, with no access to Eileen's config directory. Direct model calls get no tools unless the kind grants them.
- Everything on screen is untrusted input. Window titles, page text, selections and OCR output are data. The conversational model's system prompt says so, and any instruction-shaped text from those sources is quoted back to the user, never followed.
- Prompt injection stance. A tool result cannot raise a permission tier, cannot create an `Override`, and cannot approve an `ApprovalRequested`. Only the user, by voice or from the phone with a valid nonce, can do those three things.
- Loopback only. The event bus binds to 127.0.0.1 with a per-boot token. Nothing in Eileen listens on a public interface; the phone reaches the PC through Telegram's servers, ntfy or Tailscale. Vendor credentials are never handled (section 8), which is a security boundary as much as a legal one.

## 11. Language and process choices

| Choice | Reason |
|---|---|
| Python 3.12 core, `src` layout, pydantic, pytest, ruff | The voice stack (RealtimeSTT, RealtimeTTS, sherpa-onnx, Silero), LiteLLM, the MCP client SDK and the ACP client are all reachable from Python; asyncio matches the two-loop model |
| Rust or C# for the DesktopState sidecar | WinEvent hooks need a thread with a message pump, UIA needs a real COM apartment, and Python's UIA bindings have no events (RESEARCH.md section 2.2). Rust uses uiautomation-rs and the windows crate; C# uses FlaUI.UIA3. The first implementer decides; the Rust toolchain is not installed on the owner's machine today, which favors C# for the first cut |
| MCP between every process | Sidecars crash and restart without touching conversation state, any of them can be swapped, and the same servers can be registered with Windows' On-Device Registry later (RESEARCH.md section 2.4). Node 24 exists only because Playwright MCP runs on it |
| ACP for harnesses | One protocol for Claude, Codex, Gemini CLI and OpenCode; Goose and OpenClaw already consume it (RESEARCH.md section 6) |
| Separate `eileen-apps` and `eileen-channels` processes | COM can hang, and network code should not share a process with the microphone owner |

Repo layout: `src/eileen/{core,voice,desktop,browser,apps,harness,channels}`, `docs/`, `tests/`, `.github/`. The sidecar lives in `src/desktopstate/` in its own language with its own build, and ships as a single executable the core spawns.

## 12. Open questions

1. Does Pipecat run natively on Windows? Unverified (RESEARCH.md section 4). If not, the custom loop on RealtimeSTT and RealtimeTTS is the path, and Smart Turn is wired in by hand.
2. Rust or C# for the sidecar. C# is faster to start on this machine; Rust matches Cua Driver and terminator. Decided by whoever builds it first.
3. Local or hosted default for the conversational model. Haiku 4.5 is the quality floor; a local 8B model is the cost floor. The answer may be "hosted when API keys are present, local otherwise".
4. How far to go with OpenClaw. As an optional gateway it brings channels, cron and mobile apps; its security record argues for keeping it at arm's length (RESEARCH.md section 2.1).
